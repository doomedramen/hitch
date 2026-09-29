//! The rebuild planner and executor.
//!
//! One operation, end to end, so that the three later operations have a shape
//! to copy rather than invent. The shape is:
//!
//! ```text
//!   plan_rebuild ──▶ OperationPlan ──▶ validate_plan ──▶ apply_rebuild_plan ──▶ ExecutionReceipt
//!        │                                                                ▲
//!        └── discard_plan (preview, or a plan that will not be applied) ───┘
//! ```
//!
//! The planner is not a reimplementation of the build. It is
//! [`compose_environment`](crate::utils::prelude::compose_environment) plus a
//! description of what publishing it will do — which is the honest meaning of
//! "plan": the merges have already happened by the time a plan exists, so the
//! plan summarises a decision rather than proposing one.

use anyhow::{Context, Result};

use crate::commands::global_context::GlobalContext;
use crate::core::state::build_state_snapshot;
use crate::operations::model::{
    changed_inputs, AppliedEffect, CompositionPlan, ConfirmationRequirement, EnvironmentProjection,
    ExecutionReceipt, ExecutionWarning, OperationIntent, OperationKind, OperationOutcome,
    OperationPlan, PlanApplyError, PlanFingerprint, PlanWarning, PlannedBranch, PlannedBranchState,
    PlannedEffect, ResourceKind, UnaffectedResource,
};
use crate::types::OnConflict;
use crate::utils::build_record::{EnvironmentBuildRecord, ResolutionUse};
use crate::utils::git_operations::{GitOperations, RefEdit};
use crate::utils::prelude::{
    compose_environment, pin_environment_inputs, publish_environment_build, CompatibilityConflict,
    PushOutcome,
};

/// What a plan is *for*, which decides the three things that may and may not
/// happen while it is being built.
///
/// This is an enum rather than two `bool`s on purpose. Each variant is a
/// bundle of three properties that only make sense together, and the pairing
/// that matters is the one a preview must never get wrong:
///
/// | | synchronise | lock the environment | anchor the composed commit |
/// |---|---|---|---|
/// | [`PlanPurpose::Confirm`] | yes | yes | yes |
/// | [`PlanPurpose::Preview`] | **no** | no | **no** |
///
/// The synchronisation asymmetry is P1's, and it is deliberate: `pin_
/// environment_inputs(synchronize: true)` fetches, fast-forwards local
/// branches, and creates remote-only ones, so a *preview* that synchronised
/// would move the user's branches — a read-only command that writes. The
/// consequence (a preview reflects local refs while the build syncs first, so
/// a stale local branch can make the preview describe older content) is
/// ordinary staleness, and categorically weaker than the two-doors-into-the-
/// merge-engine bug P1 removed. It is not a bug to fix here; it is a property
/// to encode.
///
/// Encoding purpose rather than knobs is what makes "a preview that mutates"
/// unrepresentable, and what makes it obvious in review that a new caller has
/// to choose.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanPurpose {
    /// Shown to a human, then applied if it is still current.
    Confirm,
    /// Shown to a human, then discarded. Never writes a ref, never takes a
    /// lock, never reaches the network except through read-only local reads.
    Preview,
}

impl PlanPurpose {
    /// Public because `release::plan_release` consumes the same enum: a release
    /// synchronises and anchors too, and the safety property that makes a
    /// preview a preview is the *same* three-way property, not a per-planner
    /// re-derivation of it.
    pub fn synchronizes(self) -> bool {
        matches!(self, PlanPurpose::Confirm)
    }

    /// Only a plan that might be applied may leave a ref behind. A preview's
    /// composed commit is unreferenced, which is fine: it is the same
    /// situation `--dry-run` has always been in, and `git gc` collects it.
    pub fn anchors(self) -> bool {
        matches!(self, PlanPurpose::Confirm)
    }
}

