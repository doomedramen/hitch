//! The release planner and executor.
//!
//! A release merges one environment's promoted branches into a target branch
//! that is usually *not* an environment branch — `main`, `production` — and
//! then tidies up after itself: prunes the branches it just integrated out of
//! the declarations that carried them, and rebuilds the environments whose
//! bases just moved. So it is the one operation whose plan describes a merge
//! about to be pushed at something shared.
//!
//! Two properties are load-bearing here and both are easy to lose in the move.
//!
//! **A release is all-or-nothing, and that is a property of where the conflict
//! check sits.** The composition runs in the planner, and a conflict returns
//! `Err` from there — before the anchor is written, before the tag exists,
//! before the target ref is touched. There is no partial release to unwind,
//! because nothing was written. The same holds for the planner failing for any
//! other reason. Do not move the composition into the executor "so the plan can
//! be shown first": the moment the merges move, "all-or-nothing" becomes a
//! claim rather than a fact.
//!
//! **The prune predicate is evaluated against the commit the release is about
//! to publish, not against the live target ref.** A prune removes a promoted
//! branch from a declaration because the branch is now contained in that
//! environment's base. The released branches are contained in the *composed*
//! commit and — by construction — not in the pre-release tip, which is what
//! `refs/heads/<target>` still reads until the publish lands. Evaluating the
//! predicate against the live ref would therefore prune *nothing*, silently, on
//! every release whose whole purpose was to integrate those branches. The
//! planner pins `result_sha` and uses it as the base for any environment whose
//! base is the release target; the executor then applies exactly the prune list
//! the plan names, rather than re-running the predicate — which would be a
//! second decision point, against a ref that has by then moved.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::commands::global_context::GlobalContext;
use crate::core::render::render_dependent_skip;
use crate::core::state::build_state_snapshot;
use crate::operations::model::{
    changed_inputs, AppliedEffect, CompositionPlan, ConfirmationRequirement,
    DependentRebuildOutcome, EnvironmentProjection, ExecutionReceipt, ExecutionWarning, HoldPair,
    OperationIntent, OperationKind, OperationOutcome, OperationPlan, PlanApplyError,
    PlanFingerprint, PlanWarning, PlannedBranch, PlannedBranchState, PlannedEffect, ResourceKind,
    UnaffectedResource,
};
use crate::operations::rebuild::PlanPurpose;
use crate::types::{Environment, HitchConfig};
use crate::utils::build_record::PinnedBranch;
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::{
    access_metadata_read_only, modify_metadata, predict_composition, publish_branch,
    push_branch_with_deploy_key_if_configured, rebuild_environment, with_locked_env,
    CompatibilityConflict, PublishOutcome, PushOutcome,
};

/// The per-operation options a caller chose. Not a clap type, for the same
/// reason as [`crate::operations::rebuild::RebuildPlanOptions`].
#[derive(serde::Serialize, Debug, Clone, Copy, Default)]
pub struct ReleasePlanOptions {
    /// `hitch release --squash`. Part of the plan, not a display flag: it
    /// changes the parent count of the commit that is about to exist, so a plan
    /// that ignored it would be describing a different commit.
    pub squash: bool,
    /// `hitch release --no-prune`. Leaves integrated branches in their
    /// declarations, which is why the plan says so rather than omitting the
    /// consequence.
    pub no_prune: bool,
    /// `hitch release --no-rebuild-dependents`.
    pub no_rebuild_dependents: bool,
}

/// A promoted branch this release will remove from an environment's
/// declaration.
///
/// `branches` is the *decision*, not the candidate list: the "is it contained
/// in that environment's base yet" predicate is evaluated by the planner (see
/// the module header on `result_sha`) so that the executor applies a list
/// rather than re-deriving one.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrune {
    pub environment: String,
    pub branches: Vec<String>,
}

/// An environment this release will rebuild because its base moved or its
/// declaration was pruned.
///
/// The `dependents` list is the set the planner could *prove* it would
/// attempt. Two kinds of skip live on the other side of that line, and which
/// side a skip falls on is the whole design:
///
/// - **Decided here, so the environment is simply absent.** Locked by another
///   operation, or a preflight the planner already knows will conflict. Both
///   are reads available at plan time, and an environment the plan declares a
///   rebuild for while also declaring it cannot happen is a plan that
///   contradicts itself. Each exclusion is an [`PlanWarning::advisory`] instead,
///   which is a strictly better answer than the `log_warning` line the old code
///   printed mid-mutation.
/// - **Decided at apply time, so the environment is present and the receipt
///   records `Skipped`.** "Its base environment failed to rebuild" is not a
///   plan-time read at all: it depends on the outcome of an operation that has
///   not run yet. [`base_environment`] carries the fact the executor needs to
///   decide it, without the plan having to predict the answer.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct DependentRebuild {
    pub environment: String,
    /// The plan-time reason this environment is in the set, in one clause. The
    /// executor uses it as the step label, so the user is told *why* their
    /// environment is being rebuilt rather than just that it is.
    pub because: String,
    /// Another environment this one is built on, when that base is itself part
    /// of the closure — including when the planner excluded it. `None` means
    /// this environment's base is a plain branch, not an environment, so there
    /// is nothing to wait for.
    ///
    /// Kept because "was its base rebuilt?" is a question about the *closure*,
    /// not about the attempted set: if a base is in the closure and was skipped
    /// or failed, everything standing on it is built from a stale base and must
    /// be skipped too. Narrowing the check to the attempted list would quietly
    /// start rebuilding those environments instead — a behaviour change nobody
    /// would be looking for.
    pub base_environment: Option<String>,
}

/// The release-specific payload a plan carries.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlanDetail {
    pub environment: String,
    pub target: String,
    /// Each promoted branch, pinned to the SHA the composition consumed, in
    /// **declaration order**. A release folds branches in one at a time and the
    /// order decides the merge order, so this list is never sorted.
    pub released: Vec<PlannedBranch>,
    pub squash: bool,
    /// The composed target tip. Already in the object database — the module
    /// header explains why that is not an implementation detail.
    pub result_sha: String,
    /// The ref anchoring the composed commit between planning and publishing,
    /// or `None` for a preview. See [`discard_release_plan`].
    pub anchor_ref: Option<String>,
    /// The `refs/heads/<target>` value the publish's CAS will require.
    pub target_sha_before: String,
    /// The remote target tip observed *before* composing, so the eventual push
    /// is reported against what was observed rather than against whatever the
    /// remote has become by the time the release finishes.
    pub remote_target_sha_before: Option<String>,
    /// The *base* tag name. The executor may land a disambiguated variant of it
    /// — `create_release_tag` handles a same-name collision from a
    /// crash-then-immediate-retry — and the receipt records the name that
    /// actually exists. The plan says what it intends; the receipt says what is
    /// there.
    pub tag_name: String,
    pub tag_message: String,
    pub prunes: Vec<ReleasePrune>,
    pub dependents: Vec<DependentRebuild>,
}

