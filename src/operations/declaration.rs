//! The promote / demote planner and executor.
//!
//! Promote and demote are **one** operation on the declaration — change an
//! environment's promoted-branch list, then rebuild whatever that invalidates
//! — with the direction as the only difference. They are not two operations on
//! the wire either: `hitch promote b dev` and `hitch demote b dev` are the same
//! declaration edit with opposite sign, and they share the approval gate, the
//! environment lock, the auto-stash, the rollback, the sibling-conflict check
//! and the dependent rebuild. Two planners would be two copies of that
//! machinery that have to agree forever, which is the failure class this
//! program exists to remove. So this module is one planner, and
//! [`plan_promote`]/[`plan_demote`] are named entry points over it — a reader
//! looking for "the promote planner" finds exactly one.
//!
//! What a promote does *not* do is compose anything itself. It edits a
//! declaration, and that edit invalidates a build; the rebuild is a nested
//! operation with its own plan and its own receipt, reached through
//! [`rebuild_environment`]. The plan therefore declares a
//! [`PlannedEffect::DependentEnvironmentRebuild`] rather than pretending to
//! predict the rebuild's result, and the nested plan is built *after* this
//! plan's edit lands — see [`apply_declaration_plan`].
//!
//! One consequence worth stating up front: this planner takes **no**
//! [`crate::operations::rebuild::PlanPurpose`]. There is nothing here to
//! synchronise (a declaration edit reads only `hitch-metadata`), nothing to
//! lock (the lock belongs to the command, and only on the path that mutates),
//! and no composed commit to anchor — so the enum has nothing to decide, and
//! taking it would be a parameter that always reads `Confirm`. If
//! `hitch promote --dry-run` ever exists, that is when the purpose is threaded
//! here *and* down into the nested rebuild; adding it earlier would be a
//! parameter promising a safety property nothing enforces.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::commands::global_context::GlobalContext;
use crate::core::state::build_state_snapshot;
use crate::operations::model::{
    changed_inputs, AppliedEffect, ConfirmationRequirement, DependentRebuildOutcome,
    EnvironmentProjection, ExecutionReceipt, ExecutionWarning, HoldPair, OperationIntent,
    OperationKind, OperationOutcome, OperationPlan, PlanApplyError, PlanFingerprint, PlanWarning,
    PlannedEffect, UnaffectedResource,
};
use crate::types::Operation;
use crate::utils::build_record::PinnedBranch;
use crate::utils::command_helpers::{ensure_environment_exists, validate_branch_for_promotion};
use crate::utils::prelude::{
    access_metadata_read_only, create_approval_requests_for_operation,
    display_approval_request_created, modify_metadata, pre_promote_conflict_reason,
    rebuild_environment, StepNarration,
};
use crate::utils::validation::validate_name;

/// Which way the declaration moves.
///
/// An enum rather than two `Vec`s because `Add(vec![])` and `Remove(vec![])`
/// are both meaningless, and because the direction decides the intent, the
/// remedy, the effect description, and which of `added`/`removed` in the
/// detail is the real one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationChange {
    /// Promote these branches into the environment.
    Add(Vec<String>),
    /// Demote these branches out of the environment. Branches that are not
    /// currently promoted are silently skipped, which is what the
    /// environment-name-expands-to-branches form of demote needs: expanding
    /// `dev` into `qa` can name branches `qa` never promoted, and erroring on
    /// them would make a partial demotion impossible.
    Remove(Vec<String>),
    /// Promote these branches, on the strength of an approval that has already
    /// been committed.
    ///
    /// A variant of *this* enum rather than a third planner, because it is the
    /// same edit to the same declaration in the same direction — a promote
    /// planner that took a flag saying "don't check the approval gate" would be
    /// a promote with a hole in it, and a separate planner would duplicate the
    /// three steps (edit, snapshot, rebuild) that have to stay in agreement.
    ///
    /// What differs is *identity*: the user ran `hitch approve <id>`, so the
    /// plan's kind, headline and remedy all have to name that command. That is
    /// why `kind()` below is a method on the change rather than a parameter at
    /// the call site — a parameter could disagree with the change it claims to
    /// describe, and nothing would notice.
    ApprovedApply {
        /// Which way the approved change moves the list.
        ///
        /// A field rather than two variants (`ApprovedPromote` /
        /// ApprovedDemote`) because every arm downstream only ever asks two
        /// things of this change — which way does it move, and is it already
        /// authorised — and a direction *field* is the one shape that answers
        /// both without the arms re-matching on the variant. The approval is
        /// equally real for a demotion: `hitch demote b production` files a
        /// request, `hitch approve <id>` applies it, and an enum that could only
        /// express the promotion half would leave the demotion half with no
        /// plan and therefore the old `apply_declaration_change` with a
        /// permanent reason to exist.
        operation: Operation,
        branches: Vec<String>,
        /// The approval request, carried so the plan can be correlated with the
        /// row in `hitch approvals` and the receipt can name it.
        request_id: String,
    },
}

impl DeclarationChange {
    pub fn branches(&self) -> &[String] {
        match self {
            DeclarationChange::Add(b)
            | DeclarationChange::Remove(b)
            | DeclarationChange::ApprovedApply { branches: b, .. } => b,
        }
    }

    pub fn kind(&self) -> OperationKind {
        match self {
            DeclarationChange::Add(_) => OperationKind::Promote,
            DeclarationChange::Remove(_) => OperationKind::Demote,
            DeclarationChange::ApprovedApply { .. } => OperationKind::ApprovalApply,
        }
    }