/// The rebuild-specific payload a plan carries alongside the shared fields.
///
/// Two of these are the *plan's own* ref operations, computed once here and
/// applied verbatim later. Recomputing them at apply time would be a second
/// decision point, and the build record in particular is a claim about *this*
/// composition — a record rebuilt after the fact could describe a build that
/// did not happen.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct RebuildPlanDetail {
    pub environment: String,
    pub held: Vec<CompatibilityConflict>,
    pub replayed: Vec<ResolutionUse>,
    /// The ref anchoring the composed commit while the plan waits to be
    /// applied, or `None` for a preview.
    pub anchor_ref: Option<String>,
    /// Timestamp naming the archival `prev`/`backup` refs and the anchor. Part
    /// of the plan because the *names* of the refs the publish will write are
    /// effects, and a plan that cannot name them is not describing the publish.
    pub backup_timestamp: String,
    /// The remote environment tip observed *before* composing, so the push
    /// leases against what was observed rather than against whatever the remote
    /// has become by the time the build finishes.
    pub remote_env_sha_before: Option<String>,
    pub record: EnvironmentBuildRecord,
    pub state_edit: RefEdit,
}

impl RebuildPlanDetail {
    /// The ref the build record lives at, named rather than dug out of
    /// [`RefEdit::Update`] by callers.
    pub fn state_refname(&self) -> &str {
        match &self.state_edit {
            RefEdit::Update { refname, .. } | RefEdit::Create { refname, .. } => refname,
            RefEdit::Delete { refname, .. } => refname,
        }
    }
}

/// The per-operation options a caller chose. Deliberately not a clap type: the
/// planner must be callable from tests, from `rebuild_environment`, and
/// from a future non-CLI surface without dragging an argument parser in.
#[derive(Debug, Clone, Copy, Default)]
pub struct RebuildPlanOptions {
    /// `hitch rebuild --replay-resolutions`. See
    /// [`crate::utils::prelude::rebuild_environment_gated`] for why only the CLI
    /// ever sets this.
    pub replay: bool,
    /// `hitch rebuild --on-conflict`. `None` means the environment's own
    /// policy.
    pub on_conflict: Option<OnConflict>,
}