/// Build the plan for releasing `environment` into `target`.
///
/// `target` is resolved by the command rather than here, because
/// `commands/release.rs`'s confirmation prompt needs it *before* anything is
/// planned — and a prompt describing a target the plan later disagreed with
/// would be the one place a user is asked to approve something other than what
/// happens.
pub fn plan_release(
    context: &GlobalContext,
    environment: &str,
    target: &str,
    options: ReleasePlanOptions,
    purpose: PlanPurpose,
) -> Result<OperationPlan<ReleasePlanDetail>> {
    let config = access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let declared = config.environments.get(environment).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            environment,
            config.get_environment_names().join(", ")
        )
    })?;

    // A release of nothing is not a release. `commands/release.rs` keeps its own
    // early `return Ok(())` for this case — that is what preserves the exit-0
    // behaviour — and this refusal guarantees a degenerate plan (an empty
    // release that still proposes a tag on the target's current tip) is
    // unreachable from any other caller.
    if declared.branches.is_empty() {
        anyhow::bail!(
            "No branches promoted to environment '{}', nothing to release",
            environment
        );
    }

    // The user-visible account of what is about to be composed.
    context.log_info(&format!(
        "Releasing {} promoted branches from environment '{}' to '{}'",
        declared.branches.len(),
        environment,
        target
    ));

    // Synchronise, then pin. Everything below composes against these concrete
    // SHAs rather than the mutable branch names, so a ref moving mid-release
    // cannot change what gets merged.
    context.log_verbose("Synchronizing branches for release...");
    let mut sync = declared.branches.clone();
    sync.push(target.to_string());
    if purpose.synchronizes() {
        context.git().synchronize_branches(&sync)?;
    }

    let git = context.git();
    let mut released: Vec<PlannedBranch> = Vec::new();
    for branch in &declared.branches {
        if !git.branch_exists(branch)? {
            return Err(anyhow::anyhow!(
                "Branch '{}' does not exist locally after synchronization",
                branch
            ));
        }
        released.push(PlannedBranch {
            branch: branch.clone(),
            sha: git.get_branch_commit_sha(branch)?,
            // Overwritten by `compose_release`; declared here so the vec is
            // fully populated before the composition rather than after.
            state: PlannedBranchState::Missing,
        });
    }

    // The target tip is read *before* composing: it is the CAS old value, and
    // it is the point the composition starts from. Read as an exact ref rather
    // than through `get_branch_commit_sha`, whose remote fallback would hand a
    // CAS a SHA belonging to a different ref and wedge the publish. The `None`
    // arm is unreachable after `synchronize_branches` — a branch that exists
    // anywhere gets a local ref — but the error is a real one if that ever
    // changes, and it has to be.
    let target_ref = format!("refs/heads/{}", target);
    let target_sha_before = git.rev_parse_opt(&target_ref)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Branch '{}' does not exist locally after synchronization",
            target
        )
    })?;
    let remote_target_ref = format!("refs/remotes/origin/{}", target);
    let remote_target_sha_before = git.rev_parse_opt(&remote_target_ref)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let now = chrono::Utc::now();

    // The composition. See the module header: a conflict returns `Err` before
    // anything has been written, which is what makes a release all-or-nothing
    // rather than merely careful.
    let result_sha = compose_release(
        context,
        environment,
        target,
        &target_sha_before,
        &mut released,
        options.squash,
    )?;

    // Anchor the composed tip for the window between planning and publishing, so
    // a concurrent `git gc --prune=now` cannot collect it. This is the same
    // anchor the old inline sequence created between compose and publish, moved
    // earlier in time — not a new mechanism, and deliberately still in the
    // `release/` family rather than `build/`: a rebuild's anchor is one
    // composed commit, a release's is a published-and-anchored tip, and
    // renaming the family would orphan every anchor a half-finished release
    // left behind.
    let anchor_ref = if purpose.anchors() {
        let refname = format!("refs/hitch/release/{}/{}", target, timestamp);
        context
            .git()
            .update_ref(&refname, &result_sha)
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

    let tag_name = format!(
        "hitch-release-{}-to-{}-{}",
        environment,
        target.replace('/', "-"),
        now.format("%Y-%m-%dT%H-%M-%SZ")
    );
    let tag_message = format!(
        "Hitch release of environment '{}' to '{}' at {}",
        environment,
        target,
        now.format("%Y-%m-%d %H:%M:%S UTC")
    );

    // The prunes and the dependent closure, both decided here. Environments are
    // visited in **name order**, not `HashMap` order, because
    // `HitchConfig::environments` is a `HashMap` and an unordered effect list
    // would make two plans over identical inputs render differently.
    let (prunes, mut warnings) = plan_prunes(
        context,
        &config,
        environment,
        target,
        &released,
        &result_sha,
        options.no_prune,
    );
    let (dependents, dependent_warnings) = plan_dependents(
        context,
        &config,
        environment,
        target,
        &prunes,
        options.no_rebuild_dependents,
    )?;
    warnings.extend(dependent_warnings);

    let released_pins: Vec<PinnedBranch> = released
        .iter()
        .map(|p| PinnedBranch {
            branch: p.branch.clone(),
            sha: p.sha.clone(),
        })
        .collect();

    let detail = ReleasePlanDetail {
        environment: environment.to_string(),
        target: target.to_string(),
        released: released.clone(),
        squash: options.squash,
        result_sha: result_sha.clone(),
        anchor_ref: anchor_ref.clone(),
        target_sha_before: target_sha_before.clone(),
        remote_target_sha_before: remote_target_sha_before.clone(),
        tag_name: tag_name.clone(),
        tag_message,
        prunes: prunes.clone(),
        dependents: dependents.clone(),
    };

    let composition = CompositionPlan {
        environment: environment.to_string(),
        base: PinnedBranch {
            branch: target.to_string(),
            sha: target_sha_before.clone(),
        },
        branches: released.clone(),
        result_sha: result_sha.clone(),
        // Unreachable: a conflict returns `Err` from `compose_release`, so no
        // plan ever exists that had to hold a branch. Modelled as empty rather
        // than omitted so the field cannot be read as "holds were not checked".
        holds: Vec::new(),
    };

    // A release's `current`/`proposed` describe the **target**, not the released
    // environment. That is the one place this projection is used against a ref
    // other than the environment's own branch, and it is deliberate: the ref a
    // release moves is `refs/heads/<target>`, and describing the environment
    // branch instead would hide the only local ref update the operation
    // performs. The environment branch does move too — but through the
    // *dependent* rebuild below, whose result no plan can predict, so it is
    // declared as an effect rather than predicted as a value.
    // Two projections, and they differ. `proposed` is the target as the release
    // will leave it — base plus the branches this plan merges. `current` is the
    // target as it is *now*, which contains none of them: the branches are being
    // merged, so they are not in it yet. Both arms once took the same branch
    // list, and the plan then rendered
    //
    //     Current    main = main + feature-1
    //     Proposed   main = main + feature-1
    //
    // — a plan stating, as fact, that the release it is about to perform has
    // already happened. `EnvironmentProjection::branches` is documented as
    // "the declaration, in declaration order", which is exactly right for a
    // promote (the declaration is the thing being edited) and exactly wrong for
    // a release (the target's history is not a declaration at all). The
    // distinction only became visible once a renderer printed the two side by
    // side; before P6 both were computed and neither was read.
    let projection =
        |branch_sha: Option<String>, branches: Vec<PinnedBranch>| EnvironmentProjection {
            environment: target.to_string(),
            base: target.to_string(),
            branches,
            branch_sha,
        };

    let mut effects = vec![
        PlannedEffect::LocalRefUpdate {
            refname: target_ref.clone(),
            old: Some(target_sha_before.clone()),
            new: result_sha.clone(),
        },
        PlannedEffect::TagCreation {
            name: tag_name,
            target_sha: result_sha.clone(),
        },
        PlannedEffect::MetadataChange {
            refname: "refs/heads/hitch-metadata".to_string(),
            description: format!("'released_at' stamp for '{}'", environment),
        },
    ];
    if let Some(anchor) = &anchor_ref {
        effects.push(PlannedEffect::MetadataChange {
            refname: anchor.clone(),
            description:
                "temporary anchor holding the composed commit until it is published, then removed"
                    .to_string(),
        });
    }
    // A remote effect is predicted *only* when the command will actually attempt
    // a push. A plan that predicts a push the apply will not make is exactly
    // the false "fully synced" a receipt must never produce.
    if context.should_push() {
        effects.push(PlannedEffect::RemoteRefUpdate {
            refname: remote_target_ref.clone(),
            old: remote_target_sha_before.clone(),
            new: result_sha.clone(),
        });
    }
    for prune in &prunes {
        effects.push(PlannedEffect::PromotionPrune {
            environment: prune.environment.clone(),
            branches: prune.branches.clone(),
            refname: "refs/heads/hitch-metadata".to_string(),
        });
    }
    for dependent in &dependents {
        effects.push(PlannedEffect::DependentEnvironmentRebuild {
            environment: dependent.environment.clone(),
            because: dependent.because.clone(),
            refname: format!("refs/heads/{}", dependent.environment),
        });
    }

    let released_names: Vec<&str> = released.iter().map(|p| p.branch.as_str()).collect();
    let mut unaffected: Vec<UnaffectedResource> = Vec::new();
    for (name, env) in sorted_environments(&config) {
        if name != environment {
            unaffected.push(UnaffectedResource {
                kind: ResourceKind::Environment,
                name,
            });
        }
        for branch in &env.branches {
            if !released_names.contains(&branch.as_str()) {
                unaffected.push(UnaffectedResource {
                    kind: ResourceKind::Branch,
                    name: branch.clone(),
                });
            }
        }
    }

    // The fingerprint. Note what is *absent*: the pruned environments' branch
    // tips. This plan asks whether a branch is merged into a base, but it asks
    // through the composed commit, and the dependent rebuild that follows
    // fingerprints its own inputs. Tracking them here as well would refuse the
    // apply when a branch this plan does not compose moved — the wrong question,
    // asked twice.
    let mut fingerprint = PlanFingerprint::new();
    fingerprint.metadata_sha = git.rev_parse_opt("refs/heads/hitch-metadata")?;
    for pinned in &released {
        fingerprint.track_ref(format!("refs/heads/{}", pinned.branch), &pinned.sha);
    }
    fingerprint.track_ref(&target_ref, &target_sha_before);
    fingerprint.track_remote_ref(&remote_target_ref, remote_target_sha_before);

    let digest = fingerprint.digest(git)?;

    Ok(OperationPlan {
        id: format!("release:{}:{}:{}", environment, target, digest),
        kind: OperationKind::Release,
        intent: OperationIntent::ReleaseEnvironment {
            environment: environment.to_string(),
            target: target.to_string(),
        },
        fingerprint,
        current: Some(projection(Some(target_sha_before), Vec::new())),
        proposed: Some(projection(Some(result_sha), released_pins.clone())),
        compositions: vec![composition],
        effects,
        unaffected,
        warnings,
        // A release merges into a shared branch and is not always a
        // fast-forward the reader should assume; the command's own prompt is
        // untouched (P6 owns that UX) but the plan states the same fact in
        // structured form, which is what lets a renderer ask instead of assume.
        confirmation: ConfirmationRequirement::required(format!(
            "merge {} promoted branch(es) from '{}' into '{}'",
            released.len(),
            environment,
            target
        )),
        detail,
    })
}