    /// Which way the promoted list moves.
    ///
    /// The three variants reduce to two facts this way, and reducing them is the
    /// point: the arithmetic in [`proposed_declaration`], the pre-checks and the
    /// sibling-conflict simulation all want *direction*, and each of them
    /// re-matching on the variant is three places that have to learn about
    /// `ApprovedApply` separately. What distinguishes an approved change is not
    /// its direction — it is that it is already authorised and names the request
    /// that authorised it — and that distinction is read from [`Self::kind`] and
    /// from the `ApprovedApply` arm alone, where it belongs.
    fn direction(&self) -> Operation {
        match self {
            DeclarationChange::Add(_) => Operation::Promote,
            DeclarationChange::Remove(_) => Operation::Demote,
            DeclarationChange::ApprovedApply { operation, .. } => *operation,
        }
    }

    /// Whether this change grows the list.
    fn is_promotion(&self) -> bool {
        self.direction() == Operation::Promote
    }

    /// The `request_id` of an [`DeclarationChange::ApprovedApply`], if this is one.
    fn request_id(&self) -> Option<&str> {
        match self {
            DeclarationChange::ApprovedApply { request_id, .. } => Some(request_id),
            _ => None,
        }
    }
}

/// Per-operation options. Not a clap type, for the same reason as
/// [`crate::operations::rebuild::RebuildPlanOptions`]: the planner must be
/// callable from tests and from a non-CLI surface.
#[derive(serde::Serialize, Debug, Clone, Copy, Default)]
pub struct DeclarationPlanOptions {
    /// `hitch promote --no-rebuild` / `hitch demote --no-rebuild`. Leaves the
    /// environment branch stale **on purpose**, so the plan has to say so
    /// rather than quietly omit the consequence.
    pub no_rebuild: bool,
}

/// The declaration-change payload.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct DeclarationPlanDetail {
    pub environment: String,
    /// **The positional argument the user typed**, not the resolved branch
    /// list. A stale-plan remedy has to reproduce the command that was run,
    /// and `hitch promote dev qa` (which expands `dev`'s branches) and
    /// `hitch promote feat-a qa` are different commands that do the same thing.
    pub argument: String,
    /// The resolved edit, in the order given.
    ///
    /// `removed` is already narrowed to the branches that *are* promoted, even
    /// though the argument may name some that are not (see
    /// [`DeclarationChange::Remove`]). Narrowing happens here so the effect
    /// list and the fingerprint describe the same edit the executor applies.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// The declaration *after* this plan, in declaration order, with every
    /// branch pinned to the SHA the plan read. This is what the nested
    /// rebuild will compose, so it is computed here rather than re-read after
    /// the write — re-reading would be a second decision point.
    pub proposed_branches: Vec<PinnedBranch>,
    /// Every branch tip the plan consulted, including the existing promoted
    /// branches the sibling check needed. A plan that reads a ref and does not
    /// fingerprint it cannot be validated, so this and the fingerprint are the
    /// same set by construction.
    pub read_branches: Vec<PinnedBranch>,
    /// `false` under `--no-rebuild`.
    pub rebuild: bool,
    /// The approval requests the executor created. Empty on every path except
    /// [`OperationOutcome::ApprovalRequested`], and deliberately *not* filled in
    /// by the planner: creating a request writes `hitch.json`, and a planner
    /// that mutates cannot be shown to a human before it runs.
    pub approval_requests: Vec<String>,
}

/// The three answers "what will this change" needs, from the current
/// declaration and the requested change.
///
/// **Order is composition order, so nothing here sorts.** A promoted list is
/// folded into the environment one branch at a time, in the order it is
/// declared, which is exactly why a plan that sorted the list would be
/// describing a *different environment* rather than a differently-spelled one
/// — and the difference only shows up in a three-way conflict, which is to say
/// in the case where being wrong is most expensive.
///
/// A demote narrows to the branches actually present, because the
/// environment-name-expands-to-branches form can name branches this
/// environment never promoted, and removing one that is not there is not a
/// change worth reporting.
pub fn proposed_declaration(
    current: &[String],
    change: &DeclarationChange,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let branches = change.branches();
    match change.direction() {
        Operation::Promote => {
            // A branch that is already promoted is folded in rather than appended
            // a second time, so re-running an approved apply after a partial
            // apply yields the same declaration and the plan reports `NoChange` —
            // which is the honest description of a re-run, and what makes it safe
            // to retry. `Add` needs no such guard (a branch already promoted is
            // refused before it gets here), so this costs the ordinary promote
            // nothing.
            let added: Vec<String> = branches
                .iter()
                .filter(|b| !current.contains(*b))
                .cloned()
                .collect();
            let mut next = current.to_vec();
            for branch in branches {
                if !next.contains(branch) {
                    next.push(branch.clone());
                }
            }
            (added, Vec::new(), next)
        }
        Operation::Demote => {
            let removed: Vec<String> = branches
                .iter()
                .filter(|b| current.contains(*b))
                .cloned()
                .collect();
            let kept: Vec<String> = current
                .iter()
                .filter(|b| !removed.contains(b))
                .cloned()
                .collect();
            (Vec::new(), removed, kept)
        }
    }
}

/// Plan a promote. `argument` is the user's original positional argument.
pub fn plan_promote(
    context: &GlobalContext,
    argument: &str,
    environment: &str,
    options: DeclarationPlanOptions,
    on_step: &mut dyn FnMut(&str),
) -> Result<OperationPlan<DeclarationPlanDetail>> {
    let resolved = resolve_to_branches(context, argument, environment, OperationKind::Promote)?;
    plan_declaration_change(
        context,
        environment,
        DeclarationChange::Add(resolved),
        argument,
        options,
        on_step,
    )
}