/// Build the plan for rebuilding `env_name`.
pub fn plan_rebuild(
    context: &GlobalContext,
    env_name: &str,
    options: RebuildPlanOptions,
    purpose: PlanPurpose,
) -> Result<OperationPlan<RebuildPlanDetail>> {
    let config = crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let environment = config
        .environments
        .get(env_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", env_name))
        .with_context(|| {
            format!(
                "Run 'hitch status' to see the environments in this repository, or \
                 'hitch add {}' to create it.",
                env_name
            )
        })?;
    let on_conflict = options.on_conflict.unwrap_or(environment.on_conflict);

    // Synchronise and pin. Everything below composes against these concrete
    // SHAs rather than the mutable branch names, so a ref moving mid-build
    // cannot change what gets composed. Shared with the preview, which is why
    // a preview cannot pin differently from the build it previews.
    let pinned = pin_environment_inputs(context, &environment, purpose.synchronizes())?;

    // The remote environment tip, read *before* composing so the eventual push
    // leases against what was observed. Reading it after the build would lease
    // against whatever the remote became in the meantime, which is precisely
    // the window a lease is supposed to close.
    let remote_env_ref = format!("refs/remotes/origin/{}", env_name);
    let remote_env_sha_before = context.git().rev_parse_opt(&remote_env_ref)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();

    // The one composition. `--dry-run` and the real build share this call
    // (P1), so a preview cannot disagree with the build it previews.
    let composition = compose_environment(
        context,
        &pinned,
        env_name,
        on_conflict,
        options.replay,
        config.require_signed_resolutions,
    )?;

    let record = build_record_for(context, env_name, &pinned, &composition)?;
    let (state_ref, state_blob) = crate::utils::build_record::record_blob(context.git(), &record)?;
    let state_edit = RefEdit::Update {
        refname: state_ref,
        new_oid: state_blob,
        // Unconditional overwrite, deliberately — see the comment at the
        // equivalent site in `rebuild_environment` and the AGENTS.md
        // gotcha on `refs/hitch/state/*`. A `Create` here would fail the
        // second rebuild of every environment, wedging it after exactly one
        // successful build.
        expected_old: Some(String::new()),
    };

    // Anchor the composed commit for the window between planning and
    // publishing, so a concurrent `git gc --prune=now` cannot collect the
    // object the plan names. This is the same anchor the old inline sequence
    // created between compose and publish, moved earlier in time — not a new
    // mechanism.
    let anchor_ref = if purpose.anchors() {
        let refname = format!("refs/hitch/build/{}/{}", env_name, timestamp);
        context
            .git()
            .update_ref(&refname, &composition.result_sha)
            .with_context(|| {
                format!(
                    "Failed to anchor the composed commit at '{}'. Nothing has been changed; \
                     re-run to try again.",
                    refname
                )
            })?;
        Some(refname)
    } else {
        None
    };

    assemble_plan(
        context,
        env_name,
        &config,
        &pinned,
        &composition,
        RebuildPlanDetail {
            environment: env_name.to_string(),
            held: composition.held.clone(),
            replayed: composition.replayed.clone(),
            anchor_ref,
            backup_timestamp: timestamp,
            remote_env_sha_before: remote_env_sha_before.clone(),
            record,
            state_edit,
        },
        &remote_env_ref,
        remote_env_sha_before,
    )
}

/// Build the environment's build record from the pinned inputs and what the
/// composition actually did.
///
/// Split out of [`plan_rebuild`] because the two subtleties in here are worth
/// reading without the rest of the planner around them, and because the same
/// construction has to be understood by whoever changes it next.
fn build_record_for(
    context: &GlobalContext,
    env_name: &str,
    pinned: &crate::utils::prelude::PinnedInputs,
    composition: &crate::utils::prelude::CompositionResult,
) -> Result<EnvironmentBuildRecord> {
    let pin = |(branch, sha): &(String, String)| crate::utils::build_record::PinnedBranch {
        branch: branch.clone(),
        sha: sha.clone(),
    };
    // `desired` is `pinned` verbatim; `included` is `pinned` *filtered by
    // membership*, never a `filter_map` over `composition.included` looking
    // each name up. Composition walks `pinned.branches` in order, so
    // `included` is a subsequence of it and this yields the same list in
    // promotion order — while a name that failed to look up would be silently
    // dropped, and a record that drops a branch lies about what was built.
    let desired: Vec<crate::utils::build_record::PinnedBranch> =
        pinned.branches.iter().map(pin).collect();
    let included: Vec<crate::utils::build_record::PinnedBranch> = pinned
        .branches
        .iter()
        .filter(|(branch, _)| composition.included.iter().any(|n| n == branch))
        .map(pin)
        .collect();

    Ok(EnvironmentBuildRecord::new(
        env_name,
        crate::utils::build_record::resolve_metadata_sha(context.git())?,
        pinned.base_name.clone(),
        pinned.base_sha.clone(),
        desired,
        included,
        composition.held.clone(),
        composition.replayed.clone(),
        composition.result_sha.clone(),
    ))
}

/// Turn the planner's intermediate results into the plan proper: the
/// composition view, the effect list, the fingerprint, and the warnings.
#[allow(clippy::too_many_arguments)] // one argument per distinct fact the plan records; a struct here would be a second model
fn assemble_plan(
    context: &GlobalContext,
    env_name: &str,
    config: &crate::types::HitchConfig,
    pinned: &crate::utils::prelude::PinnedInputs,
    composition: &crate::utils::prelude::CompositionResult,
    detail: RebuildPlanDetail,
    remote_env_ref: &str,
    remote_env_sha_before: Option<String>,
) -> Result<OperationPlan<RebuildPlanDetail>> {
    let git = context.git();
    let env_ref = format!("refs/heads/{}", env_name);
    let env_sha_before = git.rev_parse_opt(&env_ref)?;

    // One entry per declared branch, **in order**, so a reader can walk the
    // plan top to bottom and see the same sequence the composition walked.
    // `Missing` is the deliberate catch-all: a planner bug should render as
    // "this branch is accounted for nowhere", not as a silent `Included`.
    let replayed_by_branch: Vec<(&str, &str)> = detail
        .replayed
        .iter()
        .map(|r| (r.branch.as_str(), r.resolution_key.as_str()))
        .collect();
    let branches: Vec<PlannedBranch> = pinned
        .branches
        .iter()
        .map(|(branch, sha)| {
            let held = detail.held.iter().find(|c| &c.branch == branch);
            let replayed = replayed_by_branch
                .iter()
                .find(|(name, _)| *name == branch.as_str())
                .map(|(_, key)| (*key).to_string());
            let state = if let Some(conflict) = held {
                PlannedBranchState::Held {
                    conflicts_with: conflict.conflicts_with.clone(),
                    files: conflict.conflicted_files.clone(),
                }
            } else if let Some(resolution_id) = replayed {
                PlannedBranchState::ReplayedResolution { resolution_id }
            } else if composition.included.iter().any(|n| n == branch) {
                PlannedBranchState::Included
            } else {
                PlannedBranchState::Missing
            };
            PlannedBranch {
                branch: branch.clone(),
                sha: sha.clone(),
                state,
            }
        })
        .collect();

    let composition_plan = CompositionPlan {
        environment: env_name.to_string(),
        base: crate::utils::build_record::PinnedBranch {
            branch: pinned.base_name.clone(),
            sha: pinned.base_sha.clone(),
        },
        branches,
        result_sha: composition.result_sha.clone(),
        holds: detail.held.clone(),
    };

    let projection = |branch_sha: Option<String>| EnvironmentProjection {
        environment: env_name.to_string(),
        base: pinned.base_name.clone(),
        branches: pinned
            .branches
            .iter()
            .map(|(b, s)| crate::utils::build_record::PinnedBranch {
                branch: b.clone(),
                sha: s.clone(),
            })
            .collect(),
        branch_sha,
    };

    // Effects. A remote effect is predicted *only* when the command will
    // actually attempt a push — a plan that predicts a push the apply will
    // not make is exactly the false "fully synced" the receipt must never
    // produce.
    let mut effects = vec![
        PlannedEffect::LocalRefUpdate {
            refname: env_ref.clone(),
            old: env_sha_before.clone(),
            new: composition.result_sha.clone(),
        },
        PlannedEffect::MetadataChange {
            refname: detail.state_refname().to_string(),
            description: format!(
                "record of what '{}' last built (result {})",
                env_name,
                short(&composition.result_sha)
            ),
        },
        PlannedEffect::MetadataChange {
            refname: "refs/heads/hitch-metadata".to_string(),
            description: format!("'rebuilt_at' stamp for '{}'", env_name),
        },
    ];
    if let Some(anchor) = &detail.anchor_ref {
        // An anchor is a real ref hitch writes, and a plan that omits it is
        // hiding a write from the reader. It is listed as a metadata change
        // because that is the kind of ref it is, with a description that says
        // it is temporary.
        effects.push(PlannedEffect::MetadataChange {
            refname: anchor.clone(),
            description:
                "temporary anchor holding the composed commit until it is published, then removed"
                    .to_string(),
        });
    }
    if context.should_push() {
        effects.push(PlannedEffect::RemoteRefUpdate {
            refname: remote_env_ref.to_string(),
            old: remote_env_sha_before.clone(),
            new: composition.result_sha.clone(),
        });
    }

    // Unaffected resources come from the *same* pinned inputs the composition
    // consumed, so "will not change" cannot name a branch the plan never read.
    let mut unaffected = vec![UnaffectedResource {
        kind: ResourceKind::Branch,
        name: pinned.base_name.clone(),
    }];
    for (branch, _) in &pinned.branches {
        unaffected.push(UnaffectedResource {
            kind: ResourceKind::Branch,
            name: branch.clone(),
        });
    }
    for other in config.environments.keys() {
        if other != env_name {
            unaffected.push(UnaffectedResource {
                kind: ResourceKind::Environment,
                name: other.clone(),
            });
        }
    }

    // A held branch is a fact the user must see, not a reason to refuse.
    //
    // Note what is *absent*: a blocking warning for `on_conflict: halt`. There
    // is no such case, and adding one would have been a lie. `compose_
    // environment` decides the halt itself and returns `Err` with the refusal
    // report (`src/utils/prelude.rs`, the `OnConflict::Halt` arm) — so by the
    // time anything here could classify a branch, the policy has already been
    // enforced and no plan exists. A plan is downstream of composition by
    // construction; it cannot report a decision it never got to make. That is
    // a real constraint on the model worth writing down, because P5's
    // approvals are where a genuinely blocking plan-level warning comes from.
    let warnings: Vec<PlanWarning> = detail
        .held
        .iter()
        .map(|conflict| {
            PlanWarning::advisory(format!(
                "'{}' conflicts with '{}' and will be held out of this build ({} file(s))",
                conflict.branch,
                conflict.conflicts_with,
                conflict.conflicted_files.len()
            ))
        })
        .collect();

    // The fingerprint. Note what is *absent*: refs this plan does not depend
    // on. A ref that exists in the live repository and is missing from here is
    // not a change — see `PlanFingerprint`'s own doc comment.
    let mut fingerprint = PlanFingerprint::new();
    fingerprint.metadata_sha = git
        .rev_parse_opt("refs/heads/hitch-metadata")
        .unwrap_or(None);
    fingerprint.track_ref(format!("refs/heads/{}", pinned.base_name), &pinned.base_sha);
    for (branch, sha) in &pinned.branches {
        fingerprint.track_ref(format!("refs/heads/{}", branch), sha);
    }
    if let Some(sha) = &env_sha_before {
        fingerprint.track_ref(&env_ref, sha);
    }
    fingerprint.track_remote_ref(remote_env_ref, remote_env_sha_before);
    for use_ in &detail.replayed {
        fingerprint.track_resolution(use_.resolution_key.clone());
    }

    let digest = fingerprint.digest(git)?;

    Ok(OperationPlan {
        id: format!("rebuild:{}:{}", env_name, digest),
        kind: OperationKind::Rebuild,
        intent: OperationIntent::RebuildEnvironment {
            environment: env_name.to_string(),
        },
        fingerprint,
        current: Some(projection(env_sha_before)),
        proposed: Some(projection(Some(composition.result_sha.clone()))),
        compositions: vec![composition_plan],
        effects,
        unaffected,
        warnings,
        confirmation: if context.should_push() {
            ConfirmationRequirement::required(format!(
                "force-push the rebuilt '{}' branch to origin/{}",
                env_name, env_name
            ))
        } else {
            ConfirmationRequirement::not_required()
        },
        detail,
    })
}

/// The `old` value the plan predicted for `refname`, or `None` if the plan did
/// not predict an old value for it. A receipt's `old` is history, so it comes
/// from the plan — the repository no longer holds the replaced tip.
fn planned_old_of(plan: &OperationPlan<RebuildPlanDetail>, refname: &str) -> Option<String> {
    plan.effects.iter().find_map(|effect| match effect {
        PlannedEffect::LocalRefUpdate {
            refname: r, old, ..
        }
        | PlannedEffect::RemoteRefUpdate {
            refname: r, old, ..
        } if r == refname => old.clone(),
        _ => None,
    })
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// The plan's own cleanup: drop the anchor the planner created.
///
/// Called on **every** path where a plan will not be applied — a preview, a
/// plan the human declined, a plan that failed validation. A leaked anchor is
/// harmless (the commit just stays reachable under `refs/hitch/build/*`, which
/// nothing prunes — see `commands/cleanup.rs`'s prunable set, deliberately
/// `["backup", "prev"]`) but it is untidy and it grows without bound, so the
/// obligation is a named function rather than a `Drop` impl that cannot report
/// a failure.
pub fn discard_plan(context: &GlobalContext, plan: &OperationPlan<RebuildPlanDetail>) {
    if let Some(anchor) = &plan.detail.anchor_ref {
        let _ = context.git().delete_ref(anchor);
    }
}

/// Check that everything the plan depends on is still what it was.
///
/// Every difference becomes a [`ChangedInput`], named and SHAd, because a
/// refusal that cannot say what changed forces the user to diff the repository
/// themselves — which is the entire reason the fingerprint exists.
///
/// Note the direction of the comparison: only refs *in the fingerprint* are
/// checked. A ref the plan never depended on is not a change.
pub fn validate_plan(
    context: &GlobalContext,
    plan: &OperationPlan<RebuildPlanDetail>,
) -> std::result::Result<(), PlanApplyError> {
    let git = context.git();
    let env = plan.detail.environment.as_str();
    let mut changed = changed_inputs(&plan.fingerprint, git);

    // A resolution that has disappeared since the plan was built means the
    // replay would now miss and the branch would be *held* instead of
    // composed — a materially different operation, so a missing key is a
    // change rather than a non-event.
    for key in &plan.fingerprint.resolution_keys {
        if !resolution_exists(git, key) {
            changed.push(ChangedInput {
                branch: format!("resolution {}", short(key)),
                previous_sha: Some(key.clone()),
                current_sha: None,
            });
        }
    }

    if changed.is_empty() {
        Ok(())
    } else {
        // The environment is also `hitch rebuild`'s positional argument, so
        // passing it for both parameters is not a fudge: it is what the user
        // typed.
        Err(PlanApplyError::stale_plan(plan.kind, env, env, &changed))
    }
}

fn resolution_exists(git: &GitOperations, key: &str) -> bool {
    matches!(
        git.rev_parse_opt(&format!("refs/hitch/resolutions/{}", key)),
        Ok(Some(_))
    )
}

/// Land a plan that is still current, and report what happened.
///
/// The order below *is* the contract:
///
/// 1. validate — a plan that is not current is refused before anything moves.
/// 2. publish — the branch move and the build record, in one transaction.
/// 3. read the effects back and assemble the receipt.
///
/// Releasing the plan's anchor is deliberately **not** one of the numbered
/// steps: it is a `finally`, not a step, because it is owed on *every* exit
/// path rather than at a point in the sequence. A validation refusal in
/// particular is a path that is easy to forget — the `?` fires long before
/// anything else has run — and a missed anchor is a commit left reachable
/// under `refs/hitch/build/*`, a family nothing prunes (see
/// `commands/cleanup.rs`'s prunable set, deliberately `["backup", "prev"]`).
/// Getting that wrong once per path is a bug waiting to happen, so the
/// cleanup is a single unconditional call rather than one per exit.
pub fn apply_rebuild_plan(
    context: &GlobalContext,
    plan: &OperationPlan<RebuildPlanDetail>,
) -> Result<ExecutionReceipt> {
    let started_at = chrono::Utc::now();
    let outcome = apply_validated_plan(context, plan, started_at);
    discard_plan(context, plan);
    outcome
}

fn apply_validated_plan(
    context: &GlobalContext,
    plan: &OperationPlan<RebuildPlanDetail>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<ExecutionReceipt> {
    // Converted rather than `?`-ed so the typed error crosses the
    // `anyhow::Result` boundary intact; see `PlanApplyError::into_anyhow`.
    validate_plan(context, plan).map_err(PlanApplyError::into_anyhow)?;

    let env = &plan.detail.environment;

    // `state_edit` is reused verbatim. Rebuilding it here would be a second
    // decision point, and the record is a claim about *this* composition.
    //
    // `publish_environment_build` writes the `rebuilt_at` stamp itself, on
    // purpose: it is inside the same operation for `hitch resolve`'s Mode B and
    // for `rebuild_environment`, and a second write here would stamp the
    // environment twice for one rebuild. The plan *describes* the stamp as one
    // of its effects; the executor's callee is what performs it.
    let publish = publish_environment_build(
        context,
        env,
        &plan.compositions[0].result_sha,
        std::slice::from_ref(&plan.detail.state_edit),
        &plan.detail.backup_timestamp,
        &plan.detail.remote_env_sha_before,
    )?;

    assemble_receipt(context, plan, publish, started_at)
}

fn assemble_receipt(
    context: &GlobalContext,
    plan: &OperationPlan<RebuildPlanDetail>,
    publish: crate::utils::prelude::PublishOutcome,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<ExecutionReceipt> {
    let git = context.git();
    let env = &plan.detail.environment;

    // Effects are read back, never copied. The plan said what it expected; this
    // says what is there.
    let mut effects = Vec::new();
    let env_ref = format!("refs/heads/{}", env);
    let landed = git.rev_parse_opt(&env_ref)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Internal error: '{}' was published but refs/heads/{} does not resolve afterwards.",
            env,
            env
        )
    })?;
    // `landed` must be the commit the plan named. The CAS in `publish_branch`
    // makes a mismatch impossible, so this is a check on our own bookkeeping
    // rather than a diagnosis of the user's repository — and it is worth
    // having, because "applied the wrong thing" is the one failure a receipt
    // may never report as a success.
    if landed != plan.compositions[0].result_sha {
        return Err(anyhow::anyhow!(
            "Internal error: '{}' was published as {}, but refs/heads/{} reads {} afterwards.",
            env,
            plan.compositions[0].result_sha,
            env,
            landed
        ));
    }
    effects.push(AppliedEffect::LocalRefUpdate {
        refname: env_ref,
        // The *planned* old value, which is also the value the CAS in
        // `publish_branch` was given. It is carried here as history rather than
        // re-read, because the repository no longer holds it.
        old: planned_old_of(plan, &format!("refs/heads/{}", env)),
        new: landed.clone(),
    });
    effects.push(AppliedEffect::MetadataChange {
        refname: plan.detail.state_refname().to_string(),
        description: format!(
            "record of what '{}' last built (result {})",
            env,
            short(&landed)
        ),
    });
    effects.push(AppliedEffect::MetadataChange {
        refname: "refs/heads/hitch-metadata".to_string(),
        description: format!("'rebuilt_at' stamp for '{}'", env),
    });

    // Empty, and that is the contract rather than an oversight: see
    // `ExecutionReceipt::warnings`. This used to copy every non-blocking plan
    // warning, which is to say every hold `compose_environment` decided *at plan
    // time* — so the receipt re-printed, verbatim, the sentence that says the
    // branch "will be held out of this build", in a document whose subject is
    // what already happened. The hold is a prediction, so it belongs to the
    // plan; the fact is that `dev` is now `partially realised`, which
    // `resulting_state` reads from the authority and the Result block renders.
    let mut warnings: Vec<ExecutionWarning> = Vec::new();

    // The owed push. A failed push is not a failed rebuild — the local branch
    // is published and the journal record survives so the next mutating
    // command's recovery reports it — but reporting the operation as fully
    // complete is a lie, because the remote is behind. This warning with
    // `owes_effect` set is the difference between the two.
    match &publish.push {
        PushOutcome::Pushed => {
            let remote_ref = format!("refs/remotes/origin/{}", env);
            let observed = git.rev_parse_opt(&remote_ref)?;
            effects.push(AppliedEffect::RemoteRefUpdate {
                refname: remote_ref,
                old: plan.detail.remote_env_sha_before.clone(),
                new: observed.unwrap_or_else(|| landed.clone()),
            });
        }
        PushOutcome::Failed { error } => {
            warnings.push(ExecutionWarning {
                message: format!(
                    "'{}' was rebuilt and published locally, but pushing it to origin failed:\n  \
                     {}\n  The local branch is up to date; origin/{} is not. To push it:\n  \
                     hitch push {} -f",
                    env, error, env, env
                ),
                owes_effect: true,
            });
        }
        PushOutcome::Declined => {
            warnings.push(ExecutionWarning {
                message: format!(
                    "'{}' was rebuilt and published locally. Pushing to origin was declined, so \
                     origin/{} is behind. To push it later:\n  hitch push {} -f",
                    env, env, env
                ),
                owes_effect: true,
            });
        }
        PushOutcome::NotAttempted => {}
    }

    // One snapshot, not two: the plan never builds one, and re-deriving state
    // from the plan's own predictions would defeat the point of having an
    // authority for it.
    let resulting_state = build_state_snapshot(context).ok();

    let outcome = if plan.detail.held.is_empty() {
        OperationOutcome::Applied
    } else {
        OperationOutcome::AppliedWithHolds
    };

    Ok(ExecutionReceipt {
        plan_id: plan.id.clone(),
        operation: plan.kind,
        started_at,
        completed_at: chrono::Utc::now(),
        outcome,
        effects,
        warnings,
        resulting_state,
    })
}

use crate::core::state::ChangedInput;