/// Fold each promoted branch into the running composition, in declaration
/// order, and return the resulting target tip.
///
/// `out` is taken by `&mut` because it is where each branch's
/// [`PlannedBranchState`] is recorded; the tip comes back as a return value
/// because it is a different kind of fact from the per-branch states and
/// conflating them in one struct is what made the alternative worse.
///
/// Split out of [`plan_release`] so the "nothing has been written yet" property
/// is visible in one place: a conflict returns `Err` from this function, and
/// every call site is still before the anchor and the tag.
fn compose_release(
    context: &GlobalContext,
    environment: &str,
    target: &str,
    target_sha: &str,
    out: &mut [PlannedBranch],
    squash: bool,
) -> Result<String> {
    let git = context.git();
    let mut composed = target_sha.to_string();

    for planned in out.iter_mut() {
        let branch = planned.branch.as_str();
        let sha = planned.sha.as_str();
        context.log_verbose(&format!("Merging '{}' into '{}'...", branch, target));
        let merge_message = format!(
            "Hitch: release {} from {} to {}",
            branch, environment, target
        );

        let outcome = git.merge_tree_compose(&composed, sha)?;
        if !outcome.conflicted_stages.is_empty() {
            let conflicted_files = outcome
                .conflicted_stages
                .iter()
                .map(|(path, _, _, _)| path.clone())
                .collect();
            return Err(build_conflict_error(
                branch,
                Some(conflicted_files),
                target,
                environment,
            ));
        }

        // Squash keeps the released branch's commits out of the target's
        // history; the default merge commit records the second parent so
        // ancestry is preserved and GitHub can detect merged PRs.
        let parents: Vec<&str> = if squash {
            vec![composed.as_str()]
        } else {
            vec![composed.as_str(), sha]
        };

        let composed_tree = git.rev_parse(&format!("{}^{{tree}}", composed))?;
        if squash && outcome.tree_oid == composed_tree {
            // Nothing to record — the branch is already contained in the
            // target. A merge commit is still worth making in `--no-ff` mode
            // because it is what carries the ancestry link. This is
            // `AlreadyInBase` rather than `Included` because no commit was made
            // for it, and a plan claiming N branches landed when N−1 commits
            // exist is a plan that cannot be reconciled with the repository.
            context.log_verbose(&format!(
                "'{}' is already contained in '{}'",
                branch, target
            ));
            planned.state = PlannedBranchState::AlreadyInBase;
            continue;
        }

        composed = git.commit_tree(&outcome.tree_oid, &parents, &merge_message)?;
        planned.state = PlannedBranchState::Included;
        context.log_verbose(&format!("✓ Merged '{}' into '{}'", branch, target));
    }

    debug_assert!(
        out.iter().all(|p| p.state != PlannedBranchState::Missing),
        "every released branch must be accounted for by the composition"
    );

    Ok(composed)
}