/// Plan a demote. `argument` is the user's original positional argument.
pub fn plan_demote(
    context: &GlobalContext,
    argument: &str,
    environment: &str,
    options: DeclarationPlanOptions,
    on_step: &mut dyn FnMut(&str),
) -> Result<OperationPlan<DeclarationPlanDetail>> {
    let resolved = resolve_to_branches(context, argument, environment, OperationKind::Demote)?;
    plan_declaration_change(
        context,
        environment,
        DeclarationChange::Remove(resolved),
        argument,
        options,
        on_step,
    )
}

/// Plan the declaration change an approval has already authorised.
///
/// The third entry point over this planner, and the only one whose `change` is
/// not derived from a positional argument: `hitch approve` is resuming an
/// operation whose declaration edit was decided when the request was filed, so
/// there is nothing to resolve and nothing to expand.
///
/// **No `OperationKind` parameter**, and that is the point rather than an
/// omission: the kind comes from `change.kind()`, which is the same
/// [`DeclarationChange::ApprovedApply`] value the caller already holds. A
/// parameter beside the change would be a second source of truth for one fact,
/// and the one this planner spends the most care on — a plan whose `kind`
/// disagreed with the change it was built from would render a heading for one
/// operation and apply another.
///
/// `argument` is the request id rather than a branch name, because that is what
/// a stale plan's remedy and a receipt have to name: `hitch approve <id>`
/// resumes at exactly the point the failure left off, where `hitch promote <id>`
/// would ask for an authorisation that is already committed.
pub fn plan_approved_declaration_change(
    context: &GlobalContext,
    environment: &str,
    change: DeclarationChange,
    argument: &str,
    options: DeclarationPlanOptions,
    on_step: &mut dyn FnMut(&str),
) -> Result<OperationPlan<DeclarationPlanDetail>> {
    debug_assert!(
        matches!(change, DeclarationChange::ApprovedApply { .. }),
        "the approved-apply entry point takes an approval-authorised change; anything else \
         would be a plan whose approval gate this planner skips for no reason"
    );
    plan_declaration_change(context, environment, change, argument, options, on_step)
}

/// Expand an environment name into the branches promoted in it, or treat the
/// argument as a single branch.
///
/// This lives in the planner rather than the command because the expansion
/// result is an input to the proposal, the effect list *and* the fingerprint —
/// resolving it in the command would mean two places know the answer, and they
/// would eventually disagree.
///
/// `kind` only selects the wording of two refusals. Promote and demote have
/// always phrased these differently ("into" vs "from", "promote" vs
/// "demote"), and the difference is the reader's clue that the command they
/// typed is not the operation being described — so it is preserved rather than
/// normalised away. It is also load-bearing for the *order* of the two checks:
/// a demote reports "from itself" before reporting "has no branches", and
/// swapping them would change which message a user sees.
fn resolve_to_branches(
    context: &GlobalContext,
    argument: &str,
    environment: &str,
    kind: OperationKind,
) -> Result<Vec<String>> {
    let config = access_metadata_read_only(context, |c| Ok(c.clone()))?;

    if let Some(source) = config.environments.get(argument) {
        if kind == OperationKind::Demote && argument == environment {
            anyhow::bail!("Cannot demote environment '{}' from itself", argument);
        }
        if source.branches.is_empty() {
            anyhow::bail!(
                "Environment '{}' has no branches promoted. Nothing to {}.",
                argument,
                if kind == OperationKind::Demote {
                    "demote"
                } else {
                    "promote"
                }
            );
        }
        if kind == OperationKind::Promote && argument == environment {
            anyhow::bail!("Cannot promote environment '{}' into itself", argument);
        }
        return Ok(source.branches.clone());
    }
    Ok(vec![argument.to_string()])
}