/// Environments in **name order**. `HitchConfig::environments` is a `HashMap`,
/// so anything that walks it has to impose an order itself or two runs over
/// identical state produce different renderings.
fn sorted_environments(config: &HitchConfig) -> Vec<(String, Environment)> {
    let mut out: Vec<(String, Environment)> = config
        .environments
        .iter()
        .map(|(name, env)| (name.clone(), env.clone()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Which promoted branches this release removes from which declarations, and
/// an advisory for every candidate it declines to remove.
///
/// Every "no" here is a fact the user would otherwise only learn from a
/// `log_warning` emitted *after* something had already been written — which is
/// exactly the gap a plan exists to close.
fn plan_prunes(
    context: &GlobalContext,
    config: &HitchConfig,
    released_env: &str,
    target: &str,
    released: &[PlannedBranch],
    result_sha: &str,
    no_prune: bool,
) -> (Vec<ReleasePrune>, Vec<PlanWarning>) {
    let released_names: Vec<&str> = released.iter().map(|p| p.branch.as_str()).collect();
    let mut prunes: Vec<ReleasePrune> = Vec::new();
    let mut warnings: Vec<PlanWarning> = Vec::new();

    if no_prune {
        warnings.push(PlanWarning::advisory(
            "Promoted branches will stay in their declarations even though this release \
             integrates them (--no-prune)",
        ));
        return (prunes, warnings);
    }

    for (name, env) in sorted_environments(config) {
        if name != released_env && env.is_locked() {
            // Pruning a locked environment's metadata would edit a repository
            // another operation owns, and its rebuild is skipped too, so the
            // declaration and the built branch would disagree — and it violates
            // the lock. The environment being released is locked by *this*
            // release, so its own lock is not a reason to skip it; declining
            // there would leave it permanently carrying the branches it just
            // merged.
            for branch in env
                .branches
                .iter()
                .filter(|b| released_names.contains(&b.as_str()))
            {
                warnings.push(PlanWarning::advisory(format!(
                    "'{}' will not be pruned from '{}' because that environment is locked by \
                     another operation",
                    branch, name
                )));
            }
            continue;
        }

        let base_exists = match context.git().branch_exists_anywhere(&env.base) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(PlanWarning::advisory(format!(
                    "'{}' will not be pruned of any released branch because its base '{}' could \
                     not be checked: {}",
                    name, env.base, e
                )));
                false
            }
        };

        let mut removed: Vec<String> = Vec::new();
        for branch in &env.branches {
            if !released_names.contains(&branch.as_str()) {
                continue;
            }
            if !base_exists {
                warnings.push(PlanWarning::advisory(format!(
                    "'{}' will not be pruned from '{}' because base '{}' does not exist",
                    branch, name, env.base
                )));
                continue;
            }
            if !context
                .git()
                .branch_exists_anywhere(branch)
                .unwrap_or(false)
            {
                warnings.push(PlanWarning::advisory(format!(
                    "'{}' will not be pruned from '{}' because the branch no longer exists",
                    branch, name
                )));
                continue;
            }

            // The `result_sha` case is the whole point of this function. An
            // environment whose base *is* the release target can only see the
            // newly integrated branches by looking at the commit this release is
            // about to publish — the live ref still reads the pre-release tip,
            // where they are by definition absent.
            let base = if env.base == target {
                result_sha
            } else {
                &env.base
            };
            if context
                .git()
                .is_branch_merged_into(branch, base)
                .unwrap_or(false)
            {
                removed.push(branch.clone());
            } else {
                warnings.push(PlanWarning::advisory(format!(
                    "'{}' is promoted to '{}' but is not contained in its base '{}' yet, so it \
                     will not be pruned",
                    branch, name, env.base
                )));
            }
        }

        if !removed.is_empty() {
            prunes.push(ReleasePrune {
                environment: name,
                branches: removed,
            });
        }
    }

    (prunes, warnings)
}

/// Which environments this release will rebuild, in an order that rebuilds a
/// base before anything standing on it, plus an advisory for each environment
/// the planner can already tell will be skipped.
///
/// The closure is "base is the release target" ∪ "was pruned" ∪ "bases one of
/// those". The order is [`topological_environment_order`], because an
/// environment built on a base that has not been rebuilt yet would be built
/// from the old base — the exact staleness the closure exists to remove.
fn plan_dependents(
    context: &GlobalContext,
    config: &HitchConfig,
    released_env: &str,
    target: &str,
    prunes: &[ReleasePrune],
    no_rebuild_dependents: bool,
) -> Result<(Vec<DependentRebuild>, Vec<PlanWarning>)> {
    let mut warnings: Vec<PlanWarning> = Vec::new();
    if no_rebuild_dependents {
        warnings.push(PlanWarning::advisory(
            "Environments built on the released target will be left stale \
             (--no-rebuild-dependents)",
        ));
        return Ok((Vec::new(), warnings));
    }
    if config.environments.is_empty() {
        return Ok((Vec::new(), warnings));
    }

    let pruned: HashSet<&str> = prunes.iter().map(|p| p.environment.as_str()).collect();
    let mut candidates: HashSet<String> = HashSet::new();
    for (name, env) in &config.environments {
        if env.base == target || pruned.contains(name.as_str()) {
            candidates.insert(name.clone());
        }
    }
    if candidates.is_empty() {
        return Ok((Vec::new(), warnings));
    }
    // Transitive dependents, where the base is another environment's name.
    loop {
        let before = candidates.len();
        for (name, env) in &config.environments {
            if candidates.contains(&env.base) {
                candidates.insert(name.clone());
            }
        }
        if candidates.len() == before {
            break;
        }
    }

    let mut dependents: Vec<DependentRebuild> = Vec::new();
    for name in topological_environment_order(config) {
        if !candidates.contains(&name) {
            continue;
        }
        let env = match config.environments.get(&name) {
            Some(e) => e,
            None => continue,
        };

        // The released environment is (expected to be) locked by *this* release,
        // so its lock is not a reason to skip it — otherwise its metadata gets
        // pruned and its branch is never rebuilt, leaving it stale against the
        // very base this release just moved.
        if name != released_env && env.is_locked() {
            warnings.push(PlanWarning::advisory(format!(
                "'{}' will not be rebuilt because it is locked",
                name
            )));
            continue;
        }

        // Preflight against the branch list the rebuild will actually have, i.e.
        // post-prune. The difference is only ever branches this release is
        // folding into that environment's base, which are ancestors of it by
        // construction, so the two lists cannot disagree about conflicts — but
        // the preflight should be about the operation, not about a superseded
        // version of it.
        let effective: Vec<String> = env
            .branches
            .iter()
            .filter(|b| {
                !prunes
                    .iter()
                    .find(|p| p.environment == name)
                    .map(|p| p.branches.contains(*b))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        // A prediction, and the executor still records the outcome it observes.
        // Run here so a plan does not declare a rebuild it already knows cannot
        // happen. It is the same composition the rebuild runs, over the branch
        // list the rebuild will have, so a held branch here is a held branch
        // there.
        let mut proposed = env.clone();
        proposed.branches = effective;
        if let Some(conflict) = predict_composition(context, &proposed, &name)?.held.first() {
            warnings.push(PlanWarning::advisory(render_dependent_skip(
                &name, conflict,
            )));
            continue;
        }

        let base_environment = if candidates.contains(&env.base) {
            Some(env.base.clone())
        } else {
            None
        };
        let because = if pruned.contains(name.as_str()) {
            "its declaration was pruned".to_string()
        } else if env.base == target {
            format!("its base is the released '{}'", target)
        } else if let Some(base) = &base_environment {
            format!("it is built on '{}', which was rebuilt", base)
        } else {
            // Unreachable given the candidate rule above (a candidate is direct
            // or transitive, and both cases are named). Written to name the real
            // base rather than guess a reason, because a step label that
            // misreports *why* is worse than one that admits it is generic.
            format!("it is built on '{}'", env.base)
        };
        dependents.push(DependentRebuild {
            environment: name,
            because,
            base_environment,
        });
    }

    Ok((dependents, warnings))
}

/// Base environments first, then whatever stands on them. Moved here from
/// `commands/release.rs` unchanged: the ordering rule is release's dependency
/// logic, not the command's presentation.
fn topological_environment_order(config: &HitchConfig) -> Vec<String> {
    // Edge: base_env -> env_name when env.base matches another environment name.
    let env_names: HashSet<String> = config.environments.keys().cloned().collect();
    let mut indegree: HashMap<String, usize> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();

    for name in &env_names {
        indegree.insert(name.clone(), 0);
    }

    for (env_name, env) in &config.environments {
        if env_names.contains(&env.base) {
            *indegree.entry(env_name.clone()).or_insert(0) += 1;
            children
                .entry(env.base.clone())
                .or_default()
                .push(env_name.clone());
        }
    }

    // Kahn's algorithm with a min-heap keyed by name, so ready nodes are emitted
    // in alphabetical order without the previous O(n²) `remove(0)` + re-`sort()`.
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    let mut queue: BinaryHeap<Reverse<String>> = indegree
        .iter()
        .filter_map(|(k, &d)| {
            if d == 0 {
                Some(Reverse(k.clone()))
            } else {
                None
            }
        })
        .collect();

    let mut out: Vec<String> = Vec::with_capacity(env_names.len());
    while let Some(Reverse(node)) = queue.pop() {
        if let Some(kids) = children.get(&node) {
            for kid in kids {
                if let Some(d) = indegree.get_mut(kid) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push(Reverse(kid.clone()));
                    }
                }
            }
        }
        out.push(node);
    }

    // If there was a cycle (should be rare), fall back to a stable alphabetical order.
    if out.len() != env_names.len() {
        let mut stable: Vec<String> = env_names.into_iter().collect();
        stable.sort();
        return stable;
    }

    out
}

/// Build detailed conflict error message. Moved here from
/// `commands/release.rs` with the text unchanged: it is the renderer's job, and
/// the release planner is what has to render its own refusal.
fn build_conflict_error(
    branch: &str,
    conflicted_files: Option<Vec<String>>,
    target_branch: &str,
    env_name: &str,
) -> anyhow::Error {
    let mut error_msg = format!(
        "Merge conflict detected when releasing branch '{}' to '{}'",
        branch, target_branch
    );

    if let Some(files) = conflicted_files {
        if !files.is_empty() {
            error_msg.push_str("\n\nConflicting files:");
            for file in files {
                error_msg.push_str(&format!("\n  • {}", file));
            }
        }
    }

    error_msg.push_str("\n\nTo resolve this:");
    error_msg.push_str(&format!(
        "\n1. Check out target branch: git checkout {}",
        target_branch
    ));
    error_msg.push_str(&format!(
        "\n2. Manually merge '{}': git merge {}",
        branch, branch
    ));
    error_msg.push_str("\n3. Resolve conflicts and commit");
    error_msg.push_str("\n4. Try release again with the environment instead:");
    error_msg.push_str(&format!(
        "\n   hitch release {} {}",
        env_name, target_branch
    ));

    anyhow::anyhow!("{}", error_msg)
}

/// Create the release tag, tolerating a same-name collision from a
/// crash-then-immediate-retry of the same release.
///
/// `create_tag_at` is a hard `git tag -a` with no `-f`, and `tag_name` is
/// stamped at second granularity — two releases of the same env/target landing
/// in the same wall-clock second collide on the name. A collision where the
/// existing tag already points at `sha` is the retry case (harmless: the retry
/// recomposed the exact same commit) and is treated as already-done. A
/// collision pointing at anything else is a second, genuinely distinct release
/// landing in the same second; disambiguate with a short random suffix rather
/// than fail outright — silently dropping or overwriting a real release's tag
/// would be worse than a slightly uglier name. Any other tag-creation failure
/// (not a name collision at all) is propagated unchanged. Returns the tag name
/// actually created or reused, which is what the receipt records rather than the
/// name the plan predicted.
fn create_release_tag(
    git: &GitOperations,
    tag_name: &str,
    message: &str,
    sha: &str,
) -> Result<String> {
    match git.create_tag_at(tag_name, message, sha) {
        Ok(()) => Ok(tag_name.to_string()),
        Err(e) => match git.rev_parse_opt(&format!("{}^{{commit}}", tag_name))? {
            Some(existing_sha) if existing_sha == sha => Ok(tag_name.to_string()),
            Some(_) => {
                let disambiguated = format!("{}-{}", tag_name, short_random_suffix());
                git.create_tag_at(&disambiguated, message, sha)?;
                Ok(disambiguated)
            }
            // The tag doesn't exist even after the failure, so this wasn't a
            // name collision at all — some other real failure. Propagate it.
            None => Err(e),
        },
    }
}

/// 8 hex characters of a fresh UUIDv4 — enough entropy to make a
/// disambiguated tag name practically unique without a second lookup.
fn short_random_suffix() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

/// The plan's own cleanup: drop the anchor the planner created.
///
/// Called on **every** path where a plan will not be applied — a preview, a
/// plan the human declined, a plan that failed validation. A leaked anchor is
/// harmless (the commit just stays reachable under `refs/hitch/release/*`, which
/// nothing prunes — see `commands/cleanup.rs`'s prunable set, deliberately
/// `["backup", "prev"]`) but it grows without bound, and the same obligation
/// [`crate::operations::rebuild::discard_plan`] carries is a named function here
/// rather than a `Drop` impl that cannot report a failure.
pub fn discard_release_plan(context: &GlobalContext, plan: &OperationPlan<ReleasePlanDetail>) {
    if let Some(anchor) = &plan.detail.anchor_ref {
        let _ = context.git().delete_ref(anchor);
    }
}

/// Check that everything the plan depends on is still what it was. See
/// [`crate::operations::model::changed_inputs`] for the one rule this is.
pub fn validate_release_plan(
    context: &GlobalContext,
    plan: &OperationPlan<ReleasePlanDetail>,
) -> std::result::Result<(), PlanApplyError> {
    let changed = changed_inputs(&plan.fingerprint, context.git());
    if changed.is_empty() {
        return Ok(());
    }
    Err(PlanApplyError::stale_plan(
        plan.kind,
        &plan.detail.environment,
        &plan.detail.environment,
        &changed,
    ))
}

/// Land a plan that is still current, and report what happened.
///
/// The order below *is* the contract:
///
/// 1. validate — a plan that is not current is refused before anything moves.
/// 2. tag, publish, prune, rebuild dependents — in that order, because each one
///    assumes the previous landed.
/// 3. read the effects back and assemble the receipt.
///
/// Releasing the plan's anchor is deliberately **not** one of the numbered
/// steps: it is a `finally`, for the same reason as
/// [`crate::operations::rebuild::apply_rebuild_plan`]'s.
pub fn apply_release_plan(
    context: &GlobalContext,
    plan: &OperationPlan<ReleasePlanDetail>,
) -> Result<ExecutionReceipt> {
    let started_at = chrono::Utc::now();
    let outcome = apply_validated_plan(context, plan, started_at);
    discard_release_plan(context, plan);
    outcome
}

fn apply_validated_plan(
    context: &GlobalContext,
    plan: &OperationPlan<ReleasePlanDetail>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<ExecutionReceipt> {
    // Converted rather than `?`-ed so the typed error crosses the
    // `anyhow::Result` boundary intact; see `PlanApplyError::into_anyhow`.
    validate_release_plan(context, plan).map_err(PlanApplyError::into_anyhow)?;

    let env = plan.detail.environment.as_str();
    let target = plan.detail.target.as_str();

    // 1. The tag. Created before the publish because it is this release's
    //    rollback anchor — the reason `publish_branch` is called with
    //    `backup_timestamp: None` below — so it has to exist before the ref
    //    moves, not after.
    // Not narrated. The tag's *name* is the one thing here that the plan may
    // have got wrong — `create_release_tag` disambiguates a second-granularity
    // collision — so the effect line below is allowed to disagree with the
    // plan, and printing the same name twice in two vocabularies invited
    // reading the two as a conflict. A failure here returns `Err`, so the error
    // path never relied on this line for feedback either.
    let tag_name = create_release_tag(
        context.git(),
        &plan.detail.tag_name,
        &plan.detail.tag_message,
        &plan.detail.result_sha,
    )?;

    // 2. The publish. `extras: &[]` and `backup_timestamp: None` are both
    //    load-bearing and stay: a release has no truthful input for a build
    //    record (the list of what it integrated is the list a human assembled
    //    by promotion, not a composition hitch performed), and the tag above is
    //    the archival anchor a `prev`/`backup` pair would otherwise provide.
    //    No `-f` on the push either: release's push is a plain fast-forward, not
    //    a rewrite, and this branch is typically `main`/`production` — exactly
    //    what `hitch setup`'s branch-protection ruleset guards against direct
    //    force pushes. See `publish_branch`'s doc comment on `push_remedy`.
    let publish = publish_branch(
        context,
        target,
        &plan.detail.result_sha,
        &[],
        None,
        &format!("hitch release {}", env),
        &format!("hitch push {}", target),
        || push_branch_with_deploy_key_if_configured(context, target).map(|()| PushOutcome::Pushed),
    )?;

    // Empty, and that is the contract rather than an oversight: see
    // `ExecutionReceipt::warnings`. This used to copy every non-blocking plan
    // warning, which for a release is a family of them — `--no-prune`,
    // `--no-rebuild-dependents`, and the per-environment "will be left stale"
    // skips. All are consequences of flags the *user* passed, decided before
    // the apply started, and all were re-printed verbatim below a receipt that
    // had already published. The predictions are the plan's; the facts are
    // `effects` (a prune is a prune, or its absence) and the Result block's
    // `⧗ <env>   needs rebuild` for anything left stale.
    let mut warnings: Vec<ExecutionWarning> = Vec::new();

    // 3. The tag push, as its own best-effort step with its own gating.
    //
    //    A tag-push failure is reported here rather than through
    //    `publish_branch`'s generic push-failure warning, because that
    //    warning's remedy (`hitch push <branch> -f`) never pushes tags and its
    //    "recovery will report it again" claim is false: the journal record
    //    `recover()` reads describes the *branch*, whose tip already matches.
    //
    //    And `publish_branch` swallows a branch-push failure — it warns and
    //    returns rather than propagating, since a failed push does not undo an
    //    already-successful local publish — so reaching this point does NOT
    //    mean the branch push landed. Pushing the tag unconditionally would
    //    publish `refs/tags/<tag>` pointing at a commit `origin/<target>` does
    //    not contain. `push_branch_with_deploy_key_if_configured` calls
    //    `record_pushed_tip` on success, which updates
    //    `refs/remotes/origin/<target>` specifically, so that comparison is
    //    reliable.
    if context.should_push() {
        let remote_ref = format!("refs/remotes/origin/{}", target);
        let branch_pushed = context.git().rev_parse_opt(&remote_ref)?.as_deref()
            == Some(plan.detail.result_sha.as_str());
        if branch_pushed {
            if let Err(e) = context.git().push_tag(&tag_name) {
                context.log_warning(&format!(
                    "Release published and pushed, but pushing the release tag '{}' failed: {}",
                    tag_name, e
                ));
                context.log_warning(&format!(
                    "Push the tag manually with: git push origin {}",
                    tag_name
                ));
                warnings.push(ExecutionWarning {
                    message: format!(
                        "The release was published and pushed, but the release tag '{}' was not. \
                         Push it manually:\n  git push origin {}",
                        tag_name, tag_name
                    ),
                    owes_effect: true,
                });
            }
        } else {
            context.log_warning(&format!(
                "Skipping the release tag push for '{}' because the branch push did not land \
                 — push the branch first (see the earlier warning for how), then push the tag \
                 manually with: git push origin {}",
                tag_name, tag_name
            ));
            warnings.push(ExecutionWarning {
                message: format!(
                    "The release tag '{}' was not pushed, because the branch push to origin did \
                     not land. Push the branch, then the tag:\n  git push origin {}",
                    tag_name, tag_name
                ),
                owes_effect: true,
            });
        }
    }

    // 4. The `released_at` stamp and the plan's prunes, in one metadata write.
    //    The prune list is applied *verbatim*: re-running the predicate here
    //    would be a second decision point, and the base it would be evaluated
    //    against has by now moved — which is precisely the bug the module
    //    header warns about.
    context.log_verbose("Updating release metadata...");
    let prunes = plan.detail.prunes.clone();
    modify_metadata(context, |config| {
        let env = config
            .get_environment_mut(env)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env))?;
        env.update_released_timestamp();
        for prune in &prunes {
            let environment = config
                .get_environment_mut(&prune.environment)
                .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", prune.environment))?;
            for branch in &prune.branches {
                environment.remove_branch(branch);
            }
        }
        Ok(())
    })?;

    // 5. The dependent rebuilds, in the order the plan fixed. Silent: the
    //    receipt's `pruned … from …` and `rebuild …` effect lines say all of
    //    this, and saying it here too meant each one appeared twice.
    let rebuilt = rebuild_dependents(context, plan);

    assemble_receipt(
        context, plan, &tag_name, publish, rebuilt, warnings, started_at,
    )
}

/// What one dependent rebuild came to, with the branches it held.
///
/// The apply-time counterpart to the plan's [`DependentRebuild`], and named
/// apart from it so the two cannot be confused at a call site: one says what the
/// release *intends*, the other what it *achieved*.
///
/// A struct rather than a tuple because the pair is read at three sites and
/// `outcome.0` says nothing at any of them. The `held` half is here for the same
/// reason it is on the effect: the nested build's own receipt is thrown away,
/// so without this a hold in a post-release rebuild has nowhere to be reported.
#[derive(Debug, Clone)]
struct DependentRebuildAttempt {
    outcome: DependentRebuildOutcome,
    held: Vec<HoldPair>,
}

/// Rebuild each dependent the plan named, in the plan's order, recording what
/// actually became of each.
///
/// Best-effort by construction: the release merge and tag have already landed by
/// the time this runs, so a failure here is reported and owed rather than
/// propagated. Failing the release would send the user to re-run a release that
/// succeeded, and the re-run would collide on the target's already-advanced tip.
///
/// Silent by construction, for the same reason `apply_declaration_plan` is: the
/// receipt's effect list says `rebuild <env>` with the outcome and the holds,
/// and a failure additionally gets an `ExecutionWarning` under "Still owed".
/// Logging here as well meant every post-release rebuild announced itself three
/// times — a step line, then a `log_success`/`log_warning`, then the receipt.
fn rebuild_dependents(
    context: &GlobalContext,
    plan: &OperationPlan<ReleasePlanDetail>,
) -> HashMap<String, DependentRebuildAttempt> {
    let mut rebuilt: HashMap<String, DependentRebuildAttempt> = HashMap::new();
    if plan.detail.dependents.is_empty() {
        return rebuilt;
    }

    for dependent in &plan.detail.dependents {
        let name = dependent.environment.as_str();

        // An environment standing on another one in the closure is skipped if
        // that one did not land, because rebuilding now would compose from a
        // stale base. The check is against the *closure* rather than the
        // attempted set, so an excluded base (locked, preflight-conflicting)
        // skips its dependents exactly as the old loop's `rebuild_set` did.
        if let Some(base_env) = &dependent.base_environment {
            if rebuilt.get(base_env).map(|d| &d.outcome) != Some(&DependentRebuildOutcome::Rebuilt)
            {
                let reason = format!(
                    "its base environment '{}' was not rebuilt successfully",
                    base_env
                );
                rebuilt.insert(
                    name.to_string(),
                    DependentRebuildAttempt {
                        outcome: DependentRebuildOutcome::Skipped(reason),
                        held: Vec::new(),
                    },
                );
                continue;
            }
        }

        let (outcome, held) = rebuild_dependent(context, name, name == plan.detail.environment);
        rebuilt.insert(
            name.to_string(),
            DependentRebuildAttempt {
                outcome,
                held: held.iter().map(HoldPair::from).collect(),
            },
        );
    }
    rebuilt
}