/// Build the plan for a declaration change.
#[allow(clippy::too_many_arguments)] // one argument per distinct fact the plan records; a struct here would be a second model
fn plan_declaration_change(
    context: &GlobalContext,
    environment: &str,
    change: DeclarationChange,
    argument: &str,
    options: DeclarationPlanOptions,
    on_step: &mut dyn FnMut(&str),
) -> Result<OperationPlan<DeclarationPlanDetail>> {
    on_step("Validating promotion preconditions");
    let kind = change.kind();
    // Total, with no wildcard. A `_ => "demotion"` here would have labelled
    // every operation added after this code was written as a demotion, and the
    // word only shows up under `--verbose` — so the failure would be a wrong
    // word in a log line that nobody reads, in a planner that is otherwise
    // correct. Cheap to make exhaustive; expensive to discover.
    let verb = match kind {
        OperationKind::Promote | OperationKind::ApprovalApply => "promotion",
        OperationKind::Demote => "demotion",
        other => {
            unreachable!(
                "{} is not a declaration change; plan_declaration_change takes a \
                 DeclarationChange, so its kind is one of the three above (got {other})",
                other
            )
        }
    };
    context.log_verbose(&format!("Validating {} preconditions...", verb));

    for branch in change.branches() {
        validate_name(branch, "Branch")?;
    }
    validate_name(environment, "Environment")?;
    ensure_environment_exists(context, environment)?;

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let declared = config
        .environments
        .get(environment)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", environment))?;

    // **No `locked` check here, on purpose.** The planner is called from inside
    // `with_locked_env`, which commits the lock to `hitch-metadata` *before*
    // running its closure — so by the time a plan is built, the environment it
    // is about to edit is always locked, and a check here would refuse every
    // promotion. Refusing a lock a *human* set is the command's job, and it
    // happens before the lock is taken; see `promote::run`. This matches
    // `commands/rebuild.rs`, which does the same check at the same point.

    match change.direction() {
        // `Add` and `ApprovedApply`-promoting share these checks, with one
        // difference: the already-promoted case is a `log_verbose` for an
        // approved change rather than a bail. A re-run after a partial apply is
        // legal and is the *designed* recovery for `hitch approve` — the
        // `ApprovalStatus::Approved` arm re-drives exactly this — so erroring
        // would turn the recovery path into the one path that cannot recover.
        // The plan's `NoChange` outcome is the honest description of that re-run.
        Operation::Promote => {
            for branch in change.branches() {
                if declared.branches.contains(branch) {
                    if change.request_id().is_some() {
                        context.log_verbose(&format!(
                            "Branch '{}' is already promoted to environment '{}'; nothing to add",
                            branch, environment
                        ));
                        continue;
                    }
                    anyhow::bail!(
                        "Branch '{}' is already promoted to environment '{}'",
                        branch,
                        environment
                    );
                }
                validate_branch_for_promotion(context, branch)?;
            }
        }
        Operation::Demote => {
            // Same asymmetry, same reason: a demotion whose branches are all
            // already demoted has nothing left to remove, which is a `NoChange`
            // plan rather than an error — *for an approved change*. A user's own
            // `hitch demote b prod` against a branch prod never promoted is a
            // mistake worth reporting, and stays a bail.
            if !change
                .branches()
                .iter()
                .any(|b| declared.branches.contains(b))
                && change.request_id().is_none()
            {
                anyhow::bail!(
                    "None of the branches from '{}' are promoted to environment '{}'",
                    change.branches().join(", "),
                    environment
                );
            }
        }
    }

    // Pin every branch this plan reads: the ones being added, and the ones
    // already promoted. The latter are read by the sibling-conflict
    // simulation *and* become the nested rebuild's base composition, so
    // omitting them from the fingerprint would leave a plan that reads a ref
    // it cannot notice changing.
    let git = context.git();
    let mut read_branches: Vec<PinnedBranch> = Vec::new();
    for branch in declared.branches.iter().chain(change.branches()) {
        if read_branches.iter().any(|p| &p.branch == branch) {
            continue;
        }
        let sha = git
            .rev_parse_opt(&format!("refs/heads/{}", branch))?
            .with_context(|| {
                format!(
                    "Branch '{}' does not exist locally, so it cannot be promoted into '{}'.",
                    branch, environment
                )
            })?;
        read_branches.push(PinnedBranch {
            branch: branch.clone(),
            sha,
        });
    }

    // The sibling-conflict simulation, run *in the plan*. This used to be a
    // pre-check in `promote.rs` that intercepted before anything was
    // describable; the verdict is a decision, and a decision belongs where the
    // reader can see it. `pre_promote_conflict_reason` returns today's error
    // text verbatim, which is what keeps
    // `test_promote_blocked_by_sibling_conflict`'s three assertions true.
    let mut warnings: Vec<PlanWarning> = Vec::new();
    let mut refused: Option<String> = None;
    if change.is_promotion() && !declared.branches.is_empty() {
        context.log_verbose("Checking for conflicts with already-promoted branches...");
        for branch in change.branches() {
            if let Some(reason) = pre_promote_conflict_reason(
                context,
                branch,
                &declared.branches,
                &declared.base,
                environment,
            )? {
                refused = Some(reason);
                break;
            }
        }
    }
    if let Some(reason) = &refused {
        // No remedy override, and the reason is why: for *this* refusal the
        // default is right. The reason text already names the unblocker — "Fix
        // feat-b first: `git checkout feat-b && git rebase main`" — and the
        // promote itself is genuinely still the command to run once the rebase
        // lands. `hitch lock dev` on a locked environment is the opposite case,
        // where the default names the command that just failed and can only
        // fail again; that is the distinction a remedy override exists for, and
        // getting it the wrong way round is worse than not having it.
        warnings.push(PlanWarning::policy_refusal(reason.clone()));
    }

    // The approval gate. A gated plan proposes nothing: composing a
    // declaration the user has not approved yet would be a plan for an
    // operation that will not happen, and the "what would change" section of
    // the plan would be fiction.
    //
    // A refusal outranks the gate. If the plan could not be applied even with
    // approval — a branch that conflicts with a sibling — then asking for
    // approval would be asking the user to authorise something that will then
    // be refused, and the request would sit pending against a change that can
    // never be applied. So the gate is only recorded when nothing else blocks.
    //
    // An `ApprovedApply` change is never gated, and that is the whole point of
    // the variant: the approval has already been granted and committed, so
    // re-requesting one would be asking the user to authorise a second time the
    // thing they authorised. The condition lives *here* rather than at the call
    // site in `approve.rs` because a gate flag passed in from outside could
    // disagree with the change it is claimed to describe, and the two places
    // would be free to drift.
    let approval_gated = !matches!(change, DeclarationChange::ApprovedApply { .. })
        && refused.is_none()
        && declared.requires_approval_check();
    if approval_gated {
        context.log_info(&format!(
            "Environment '{}' requires approval before {}",
            environment, verb
        ));
        warnings.push(PlanWarning::approval_required(format!(
            "Environment '{}' requires approval before {}",
            environment, verb
        )));
    }

    // The proposed declaration: the current list, in order, plus the additions
    // appended, minus the removals.
    let base = declared.branches.clone();
    let (added, removed, proposed_names) = proposed_declaration(&base, &change);

    let pinned_by_name: BTreeMap<&str, &str> = read_branches
        .iter()
        .map(|p| (p.branch.as_str(), p.sha.as_str()))
        .collect();
    let proposed_branches: Vec<PinnedBranch> = proposed_names
        .iter()
        .filter_map(|name| {
            pinned_by_name.get(name.as_str()).map(|sha| PinnedBranch {
                branch: name.clone(),
                sha: (*sha).to_string(),
            })
        })
        .collect();

    let env_ref = format!("refs/heads/{}", environment);
    let env_sha_before = git.rev_parse_opt(&env_ref)?;

    let projection =
        |branches: Vec<PinnedBranch>, branch_sha: Option<String>| EnvironmentProjection {
            environment: environment.to_string(),
            base: declared.base.clone(),
            branches,
            branch_sha,
        };
    let current = projection(
        base.iter()
            .filter_map(|name| {
                pinned_by_name.get(name.as_str()).map(|sha| PinnedBranch {
                    branch: name.clone(),
                    sha: (*sha).to_string(),
                })
            })
            .collect(),
        env_sha_before.clone(),
    );
    let proposed = if approval_gated || refused.is_some() {
        // A blocked plan proposes nothing that will not happen.
        current.clone()
    } else {
        projection(proposed_branches.clone(), None)
    };

    // Effects. Empty for a blocked plan, because none of them will happen.
    let mut effects: Vec<PlannedEffect> = Vec::new();
    if !approval_gated && refused.is_none() {
        // The verb comes from the change's *direction*, never from its kind.
        // `ApprovalApply` covers both directions — an approved demotion is
        // approved too — so a `kind`-keyed verb would print "promote" on the
        // plan for an approved demotion, and `unreachable!` on the
        // `shortened`/`extended` pair would never fire because `ApprovalApply`
        // had to be listed as promoting for both to compile. Two facts, two
        // places: one of them had to be wrong, and the one that was wrong was
        // the one that could not be checked by the compiler.
        let promoting = change.is_promotion();
        // The branches this edit names come from the *direction*, keyed on
        // `promoting` exactly as the verb and the preposition are — not from
        // `added.is_empty()`. `proposed_declaration` guarantees `removed` is
        // empty for a promotion and `added` is empty for a demotion, so that test
        // cannot distinguish "this is a demotion" from "this is a promotion with
        // nothing left to add", and a re-run of an approved apply hit the second
        // case and printed `promote  into 'dev'` with no branches in it at all.
        let named = if promoting { &added } else { &removed };
        effects.push(PlannedEffect::MetadataChange {
            refname: "refs/heads/hitch-metadata".to_string(),
            description: format!(
                "{} {} {} '{}'",
                if promoting { "promote" } else { "demote" },
                branch_list(named),
                if promoting { "into" } else { "out of" },
                environment,
            ),
        });
        if !options.no_rebuild {
            effects.push(PlannedEffect::DependentEnvironmentRebuild {
                environment: environment.to_string(),
                because: format!(
                    "its declaration is being {} by this plan",
                    if promoting { "extended" } else { "shortened" }
                ),
                refname: env_ref.clone(),
            });
        } else {
            warnings.push(PlanWarning::advisory(format!(
                "'{}' will be left stale until it is rebuilt. To rebuild it:\n  hitch rebuild {}",
                environment, environment
            )));
        }
    }

    let mut unaffected = vec![UnaffectedResource {
        kind: crate::operations::model::ResourceKind::Branch,
        name: declared.base.clone(),
    }];
    for other in config.environments.keys() {
        if other != environment {
            unaffected.push(UnaffectedResource {
                kind: crate::operations::model::ResourceKind::Environment,
                name: other.clone(),
            });
        }
    }

    // The fingerprint. `refs/heads/<environment>` is tracked because the
    // nested rebuild moves it: a concurrent rebuild between plan and apply is
    // exactly the kind of change this plan must refuse.
    let mut fingerprint = PlanFingerprint::new();
    fingerprint.metadata_sha = git
        .rev_parse_opt("refs/heads/hitch-metadata")
        .unwrap_or(None);
    for pinned in &read_branches {
        fingerprint.track_ref(format!("refs/heads/{}", pinned.branch), &pinned.sha);
    }
    if let Some(sha) = &env_sha_before {
        fingerprint.track_ref(&env_ref, sha);
    }
    let digest = fingerprint.digest(git)?;

    let detail = DeclarationPlanDetail {
        environment: environment.to_string(),
        argument: argument.to_string(),
        added,
        removed,
        proposed_branches,
        read_branches,
        rebuild: !options.no_rebuild,
        approval_requests: Vec::new(),
    };

    Ok(OperationPlan {
        id: format!("{}:{}:{}:{}", kind, environment, argument, digest),
        kind,
        intent: match &change {
            DeclarationChange::Add(branches) => OperationIntent::PromoteBranches {
                environment: environment.to_string(),
                branches: branches.clone(),
            },
            DeclarationChange::Remove(branches) => OperationIntent::DemoteBranches {
                environment: environment.to_string(),
                branches: branches.clone(),
            },
            DeclarationChange::ApprovedApply {
                branches,
                request_id,
                ..
            } => OperationIntent::ApplyApproval {
                request_id: request_id.clone(),
                environment: environment.to_string(),
                branches: branches.clone(),
            },
        },
        fingerprint,
        current: Some(current),
        proposed: Some(proposed),
        compositions: Vec::new(),
        effects,
        unaffected,
        warnings,
        // Promote into an approval-gated environment is the one case where a
        // confirmation cannot be answered with a plain yes: the answer is
        // "approve", which is a different command. The plan says so rather than
        // asking a question whose answer does not exist.
        //
        // Phrased as *what confirming will do* rather than as a restatement of
        // the warning, because this text is printed above the prompt
        // (`render::confirmation_question`) and the plan's own "Will change"
        // section is empty in this case — the apply files an approval request
        // instead of editing the declaration. Without this, the prompt asked the
        // user to authorise a plan that visibly does nothing, which is the one
        // thing a confirmation must never be.
        confirmation: if approval_gated {
            ConfirmationRequirement::required(format!(
                "'{}' requires approval, so confirming files an approval request \
                 for this {} rather than changing the declaration.\n  \
                 To review pending requests: hitch approvals list",
                environment,
                // Total, and deliberately so. This arm is only reachable for a
                // promote or a demote — an approved apply is past the gate by
                // construction, and nothing else reaches this planner — but a
                // `unreachable!` here would be a panic in a library reachable
                // from a future `OperationKind` variant, bought for nothing: the
                // fallback is one word in a sentence about a case that cannot
                // occur. The direction comes from the change, not the kind, for
                // the reason given on the effects above.
                if change.is_promotion() {
                    "promotion"
                } else {
                    "demotion"
                },
            ))
        } else {
            ConfirmationRequirement::not_required()
        },
        detail,
    })
}