/// Rebuild one dependent, reporting how it went.
///
/// The released environment is rebuilt *without* a nested lock: in the normal
/// path it is already locked by the outer `with_locked_env` this release runs
/// inside, and a nested `with_locked_env` that took and then released the lock
/// would unlock it out from under the release still in progress.
fn rebuild_dependent(
    context: &GlobalContext,
    environment: &str,
    is_released_env: bool,
) -> (DependentRebuildOutcome, Vec<CompatibilityConflict>) {
    let result = if is_released_env {
        rebuild_environment(context, environment)
    } else {
        with_locked_env(context, environment, || {
            rebuild_environment(context, environment)
        })
    };

    // A rebuild that landed with branches held still rebuilt, so the holds do
    // not change the outcome — they ride alongside it into the receipt. This arm
    // used to log them, which was the only place they appeared anywhere; the
    // receipt is now the one place, and it has the partner to go with them.
    match result {
        Ok(outcome) => (DependentRebuildOutcome::Rebuilt, outcome.held),
        Err(e) => (DependentRebuildOutcome::Failed(e.to_string()), Vec::new()),
    }
}

#[allow(clippy::too_many_arguments)] // one argument per fact the receipt assembles; a struct here would be a second model
fn assemble_receipt(
    context: &GlobalContext,
    plan: &OperationPlan<ReleasePlanDetail>,
    tag_name: &str,
    publish: PublishOutcome,
    rebuilt: HashMap<String, DependentRebuildAttempt>,
    mut warnings: Vec<ExecutionWarning>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<ExecutionReceipt> {
    let git = context.git();
    let env = plan.detail.environment.as_str();
    let target = plan.detail.target.as_str();
    let target_ref = format!("refs/heads/{}", target);

    // Effects are read back, never copied. The plan said what it expected; this
    // says what is there.
    let landed = git.rev_parse_opt(&target_ref)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Internal error: '{}' was published but refs/heads/{} does not resolve afterwards.",
            target,
            target
        )
    })?;
    // `landed` must be the commit the plan named. The CAS in `publish_branch`
    // makes a mismatch impossible, so this is a check on our own bookkeeping —
    // and it is worth having, because "applied the wrong thing" is the one
    // failure a receipt may never report as a success.
    if landed != plan.detail.result_sha {
        return Err(anyhow::anyhow!(
            "Internal error: '{}' was published as {}, but refs/heads/{} reads {} afterwards.",
            target,
            plan.detail.result_sha,
            target,
            landed
        ));
    }
    let mut effects = vec![
        AppliedEffect::LocalRefUpdate {
            refname: target_ref.clone(),
            old: Some(plan.detail.target_sha_before.clone()),
            new: landed.clone(),
        },
        AppliedEffect::TagCreation {
            // The name the executor actually got, which may be a disambiguated
            // variant of the one the plan predicted.
            name: tag_name.to_string(),
            target_sha: landed.clone(),
        },
        AppliedEffect::MetadataChange {
            refname: "refs/heads/hitch-metadata".to_string(),
            description: format!("'released_at' stamp for '{}'", env),
        },
    ];

    // The owed push. Release's remedy is a plain fast-forward, so it is
    // `hitch push <target>` and never a `-f`: the branch this publishes is
    // typically the one branch-protection rulesets refuse direct force pushes
    // to, and a remedy that suggested otherwise would be advice the remote will
    // reject.
    match &publish.push {
        PushOutcome::Pushed => {
            let remote_ref = format!("refs/remotes/origin/{}", target);
            let observed = git.rev_parse_opt(&remote_ref)?;
            effects.push(AppliedEffect::RemoteRefUpdate {
                refname: remote_ref,
                old: plan.detail.remote_target_sha_before.clone(),
                new: observed.unwrap_or_else(|| landed.clone()),
            });
        }
        PushOutcome::Failed { error } => {
            warnings.push(ExecutionWarning {
                message: format!(
                    "'{}' was released and published locally, but pushing it to origin failed:\n  \
                     {}\n  The local branch is up to date; origin/{} is not. To push it:\n  \
                     hitch push {}",
                    target, error, target, target
                ),
                owes_effect: true,
            });
        }
        PushOutcome::Declined => {
            warnings.push(ExecutionWarning {
                message: format!(
                    "'{}' was released and published locally. Pushing to origin was declined, so \
                     origin/{} is behind. To push it later:\n  hitch push {}",
                    target, target, target
                ),
                owes_effect: true,
            });
        }
        PushOutcome::NotAttempted => {}
    }

    // The prunes, as applied. Read back from `hitch-metadata` rather than copied
    // from the plan: a shorter list means something removed branches in between,
    // which is worth being able to see.
    let prunes_applied = access_metadata_read_only(context, |config| {
        let mut out: Vec<AppliedEffect> = Vec::new();
        for prune in &plan.detail.prunes {
            let remaining = config
                .environments
                .get(&prune.environment)
                .map(|e| e.branches.clone())
                .unwrap_or_default();
            let removed: Vec<String> = prune
                .branches
                .iter()
                .filter(|b| !remaining.contains(b))
                .cloned()
                .collect();
            out.push(AppliedEffect::PromotionPrune {
                environment: prune.environment.clone(),
                branches: removed,
                refname: "refs/heads/hitch-metadata".to_string(),
            });
        }
        Ok(out)
    })?;
    effects.extend(prunes_applied);

    // The dependent rebuilds, each by what actually became of it.
    for dependent in &plan.detail.dependents {
        let name = dependent.environment.as_str();
        // A missing entry is not reachable — every loop exit inserts one — but
        // the receipt has no "unknown" variant, so it is reported as the skip it
        // most resembles rather than fabricated as a success.
        let rebuild = rebuilt
            .get(name)
            .cloned()
            .unwrap_or_else(|| DependentRebuildAttempt {
                outcome: DependentRebuildOutcome::Skipped("not attempted".to_string()),
                held: Vec::new(),
            });
        let DependentRebuildAttempt { outcome, held } = rebuild;
        if outcome.owes_effect() {
            let DependentRebuildOutcome::Failed(error) = &outcome else {
                unreachable!("owes_effect() is only true for Failed")
            };
            warnings.push(ExecutionWarning {
                message: format!(
                    "'{}' was released to '{}', but rebuilding '{}' failed:\n  {}\n  The release \
                     itself succeeded. To rebuild it:\n  hitch rebuild {}",
                    env, target, name, error, name
                ),
                owes_effect: true,
            });
        }
        effects.push(AppliedEffect::DependentEnvironmentRebuild {
            environment: dependent.environment.clone(),
            outcome,
            held,
            refname: format!("refs/heads/{}", dependent.environment),
        });
    }

    Ok(ExecutionReceipt {
        plan_id: plan.id.clone(),
        operation: plan.kind,
        started_at,
        completed_at: chrono::Utc::now(),
        // Holds are not a release concept: a release composes all-or-nothing,
        // so a conflict never reaches an apply. The one non-`Applied` outcome
        // available here would be a lie in either direction.
        outcome: OperationOutcome::Applied,
        effects,
        warnings,
        resulting_state: build_state_snapshot(context).ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch git repo for tests exercising raw git plumbing directly —
    /// no hitch metadata needed. Mirrors the raw-git test helpers already
    /// established at `src/utils/prelude.rs`.
    #[allow(clippy::disallowed_methods)] // test-only scratch repo bootstrap, same rationale as prelude.rs's raw-git test helpers
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_scratch_repo() -> (tempfile::TempDir, GitOperations) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        // Named explicitly rather than inheriting `init.defaultBranch`, because
        // one of these tests reads "the live target" by name.
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "user.email", "test@test"]);
        git(repo, &["config", "user.name", "test"]);
        std::fs::write(repo.join("f.txt"), "one").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "one"]);
        let git_ops = GitOperations::new_at_path(&repo.to_string_lossy()).unwrap();
        (dir, git_ops)
    }

    /// A retried release (e.g. a crashed release re-run immediately, before
    /// the wall-clock second advances) regenerates the exact same composed
    /// commit and, with it, the exact same tag name. That must succeed and
    /// reuse the existing tag, not fail with "tag already exists" — this is
    /// the bug this function exists to fix.
    #[test]
    fn create_release_tag_reuses_existing_tag_for_same_content() {
        let (_dir, git_ops) = init_scratch_repo();
        let sha = git_ops.rev_parse("HEAD").unwrap();
        let tag_name = "hitch-release-dev-to-main-2026-08-01T00-00-00Z";

        let first = create_release_tag(&git_ops, tag_name, "msg", &sha).unwrap();
        assert_eq!(first, tag_name);

        let second = create_release_tag(&git_ops, tag_name, "msg", &sha).unwrap();
        assert_eq!(
            second, tag_name,
            "a same-content retry must reuse the existing tag name, not fail or rename"
        );
    }

    /// Two genuinely distinct releases (different composed commits) that
    /// happen to land in the same wall-clock second must both succeed, with
    /// distinct tags — neither silently dropped nor overwritten.
    #[test]
    fn create_release_tag_disambiguates_a_genuine_second_release_in_the_same_second() {
        let (dir, git_ops) = init_scratch_repo();
        let sha1 = git_ops.rev_parse("HEAD").unwrap();

        std::fs::write(dir.path().join("f.txt"), "two").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "two"]);
        let sha2 = git_ops.rev_parse("HEAD").unwrap();
        assert_ne!(sha1, sha2);

        let tag_name = "hitch-release-dev-to-main-2026-08-01T00-00-00Z";
        let first = create_release_tag(&git_ops, tag_name, "msg", &sha1).unwrap();
        assert_eq!(first, tag_name);

        let second = create_release_tag(&git_ops, tag_name, "msg", &sha2).unwrap();
        assert_ne!(
            second, tag_name,
            "a second, genuinely different release colliding on the same name must get a \
             distinct tag, not silently fail or overwrite the first"
        );
        assert!(
            second.starts_with(tag_name),
            "the disambiguated name should still be recognizable as this release's tag"
        );

        // Both tags must survive, pointing at their own respective commits.
        assert_eq!(
            git_ops
                .rev_parse(&format!("{}^{{commit}}", tag_name))
                .unwrap(),
            sha1,
            "the first release's tag must be untouched by the second release's collision"
        );
        assert_eq!(
            git_ops
                .rev_parse(&format!("{}^{{commit}}", second))
                .unwrap(),
            sha2
        );
    }

    /// The prune predicate's `result_sha` arm, tested as the module header
    /// describes it. A branch that is an ancestor of the *composed* commit and
    /// not of the live target is exactly the case every release creates, and it
    /// is the case that would silently prune nothing if the planner read the
    /// live ref instead.
    #[test]
    fn a_branch_merged_by_the_release_is_not_an_ancestor_of_the_live_target_yet() {
        let (dir, git_ops) = init_scratch_repo();
        let base = git_ops.rev_parse("HEAD").unwrap();

        git(dir.path(), &["checkout", "-q", "-b", "feat-a"]);
        std::fs::write(dir.path().join("f.txt"), "from the feature").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "feature"]);
        let feat = git_ops.rev_parse("HEAD").unwrap();
        git(dir.path(), &["checkout", "-q", "-"]);

        // What a release composes: a merge commit with the target as first
        // parent and the feature as second.
        let merged_tree = git_ops.merge_tree_compose(&base, &feat).unwrap().tree_oid;
        let composed = git_ops
            .commit_tree(&merged_tree, &[base.as_str(), feat.as_str()], "release")
            .unwrap();

        assert!(
            !git_ops.is_branch_merged_into("feat-a", "main").unwrap(),
            "the live target must not contain the branch yet — that is the whole \
             reason the predicate cannot read it"
        );
        assert!(
            git_ops.is_branch_merged_into("feat-a", &composed).unwrap(),
            "the composed commit is where the branch becomes integrated, and it is the \
             only place the planner can see that"
        );
    }
}