/// This plan's own cleanup.
///
/// **Deliberately empty.** A declaration plan owns no ref: it composes
/// nothing, anchors nothing, and the lock belongs to the command. The function
/// exists anyway, because "a plan that will not be applied releases what it
/// holds" is a property every planner owes, and writing it down now means a
/// future agent who *does* give a declaration plan a composed commit has an
/// obvious place to put the release rather than inventing a second convention.
/// Do not read the emptiness as a bug.
pub fn discard_declaration_plan(
    _context: &GlobalContext,
    _plan: &OperationPlan<DeclarationPlanDetail>,
) {
}

/// Refuse a plan whose inputs moved. See
/// [`crate::operations::model::changed_inputs`] for the one rule this is.
pub fn validate_declaration_plan(
    context: &GlobalContext,
    plan: &OperationPlan<DeclarationPlanDetail>,
) -> std::result::Result<(), PlanApplyError> {
    let changed = changed_inputs(&plan.fingerprint, context.git());
    if changed.is_empty() {
        return Ok(());
    }
    Err(PlanApplyError::stale_plan(
        plan.kind,
        &plan.detail.environment,
        &plan.detail.argument,
        &changed,
    ))
}

/// Apply a declaration plan: the metadata edit, then the rebuild it forces.
pub fn apply_declaration_plan(
    context: &GlobalContext,
    plan: &OperationPlan<DeclarationPlanDetail>,
    on_step: &mut dyn FnMut(&str),
) -> Result<ExecutionReceipt> {
    let started_at = chrono::Utc::now();
    validate_declaration_plan(context, plan).map_err(PlanApplyError::into_anyhow)?;

    if let Some(blocking) = plan.blocked_by() {
        return apply_blocked_plan(context, plan, blocking.kind, started_at);
    }

    let environment = plan.detail.environment.as_str();
    on_step(&format!("Updating '{}' declaration", environment));
    context.log_verbose(&format!(
        "Updating environment '{}' with {} branch(es)...",
        environment,
        plan.detail.added.len() + plan.detail.removed.len()
    ));

    // The edit is applied *verbatim from the plan*. Re-deriving it here would be
    // a second decision point, and the whole point of the edit being in the
    // plan is that it is decided.
    let added = plan.detail.added.clone();
    let removed = plan.detail.removed.clone();
    modify_metadata(context, |config| {
        let available_envs = config.get_environment_names().join(", ");
        let env = config.get_environment_mut(environment).ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' not found in hitch configuration. Available environments: {}",
                environment,
                available_envs
            )
        })?;
        for branch in &added {
            env.add_branch(branch.clone());
            context.log_verbose(&format!(
                "✓ Added '{}' to environment '{}'",
                branch, environment
            ));
        }
        for branch in &removed {
            env.remove_branch(branch);
            context.log_verbose(&format!(
                "✓ Removed '{}' from environment '{}'",
                branch, environment
            ));
        }
        Ok(())
    })?;

    // Empty, and that is the contract rather than an oversight: see
    // `ExecutionReceipt::warnings`. This used to copy every non-blocking plan
    // warning, which is to say the "`dev` will be left stale until it is
    // rebuilt" advisory — a consequence of a flag the *user* passed, decided
    // before the apply started, and re-printed verbatim below a receipt that
    // had already applied. The prediction is the plan's; the fact is the Result
    // block's `⧗ dev   needs rebuild`, read from the authority.
    let mut warnings: Vec<ExecutionWarning> = Vec::new();

    // Read the declaration back rather than describing the edit from the plan.
    // The plan said what it intended; this says what is there. A description
    // built from `plan.detail.added` would report success for an edit that a
    // later `hitch set` had already undone.
    let declared_now = access_metadata_read_only(context, |config| {
        let env = config
            .environments
            .get(environment)
            .map(|e| e.branches.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("Environment '{}' is no longer declared", environment)
            })?;
        Ok(env)
    })?;
    let mut effects = vec![AppliedEffect::MetadataChange {
        refname: "refs/heads/hitch-metadata".to_string(),
        description: applied_declaration_description(plan, &declared_now),
    }];

    // Nothing is said here when `--no-rebuild` left the environment stale. It
    // used to print "Skipping rebuild for environment 'dev' (--no-rebuild flag
    // set). Run 'hitch rebuild dev' when ready." — a fourth rendering of one
    // fact, in the gap between the halves: the plan's advisory already says the
    // environment will be left stale and how to fix it, the receipt copy below
    // said it again, and the Result block closes with `⧗ dev   needs rebuild`
    // from the authority. Same class as the `StepLogger` transcript this branch
    // is nested inside of: a second voice for a decision the user already made
    // with a flag, above the one document that accounts for it.
    if plan.detail.rebuild {
        // The nested rebuild goes quiet. Its plan said `rebuild {env} — …`
        // above, the receipt below says what became of it, and the
        // `RebuildOutcome` carries the one fact neither of those can derive —
        // which branches the build held. A `StepLogger` transcript here would
        // be a second and older vocabulary narrating work the reader has
        // already been told about, sitting between the two halves that
        // actually account for it.
        match rebuild_environment(context, environment, StepNarration::Suppressed) {
            Ok(outcome) => {
                context.log_verbose(&format!(
                    "✓ Environment '{}' rebuilt successfully",
                    environment
                ));
                effects.push(AppliedEffect::DependentEnvironmentRebuild {
                    environment: environment.to_string(),
                    outcome: DependentRebuildOutcome::Rebuilt,
                    held: outcome.held.iter().map(HoldPair::from).collect(),
                    refname: format!("refs/heads/{}", environment),
                });
            }
            Err(e) => {
                // The declaration edit has landed and is durable. The rebuild
                // not having happened is owed work, not a failed promote:
                // reporting failure here would tell the user to re-run a command
                // whose first half is already applied, and re-running a promote
                // fails with "already promoted".
                //
                // One `log_warning` here, not two. The `ExecutionWarning` below
                // renders under the receipt's "Still owed" heading carrying the
                // same remedy, and the effect above it already says `failed`;
                // a third copy on stdout in the middle of the apply is the same
                // duplication this whole function is being cleaned up for.
                effects.push(AppliedEffect::DependentEnvironmentRebuild {
                    environment: environment.to_string(),
                    outcome: DependentRebuildOutcome::Failed(e.to_string()),
                    held: Vec::new(),
                    refname: format!("refs/heads/{}", environment),
                });
                warnings.push(ExecutionWarning {
                    message: format!(
                        "'{}' was updated, but its rebuild did not run:\n  {}\n  \
                         The declaration is saved; the branch is stale. To rebuild it:\n  \
                         hitch rebuild {}",
                        environment, e, environment
                    ),
                    owes_effect: true,
                });
            }
        }
    }

    Ok(ExecutionReceipt {
        plan_id: plan.id.clone(),
        operation: plan.kind,
        started_at,
        completed_at: chrono::Utc::now(),
        outcome: OperationOutcome::Applied,
        effects,
        warnings,
        resulting_state: build_state_snapshot(context).ok(),
    })
}

/// The two "the plan does not apply" paths, which are different outcomes and
/// must not be collapsed.
///
/// An approval gate *asks*; the CLI exits 0 and the user runs `hitch approve`.
/// A policy refusal *refuses*; the CLI exits 1 and nothing was written. The
/// plan cannot tell them apart by "is it blocking" alone, which is the whole
/// reason [`crate::operations::model::PlanWarningKind`] is an enum.
fn apply_blocked_plan(
    context: &GlobalContext,
    plan: &OperationPlan<DeclarationPlanDetail>,
    kind: crate::operations::model::PlanWarningKind,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<ExecutionReceipt> {
    use crate::operations::model::PlanWarningKind;

    let environment = plan.detail.environment.as_str();
    match kind {
        PlanWarningKind::ApprovalRequired => {
            // All requests in one transaction so a mid-batch failure (one
            // branch already has a pending request) cannot leave the earlier
            // ones committed — the comment that used to live in `promote.rs`,
            // moving with the code.
            let requested = match plan.kind {
                OperationKind::Promote => plan.detail.added.clone(),
                OperationKind::Demote => plan.detail.removed.clone(),
                other => {
                    unreachable!("only a promote or a demote can be approval-gated (got {other})")
                }
            };
            let requests = create_approval_requests_for_operation(
                context,
                environment,
                &requested,
                match plan.kind {
                    OperationKind::Promote => Operation::Promote,
                    OperationKind::Demote => Operation::Demote,
                    other => unreachable!(
                        "only a promote or a demote can be approval-gated (got {other})"
                    ),
                },
            )?;
            for id in &requests {
                display_approval_request_created(context, id)?;
            }
            Ok(ExecutionReceipt {
                plan_id: plan.id.clone(),
                operation: plan.kind,
                started_at,
                completed_at: chrono::Utc::now(),
                outcome: OperationOutcome::ApprovalRequested,
                effects: vec![AppliedEffect::MetadataChange {
                    refname: "refs/heads/hitch-metadata".to_string(),
                    description: format!(
                        "{} approval request(s) for '{}'",
                        requests.len(),
                        environment
                    ),
                }],
                // Empty, per `ExecutionReceipt::warnings`. This used to copy
                // *every* plan warning, blocking ones included, so an
                // approval-gated promote printed
                // `⛔ Environment 'prod' requires approval before promotion` in
                // the plan and then the identical sentence again as a receipt
                // warning — the same words under a different glyph, since a
                // blocking plan warning renders `⛔` and a non-owed receipt
                // warning renders `⚠️`. One fact wearing two urgencies in two
                // documents is worse than the duplication it replaces.
                //
                // The approval's substance does not go missing: the effect above
                // says a request was created, `outcome` is
                // `ApprovalRequested` (exit 0), and the command prints the
                // request id and the `hitch approvals list` next step.
                warnings: Vec::new(),
                resulting_state: build_state_snapshot(context).ok(),
            })
        }
        PlanWarningKind::PolicyRefusal => {
            // The *whole* warning, not just its message: the remedy is a
            // separate field on it, and taking the message alone would be the
            // shape that made the remedy unreachable.
            let blocking = plan
                .warnings
                .iter()
                .find(|w| w.kind == PlanWarningKind::PolicyRefusal)
                .expect("the arm is only reachable through `blocked_by`, which returns a warning");
            Err(PlanApplyError::PolicyBlocked {
                environment: environment.to_string(),
                reason: blocking.message.clone(),
                // `command_hint` already spells the full `hitch …` invocation.
                // The extra `"hitch {}"` wrapper this used to add printed the
                // word twice — `hitch hitch promote …` — in the one remedy a
                // user is told to copy. And the hint is only the default: a
                // refusal whose unblocker is a different command supplies its
                // own, because "re-run the promote that just conflicted" is not
                // an answer.
                remedy: blocking
                    .remedy_for(&plan.kind.command_hint(environment, &plan.detail.argument)),
            }
            .into_anyhow())
        }
        // Unreachable: `blocked_by` only returns a blocking warning, and
        // `Advisory` is not blocking. Treated as a refusal rather than a
        // `match` arm that silently applies a plan someone believed was
        // blocked.
        PlanWarningKind::Advisory => Err(PlanApplyError::PolicyBlocked {
            environment: environment.to_string(),
            reason: "internal error: an advisory warning was treated as blocking".to_string(),
            remedy: Some(plan.kind.command_hint(environment, &plan.detail.argument)),
        }
        .into_anyhow()),
    }
}

/// A branch list that survives being empty.
///
/// Its own function because it is the answer to a question both a *plan* and a
/// *receipt* ask — "which branches does this edit name?" — and an empty list is
/// a real answer here, not a degenerate one: an approved apply re-run after a
/// partial apply genuinely has nothing left to declare. A `format!` site that
/// inlined the join printed `promote  into 'dev'`, a double space for a branch
/// list of nothing.
fn branch_list(branches: &[String]) -> String {
    if branches.is_empty() {
        "nothing".to_string()
    } else {
        branches.join(", ")
    }
}

/// Describe the declaration as it now stands, naming what changed rather than
/// what was intended.
fn applied_declaration_description(
    plan: &OperationPlan<DeclarationPlanDetail>,
    declared_now: &[String],
) -> String {
    let environment = &plan.detail.environment;
    let list = branch_list;
    let before = plan.current_composition();
    match plan.kind {
        OperationKind::Promote | OperationKind::ApprovalApply => {
            // Computed from the *declaration as it is now* rather than from
            // `detail.added`, so a re-run that added nothing describes itself
            // honestly ("promote nothing into 'dev' (now: feat-a)") instead of
            // claiming the addition the earlier attempt made.
            let added = declared_now
                .iter()
                .filter(|b| !before.branches.iter().any(|p| &p.branch == *b))
                .cloned()
                .collect::<Vec<_>>();
            format!(
                "promote {} into '{}' (now: {})",
                list(&added),
                environment,
                list(declared_now)
            )
        }
        OperationKind::Demote => {
            let removed = before
                .branches
                .iter()
                .filter(|p| !declared_now.contains(&p.branch))
                .map(|p| p.branch.clone())
                .collect::<Vec<_>>();
            format!(
                "demote {} out of '{}' (now: {})",
                list(&removed),
                environment,
                list(declared_now)
            )
        }
        other => unreachable!("not a declaration change: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn the_direction_decides_the_kind() {
        assert_eq!(
            DeclarationChange::Add(s(&["feat-a"])).kind(),
            OperationKind::Promote
        );
        assert_eq!(
            DeclarationChange::Remove(s(&["feat-a"])).kind(),
            OperationKind::Demote
        );
        assert_eq!(
            DeclarationChange::Add(s(&["a", "b"])).branches(),
            s(&["a", "b"])
        );
    }

    /// The single most dangerous thing this module could do wrong: sort the
    /// list. A three-branch promote deliberately takes branches whose sorted
    /// order differs from their given order, so a sort would be visible here
    /// rather than only in a three-way conflict months later.
    #[test]
    fn a_promote_appends_in_the_given_order_and_never_sorts() {
        let (added, removed, proposed) = proposed_declaration(
            &s(&["zeta", "alpha"]),
            &DeclarationChange::Add(s(&["mid", "beta"])),
        );
        assert_eq!(added, s(&["mid", "beta"]));
        assert!(removed.is_empty());
        assert_eq!(proposed, s(&["zeta", "alpha", "mid", "beta"]));
    }

    /// A demote preserves the *survivors'* order rather than rebuilding it,
    /// for the same reason: the remaining branches still fold in this order.
    #[test]
    fn a_demote_keeps_survivors_in_declaration_order() {
        let (added, removed, proposed) = proposed_declaration(
            &s(&["zeta", "alpha", "mid"]),
            &DeclarationChange::Remove(s(&["alpha"])),
        );
        assert!(added.is_empty());
        assert_eq!(removed, s(&["alpha"]));
        assert_eq!(proposed, s(&["zeta", "mid"]));
    }

    /// Expanding an environment name into another environment's branch list can
    /// name branches this environment never promoted. Reporting a removal of a
    /// branch that was never there would be a plan claiming a change it cannot
    /// make — and the CLI's "nothing was demoted" message would be a lie.
    #[test]
    fn a_demote_of_an_absent_branch_is_not_a_change() {
        let (added, removed, proposed) = proposed_declaration(
            &s(&["feat-a"]),
            &DeclarationChange::Remove(s(&["feat-a", "never-here"])),
        );
        assert!(added.is_empty());
        assert_eq!(removed, s(&["feat-a"]));
        assert!(proposed.is_empty());
    }
}
