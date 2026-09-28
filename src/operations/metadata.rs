//! The metadata operations: `lock`, `unlock`, `set`, `add`, `remove`.
//!
//! Read [`crate::operations`]'s header first. Three things about this file are
//! load-bearing, and two of them are the *opposite* of what the other planners
//! do — which is exactly why they are written down rather than left to be
//! inferred from `rebuild.rs`.
//!
//! **A metadata plan's decision is the resolved edit, and nothing else.** Every
//! planner here computes the difference between the declaration as it is and
//! the declaration as it will be, from the declaration itself. Not the clap
//! args: `hitch set dev --add-approver alice@example.com` run twice makes no
//! change the second time, and a plan built from the flags would claim an edit
//! that does not exist, print it, ask the user to confirm it, and then write
//! nothing — four documents about an operation that never happened.
//!
//! **A metadata operation has no rollback, and must not grow one.** Its entire
//! effect is one [`modify_metadata`] closure, and `modify_metadata_impl` runs
//! the closure *before* `write_file`/`commit_branch_write` — so a closure that
//! returns `Err` has committed nothing at all. That is the whole of the safety
//! argument, and it is why the two `attempt_*_rollback` helpers in
//! `approvals/approve.rs` are, on every path that reaches them, restoring a
//! snapshot onto a ref that already holds it: a no-op commit wrapped in a
//! narrative about a repair. Promote and demote *do* keep their rollback,
//! because their nested rebuild can fail *after* the edit commits and leave the
//! declaration and the environment branch disagreeing. Nothing in this file has
//! that shape, and a rollback added "just in case" would be a second code path
//! with no failure to serve.
//!
//! **A metadata plan anchors nothing, so there is nothing to forget.** The
//! unconditional `finally` that discards `apply_rebuild_plan`'s anchor exists
//! because a `commit-tree` commit is unreachable until its publish CAS lands,
//! and a `git gc --prune=now` in that window would collect it. There is no such
//! commit here: the whole effect is a commit to `hitch-metadata` made by
//! `commit_branch_write`, which creates the object and moves the ref in the
//! same step. Do not copy the `finally` here without the reason it exists; the
//! absence is a property, not an oversight.

use crate::commands::global_context::GlobalContext;
use crate::core::state::build_state_snapshot;
use crate::operations::model::{
    changed_inputs, AppliedEffect, ConfirmationRequirement, EnvironmentField,
    EnvironmentFieldChange, EnvironmentFieldValue, EnvironmentProjection, ExecutionReceipt,
    OperationIntent, OperationKind, OperationOutcome, OperationPlan, PlanApplyError,
    PlanFingerprint, PlanWarning, PlannedEffect,
};
use crate::types::{Environment, HitchConfig};
use crate::utils::build_record::PinnedBranch;
use crate::utils::prelude::{access_metadata_read_only, modify_metadata};
use anyhow::Result;
use std::collections::BTreeSet;

const METADATA_REF: &str = "refs/heads/hitch-metadata";

/// The edit a `hitch set` resolves to, once the flags have been interpreted.
///
/// A struct rather than a set of parameters on the planner, for the same reason
/// [`crate::operations::declaration::DeclarationChange`] exists: it is a
/// *value*, so a test can build one, a future caller can pass one around, and
/// "which flags was this again" is a question the argument list answers. Every
/// field is `Option`/`Vec` so that a caller naming one flag is not also
/// asserting that the other six are absent — a `bool` would have made
/// `--add-approver` silently also mean "no removals, no base change", which is
/// how a flag ends up looking wired when it is not.
#[derive(serde::Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentSet {
    pub base: Option<String>,
    pub requires_approval: Option<bool>,
    pub min_approvals: Option<usize>,
    pub add_approver: Vec<String>,
    pub remove_approver: Vec<String>,
    pub set_approvers: Vec<String>,
    pub on_conflict: Option<crate::types::OnConflict>,
}

/// Create or destroy — the add/remove direction, mirroring
/// [`crate::operations::declaration::DeclarationChange`].
///
/// One enum and one planner for both, because the two are the same edit in
/// opposite directions and two planners would be two places to keep the same
/// three steps (resolve, refuse, apply) in agreement. The agreement is not
/// checked by any compiler.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentChange {
    /// Declare an environment that does not exist yet, on `base`.
    Create { base: String },
    /// Drop it from the declaration.
    Destroy,
}

impl EnvironmentChange {
    fn kind(&self) -> OperationKind {
        match self {
            EnvironmentChange::Create { .. } => OperationKind::AddEnvironment,
            EnvironmentChange::Destroy => OperationKind::RemoveEnvironment,
        }
    }
}

/// Which of the four edits a metadata plan is.
///
/// A discriminator the plan *states*, not one the executor infers from the
/// other fields, and the reason is a concrete pair of plans that are otherwise
/// indistinguishable: a `hitch unlock` carries no field changes, and so does a
/// `hitch set dev --min-approvals 1` where the threshold was already 1. An
/// executor that guessed from "are there changes?" would unlock on one and
/// write settings on the other, and both would be silent.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataEdit {
    /// A `hitch set`: apply the resolved [`EnvironmentFieldChange`] list.
    Settings,
    /// A `hitch lock`: set the lock, held by [`MetadataPlanDetail::locked_by`].
    Lock,
    /// A `hitch unlock`: clear it.
    Unlock,
    /// A `hitch add`: declare a new environment on
    /// [`MetadataPlanDetail::base`].
    Create,
    /// A `hitch remove`: drop it from the declaration.
    Destroy,
}

/// The metadata operation's plan detail.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct MetadataPlanDetail {
    pub environment: String,
    /// The user's original positional argument, for the remedy. Not the
    /// environment: `hitch remove dev` needs a remedy of `hitch remove dev`, and
    /// every metadata command is told the name directly, so the two are the same
    /// string today — but a command that later accepts an environment *name* as
    /// an alias would otherwise get a remedy quoting the wrong one.
    pub argument: String,
    pub edit: MetadataEdit,
    /// The resolved edit, in the order the executor applies it. Never the clap
    /// args — see the module header.
    pub changes: Vec<EnvironmentFieldChange>,
    /// The promoted branch this edit drops from the list because it made that
    /// branch the base.
    ///
    /// `Option` rather than a `Vec` of a one-variant enum, because it is
    /// singular: exactly one branch can equal the new base. A list would have
    /// been a shape with no second member, and a reader would have had to check
    /// whether emptiness meant "no collateral" or "a collateral kind not yet
    /// implemented".
    pub branch_absorbed_by_base: Option<String>,
    /// Who will hold the lock after a [`MetadataEdit::Lock`]. `None` for every
    /// other edit.
    pub locked_by: Option<String>,
    /// The base a [`MetadataEdit::Create`]d environment is declared on. `None`
    /// for every other edit.
    ///
    /// Redundant with the plan's `proposed` projection, and kept anyway: the
    /// executor must not have to reach into a projection to learn what it is
    /// about to write, and a field whose only reader is a second field is one
    /// refactor away from a second source of truth.
    pub base: Option<String>,
}

impl MetadataPlanDetail {
    /// Whether this plan's resolved edit writes anything at all.
    ///
    /// The three arms are `false` for genuinely different reasons and each one
    /// matters: a `set` with no changes is a no-op, a lock/unlock/add/remove is
    /// never one, and — the case that would otherwise be missed — a `set` whose
    /// only effect is absorbing a promoted branch into a new base *is* a change
    /// even though `changes` is empty. A check on `changes.is_empty()` alone
    /// would skip the write and silently leave the list too long.
    fn writes_anything(&self) -> bool {
        match self.edit {
            MetadataEdit::Settings => {
                !self.changes.is_empty() || self.branch_absorbed_by_base.is_some()
            }
            MetadataEdit::Lock
            | MetadataEdit::Unlock
            | MetadataEdit::Create
            | MetadataEdit::Destroy => true,
        }
    }
}

/// Plan a `hitch lock`.
pub fn plan_lock(
    context: &GlobalContext,
    environment: &str,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    plan_lock_change(context, environment, MetadataEdit::Lock)
}

/// Plan a `hitch unlock`.
pub fn plan_unlock(
    context: &GlobalContext,
    environment: &str,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    plan_lock_change(context, environment, MetadataEdit::Unlock)
}

/// The one implementation behind [`plan_lock`] and [`plan_unlock`].
///
/// A lock takes no [`ConfirmationRequirement`]. A lock is one keystroke, and
/// §10.2 of the spec is explicit that a prompt is not implied by showing a
/// plan: the plan is still *shown*, which is `decide_gate`'s `Proceed` arm, not
/// a new behaviour. A prompt here would be a change to `hitch lock` dressed up
/// as a consequence of introducing plans.
fn plan_lock_change(
    context: &GlobalContext,
    environment: &str,
    edit: MetadataEdit,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    let declared = read_environment(context, environment)?;
    let you = current_user_email(context);

    // The two refusals are opposite and both belong *here*, in the plan: a
    // refusal raised as `Err` never reaches the reader, so `hitch lock` on an
    // already-locked environment would print only the error with no plan above
    // it — the pre-P4 experience, and the one this programme exists to end.
    // Both are `PolicyRefusal` rather than two kinds, because no human action
    // *within this command* will help either way; what differs is the subject,
    // and the message carries it.
    let mut warnings = Vec::new();
    let holder = declared.locked_by.clone();
    // Every refusal below names its own remedy, and none of them is the command
    // the reader just ran. The default would be `command_hint` — `hitch lock
    // dev` for a lock that cannot happen because it is already locked — and
    // that is the one move guaranteed to produce the same refusal, printed
    // under the heading "To proceed:". The three refusals have three different
    // unblockers, and only this half knows which is which.
    match edit {
        MetadataEdit::Lock => {
            if declared.is_locked() {
                let who = holder.as_deref().unwrap_or("someone");
                warnings.push(
                    PlanWarning::policy_refusal(format!(
                        "Environment '{}' is already locked by '{}'",
                        environment, who
                    ))
                    .with_remedy(format!("hitch unlock {environment}")),
                );
            }
        }
        MetadataEdit::Unlock => {
            if !declared.is_locked() {
                // No unblocker, so no command. The environment is already in
                // the state the command was asked to put it in, and pointing
                // at the *other* lock flag would answer a question the reader
                // did not ask: they typed `unlock`, so `hitch lock dev` reads
                // as an instruction to do the opposite of what they wanted. A
                // remedy line is allowed to be a sentence for exactly this case
                // — see the stranger's refusal below.
                warnings.push(
                    PlanWarning::policy_refusal(format!(
                        "Environment '{}' is not currently locked",
                        environment
                    ))
                    .with_remedy(format!(
                        "nothing to undo — '{environment}' is already unlocked"
                    )),
                );
            } else if let Some(holder) = holder.as_deref() {
                if holder != you {
                    // No command helps here — the *other* user has to release
                    // the lock. A remedy is a command, so this one is a
                    // sentence, and the heading's "To proceed:" reads as
                    // correctly in front of it as in front of a command.
                    warnings.push(
                        PlanWarning::policy_refusal(format!(
                            "Environment '{}' is locked by '{}', not by you",
                            environment, holder
                        ))
                        .with_remedy(format!("ask {holder} to unlock it")),
                    );
                }
            }
        }
        MetadataEdit::Settings | MetadataEdit::Create | MetadataEdit::Destroy => {
            return Err(anyhow::anyhow!(
                "{:?} is not a lock change; the two entry points are the only callers",
                edit
            ))
        }
    }

    let blocked = warnings.iter().any(PlanWarning::is_blocking);
    // Both sides are `Some` and equal: a lock composes nothing and changes
    // nothing about what is composed. That is a different fact from `None` and
    // renders differently — see `core::render::render_plan`.
    let projection = EnvironmentProjection {
        environment: environment.to_string(),
        base: declared.base.clone(),
        branches: pin_declared(context, &declared)?,
        branch_sha: context
            .git()
            .rev_parse_opt(&format!("refs/heads/{}", environment))?,
    };

    let plan = OperationPlan {
        id: String::new(),
        kind: match edit {
            MetadataEdit::Lock => OperationKind::Lock,
            _ => OperationKind::Unlock,
        },
        intent: match edit {
            MetadataEdit::Lock => OperationIntent::LockEnvironment {
                environment: environment.to_string(),
            },
            _ => OperationIntent::UnlockEnvironment {
                environment: environment.to_string(),
            },
        },
        fingerprint: metadata_fingerprint(context)?,
        current: Some(projection.clone()),
        proposed: Some(projection),
        compositions: Vec::new(),
        // Empty for a blocked plan, because none of them will happen — the same
        // rule `plan_declaration_change` follows. A `Will change` row reading
        // "lock 'dev' held by …" directly above a refusal that it cannot lock
        // is a plan claiming an effect it has just said it will not have.
        effects: if blocked {
            Vec::new()
        } else {
            vec![PlannedEffect::MetadataChange {
                refname: METADATA_REF.to_string(),
                description: match edit {
                    MetadataEdit::Lock => format!("lock '{}' held by {}", environment, you),
                    _ => format!("release the lock on '{}'", environment),
                },
            }]
        },
        unaffected: Vec::new(),
        confirmation: ConfirmationRequirement::not_required(),
        warnings,
        detail: MetadataPlanDetail {
            environment: environment.to_string(),
            argument: environment.to_string(),
            edit,
            changes: Vec::new(),
            branch_absorbed_by_base: None,
            locked_by: match edit {
                MetadataEdit::Lock if !blocked => Some(you),
                _ => None,
            },
            base: None,
        },
    };
    with_id(context, plan)
}

/// Plan a `hitch set`: the resolved edit, and any refusal it raises.
pub fn plan_set_environment(
    context: &GlobalContext,
    environment: &str,
    requested: &EnvironmentSet,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    let declared = read_environment(context, environment)?;
    let after = resolve_set(&declared, requested);

    // The decision is the *difference*. `field_changes` produces one entry per
    // field that actually moved, so a no-op edit produces an empty list, and an
    // empty list is a plan that says "this will do nothing" — which is
    // `OperationOutcome::NoChange`, a successful outcome, rather than a refusal.
    let changes = field_changes(&declared, &after);

    // A new base that is also a promoted branch shortens the list. That happens
    // today with no word about it anywhere, which is why the planner names it:
    // the decision includes the collateral, so the plan does, and so does the
    // receipt.
    let branch_absorbed_by_base =
        Some(after.base.clone()).filter(|base| declared.branches.iter().any(|b| b == base));

    let mut warnings = Vec::new();
    // A decision the plan can make at plan time belongs in the plan, and this is
    // one: `validate_approval_config` is pure over an `Environment`, so running
    // it here means the reader sees the resolved edit *and* the reason it cannot
    // apply — rather than an error with no plan above it, which is what
    // `set.rs` produces today.
    if let Err(problem) = after.validate_approval_config() {
        // The default remedy would be `hitch set <env>` — this same command,
        // with the same flags, producing the same refusal. Every one of
        // `validate_approval_config`'s four failure modes is a disagreement
        // between `requires_approval`, `approvers` and `min_approvals`, and
        // three of the four are fixed outright by naming an approver, so that
        // is the flag the line points at. The fourth (a threshold above the
        // approver count) is also fixed by adding approvers, which is why the
        // same line serves all four rather than the message trying to name
        // which one fired — it cannot, and a remedy that guesses which field
        // to edit is a remedy the reader has to interpret.
        warnings.push(
            PlanWarning::policy_refusal(problem)
                .with_remedy(format!("hitch set {environment} --add-approver <email>")),
        );
    }
    // An approval request already pending against this environment will be
    // measured against the settings *this* edit changes, so a reader deserves
    // to know before they confirm — not after a request they did not mean to
    // move is auto-approved by a threshold they just raised.
    if !changes.is_empty() && has_pending_request(context, environment) {
        warnings.push(PlanWarning::advisory(format!(
            "Environment '{}' has a pending approval request, which will be \
             measured against these settings. Review it first:\n  \
             hitch approvals list --status pending",
            environment
        )));
    }

    let blocked = warnings.iter().any(PlanWarning::is_blocking);
    // A blocked plan proposes nothing that will not happen, so its proposed side
    // is its current one. The same rule `plan_declaration_change` follows, and
    // for the same reason: a plan describing an outcome it will not reach is the
    // lie this architecture exists to prevent.
    let effective = if blocked { &declared } else { &after };
    let branch_sha = context
        .git()
        .rev_parse_opt(&format!("refs/heads/{}", environment))?;
    let projected = |branches: Vec<PinnedBranch>| EnvironmentProjection {
        environment: environment.to_string(),
        base: effective.base.clone(),
        branches,
        branch_sha: branch_sha.clone(),
    };
    let current_branches = pin_declared(context, &declared)?;
    let proposed_branches = if blocked {
        current_branches.clone()
    } else {
        pin_declared(context, &after)?
    };

    let plan = OperationPlan {
        id: String::new(),
        kind: OperationKind::SetEnvironment,
        intent: OperationIntent::SetEnvironment {
            environment: environment.to_string(),
            changes: changes.clone(),
        },
        fingerprint: metadata_fingerprint(context)?,
        current: Some(projected(current_branches)),
        proposed: Some(projected(proposed_branches)),
        compositions: Vec::new(),
        // Empty for a blocked plan, because none of them will happen.
        effects: if blocked {
            Vec::new()
        } else {
            vec![PlannedEffect::MetadataChange {
                refname: METADATA_REF.to_string(),
                description: describe_set(environment, &changes),
            }]
        },
        unaffected: Vec::new(),
        confirmation: ConfirmationRequirement::not_required(),
        warnings,
        detail: MetadataPlanDetail {
            environment: environment.to_string(),
            argument: environment.to_string(),
            edit: MetadataEdit::Settings,
            changes,
            branch_absorbed_by_base,
            locked_by: None,
            base: None,
        },
    };
    with_id(context, plan)
}

/// Plan a `hitch add`.
pub fn plan_add_environment(
    context: &GlobalContext,
    environment: &str,
    base: Option<&str>,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    let change = EnvironmentChange::Create {
        base: base.unwrap_or("main").to_string(),
    };
    plan_create_or_destroy(context, environment, &change)
}

/// Plan a `hitch remove`.
pub fn plan_remove_environment(
    context: &GlobalContext,
    environment: &str,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    plan_create_or_destroy(context, environment, &EnvironmentChange::Destroy)
}

fn plan_create_or_destroy(
    context: &GlobalContext,
    environment: &str,
    change: &EnvironmentChange,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let existing = config.get_environment(environment).cloned();
    let mut warnings = Vec::new();

    let (current, proposed) = match change {
        EnvironmentChange::Create { base } => {
            if existing.is_some() {
                warnings.push(PlanWarning::policy_refusal(format!(
                    "Environment '{}' already exists in the configuration",
                    environment
                )));
            }
            (
                // There is no `qa` to project before it exists, and an empty
                // projection would be a statement about a thing that is not
                // there.
                None,
                Some(EnvironmentProjection {
                    environment: environment.to_string(),
                    base: base.clone(),
                    branches: Vec::new(),
                    branch_sha: None,
                }),
            )
        }
        EnvironmentChange::Destroy => {
            // A missing environment stays an `Err`. The refusals that are *not*
            // about confirmation are a user's mistake in naming, and there is no
            // plan worth drawing for one; the two that *are* about confirmation
            // become warnings below, so the reader sees the plan and the reason
            // it needs `--force`.
            let Some(declared) = existing.as_ref() else {
                return Err(anyhow::anyhow!(
                    "Environment '{}' not found in configuration",
                    environment
                ));
            };
            if declared.is_locked() {
                warnings.push(PlanWarning::policy_refusal(format!(
                    "Environment '{}' is currently locked by '{}'",
                    environment,
                    declared.locked_by.as_deref().unwrap_or("someone")
                )));
            }
            if !declared.branches.is_empty() {
                warnings.push(PlanWarning::advisory(format!(
                    "Environment '{}' still has {} promoted branch{}; removing it \
                     drops them from every pipeline that names it",
                    environment,
                    declared.branches.len(),
                    if declared.branches.len() == 1 {
                        ""
                    } else {
                        "es"
                    }
                )));
            }
            (
                Some(EnvironmentProjection {
                    environment: environment.to_string(),
                    base: declared.base.clone(),
                    branches: pin_declared(context, declared)?,
                    branch_sha: context
                        .git()
                        .rev_parse_opt(&format!("refs/heads/{}", environment))?,
                }),
                // And afterwards there is no composition left to describe.
                None,
            )
        }
    };

    let blocked = warnings.iter().any(PlanWarning::is_blocking);
    // Promoted branches, not confirmation, are what needs a yes/no here — and
    // only in the case that has them. A `hitch remove` of an empty environment
    // is one keystroke like a lock, and gating it would be a new prompt
    // introduced by a refactor. `--force` clears this at the call site
    // (deviation 3).
    let confirmation = match change {
        EnvironmentChange::Destroy
            if !existing
                .as_ref()
                .is_none_or(|declared| declared.branches.is_empty()) =>
        {
            ConfirmationRequirement::required(
                "this removes promoted branches from the pipeline".to_string(),
            )
        }
        _ => ConfirmationRequirement::not_required(),
    };

    let plan = OperationPlan {
        id: String::new(),
        kind: change.kind(),
        intent: match change {
            EnvironmentChange::Create { base } => OperationIntent::AddEnvironment {
                environment: environment.to_string(),
                base: base.clone(),
            },
            EnvironmentChange::Destroy => OperationIntent::RemoveEnvironment {
                environment: environment.to_string(),
            },
        },
        fingerprint: metadata_fingerprint(context)?,
        current,
        proposed,
        compositions: Vec::new(),
        // Empty for a blocked plan, because none of them will happen.
        effects: if blocked {
            Vec::new()
        } else {
            vec![PlannedEffect::MetadataChange {
                refname: METADATA_REF.to_string(),
                description: match change {
                    EnvironmentChange::Create { base } => {
                        format!("declare environment '{}' on base {}", environment, base)
                    }
                    EnvironmentChange::Destroy => {
                        format!("remove environment '{}' from the declaration", environment)
                    }
                },
            }]
        },
        unaffected: Vec::new(),
        confirmation: if blocked {
            ConfirmationRequirement::not_required()
        } else {
            confirmation
        },
        warnings,
        detail: MetadataPlanDetail {
            environment: environment.to_string(),
            argument: environment.to_string(),
            edit: match change {
                EnvironmentChange::Create { .. } => MetadataEdit::Create,
                EnvironmentChange::Destroy => MetadataEdit::Destroy,
            },
            changes: Vec::new(),
            branch_absorbed_by_base: None,
            locked_by: None,
            base: match change {
                EnvironmentChange::Create { base } => Some(base.clone()),
                EnvironmentChange::Destroy => None,
            },
        },
    };
    with_id(context, plan)
}

/// Refuse a plan whose declaration has moved under it.
///
/// Deliberately *not* a call to `operations::rebuild::validate_plan`: that is
/// typed to `RebuildPlanDetail`, and its resolution arm — "a recorded resolution
/// has disappeared" — has no meaning for a metadata edit, so reusing it would
/// put a dead loop in the path of every `hitch lock`. If this body is ever a
/// third copy, extract it then; three is the point at which the duplication
/// earns a name, and two is not.
pub fn validate_metadata_plan(
    context: &GlobalContext,
    plan: &OperationPlan<MetadataPlanDetail>,
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

/// Apply a metadata plan and report what happened.
///
/// One [`modify_metadata`] closure, applying the resolved edit **verbatim** —
/// the same rule `apply_declaration_plan` follows, and for the same reason:
/// re-deriving the edit here would be a second decision point, and the whole
/// reason the plan carries the edit is that it is decided.
///
/// The closure returns `Err` and commits nothing if the repository disagrees
/// with the plan, which is why there is no rollback anywhere in this file. The
/// environment is re-read *after* the transaction, not inside the closure —
/// the closure runs before the write (see the module header), so reading it
/// back there would return the pre-edit declaration, which is the bug
/// `approvals/approve.rs` had for a year.
pub fn apply_metadata_plan(
    context: &GlobalContext,
    plan: &OperationPlan<MetadataPlanDetail>,
    _on_step: &mut dyn FnMut(&str),
) -> Result<ExecutionReceipt> {
    let started_at = chrono::Utc::now();
    validate_metadata_plan(context, plan).map_err(PlanApplyError::into_anyhow)?;

    if let Some(blocking) = plan.blocked_by() {
        return Err(PlanApplyError::PolicyBlocked {
            environment: plan.detail.environment.clone(),
            reason: blocking.message.clone(),
            // The planner's remedy, not `command_hint`. Every refusal in this
            // file names one, because every refusal here has an unblocker that
            // is *not* this command — re-running the command that just refused
            // is the one move guaranteed to refuse again.
            remedy: blocking
                .remedy_or(
                    &plan
                        .kind
                        .command_hint(&plan.detail.environment, &plan.detail.argument),
                )
                .to_string(),
        }
        .into_anyhow());
    }

    let detail = plan.detail.clone();
    if !detail.writes_anything() {
        // A `set` whose resolved edit is empty. Nothing to write, and a
        // `modify_metadata` call with a no-op closure would spend a commit to
        // record that hitch did nothing — which would then make the *next* plan
        // stale for a reason no reader could see.
        return Ok(ExecutionReceipt {
            plan_id: plan.id.clone(),
            operation: plan.kind,
            started_at,
            completed_at: chrono::Utc::now(),
            outcome: OperationOutcome::NoChange,
            effects: Vec::new(),
            warnings: Vec::new(),
            resulting_state: build_state_snapshot(context).ok(),
        });
    }

    modify_metadata(context, |config| apply_edit(config, &detail))?;

    // Read back rather than assert from the plan: a receipt is a *record*, and a
    // predicted value and an observed one are different claims.
    let after = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let observed = after.get_environment(&detail.environment);

    Ok(ExecutionReceipt {
        plan_id: plan.id.clone(),
        operation: plan.kind,
        started_at,
        completed_at: chrono::Utc::now(),
        outcome: OperationOutcome::Applied,
        effects: vec![AppliedEffect::MetadataChange {
            refname: METADATA_REF.to_string(),
            description: describe_applied(plan, observed),
        }],
        warnings: Vec::new(),
        resulting_state: build_state_snapshot(context).ok(),
    })
}

/// Write the resolved edit into the configuration.
///
/// Verbatim from the plan, in the plan's order, and it re-checks nothing: the
/// plan is the decision, and [`validate_metadata_plan`] is what decides whether
/// that decision is still current. A validation call here would be a third
/// opinion, arriving after the last point where it could have mattered.
fn apply_edit(config: &mut HitchConfig, detail: &MetadataPlanDetail) -> Result<()> {
    let environment = detail.environment.as_str();
    match detail.edit {
        MetadataEdit::Destroy => {
            // Nothing to look up: the environment *is* the thing being removed,
            // and `remove_environment` is unconditional. An `ok_or_else` here
            // would turn a `hitch remove` of an environment that vanished
            // between the plan and the apply into an error instead of a
            // success — and the plan's staleness check is what should have
            // caught the window, not a second opinion inside the write.
            config.remove_environment(environment);
            return Ok(());
        }
        MetadataEdit::Create => {
            let base = detail.base.clone().ok_or_else(|| {
                anyhow::anyhow!("internal: a create plan with no base; the planner always sets one")
            })?;
            // `add_environment` validates the whole config with the addition
            // applied, which is the check `hitch add` has always relied on and
            // the reason a duplicate name is a validation error rather than a
            // silent overwrite.
            config
                .add_environment(environment.to_string(), Environment::new(base))
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            return Ok(());
        }
        MetadataEdit::Lock | MetadataEdit::Unlock | MetadataEdit::Settings => {}
    }

    let env = config.get_environment_mut(environment).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' not found in hitch configuration",
            environment
        )
    })?;

    match detail.edit {
        MetadataEdit::Lock => {
            let holder = detail.locked_by.clone().ok_or_else(|| {
                anyhow::anyhow!("internal: a lock plan with no holder; the planner resolves one")
            })?;
            env.lock(holder);
        }
        MetadataEdit::Unlock => env.unlock(),
        MetadataEdit::Settings => {
            for change in &detail.changes {
                match (change.field, &change.new) {
                    (EnvironmentField::Base, EnvironmentFieldValue::Branch(value)) => {
                        env.base = value.clone()
                    }
                    (
                        EnvironmentField::OnConflict,
                        EnvironmentFieldValue::ConflictPolicy(value),
                    ) => env.on_conflict = *value,
                    (EnvironmentField::RequiresApproval, EnvironmentFieldValue::Flag(value)) => {
                        env.requires_approval = *value
                    }
                    (EnvironmentField::MinApprovals, EnvironmentFieldValue::Count(value)) => {
                        env.min_approvals = *value
                    }
                    (EnvironmentField::Approvers, EnvironmentFieldValue::Addresses(value)) => {
                        env.approvers = value.clone()
                    }
                    // Total by construction: `field_changes` only ever pairs a
                    // field with a value of that field's type. A planner bug is
                    // better as an error than as a silently-skipped field — a
                    // skipped field is a plan that promises a change the apply
                    // does not make.
                    (other, new) => {
                        return Err(anyhow::anyhow!(
                            "internal: field {:?} planned with value {:?}",
                            other,
                            new
                        ))
                    }
                }
            }
            if let Some(absorbed) = &detail.branch_absorbed_by_base {
                env.remove_branch(absorbed);
            }
        }
        // Handled above, before the environment lookup.
        MetadataEdit::Create | MetadataEdit::Destroy => {
            unreachable!("create and destroy return before the lookup; the match above is total")
        }
    }
    Ok(())
}

fn read_environment(context: &GlobalContext, environment: &str) -> Result<Environment> {
    access_metadata_read_only(context, |config| {
        config.get_environment(environment).cloned().ok_or_else(|| {
            anyhow::anyhow!("Environment '{}' not found in configuration", environment)
        })
    })
}

/// Fill in a plan's `id` from its fingerprint, and return it.
///
/// The same `<kind>:<environment>:<argument>:<digest>` shape `plan_declaration_change`
/// builds, so a log line naming a plan reads the same whatever built it — an id
/// that is only a digest identifies *which* plan but not *what* it was.
///
/// Propagating the digest's error rather than swallowing it is the other half.
/// A plan whose fingerprint cannot be hashed has a claim that can never be
/// checked, and `unwrap_or_default` would have handed it an empty id that
/// correlates with every other empty-id plan — which is worse than refusing,
/// because the refusal is visible.
fn with_id(
    context: &GlobalContext,
    mut plan: OperationPlan<MetadataPlanDetail>,
) -> Result<OperationPlan<MetadataPlanDetail>> {
    let digest = plan.fingerprint.digest(context.git())?;
    plan.id = format!(
        "{}:{}:{}:{}",
        plan.kind, plan.detail.environment, plan.detail.argument, digest
    );
    Ok(plan)
}

/// The one dependency a metadata plan has.
fn metadata_fingerprint(context: &GlobalContext) -> Result<PlanFingerprint> {
    let mut fingerprint = PlanFingerprint::new();
    fingerprint.metadata_sha = context.git().rev_parse_opt(METADATA_REF)?;
    Ok(fingerprint)
}

fn current_user_email(context: &GlobalContext) -> String {
    context
        .git()
        .get_user_email()
        .unwrap_or_else(|_| "you".to_string())
}

/// Pin each declared branch to the SHA this plan consumed, in declaration
/// order.
///
/// Order is composition order and is load-bearing, so this never sorts. A
/// branch that cannot be resolved is *omitted* rather than erroring: the plan
/// is describing a declaration edit, and a branch that does not exist is the
/// build's problem to report, not a reason `hitch set` should refuse.
fn pin_declared(context: &GlobalContext, declared: &Environment) -> Result<Vec<PinnedBranch>> {
    let git = context.git();
    let mut pinned = Vec::new();
    for name in &declared.branches {
        if let Some(sha) = git.rev_parse_opt(&format!("refs/heads/{}", name))? {
            pinned.push(PinnedBranch {
                branch: name.clone(),
                sha,
            });
        }
    }
    Ok(pinned)
}

/// Fold the requested flags into a copy of the declaration.
///
/// The order is the one `hitch set` has always used, and it is observable: a
/// `--set-approvers` combined with `--add-approver` sets and then adds, so the
/// add wins. Changing it would be a behaviour change dressed as a refactor.
fn resolve_set(declared: &Environment, requested: &EnvironmentSet) -> Environment {
    let mut after = declared.clone();
    if let Some(base) = requested.base.as_ref() {
        after.base = base.clone();
    }
    if let Some(flag) = requested.requires_approval {
        after.requires_approval = flag;
    }
    if let Some(count) = requested.min_approvals {
        after.min_approvals = count;
    }
    for approver in &requested.add_approver {
        // Fold, not append: `--add-approver` for someone who is already an
        // approver is a no-op, and if it were an append the same person would
        // appear twice and count twice towards a threshold.
        if !after.approvers.contains(approver) {
            after.approvers.push(approver.clone());
        }
    }
    for approver in &requested.remove_approver {
        after.approvers.retain(|a| a != approver);
    }
    if !requested.set_approvers.is_empty() {
        after.approvers = requested.set_approvers.clone();
    }
    if let Some(policy) = requested.on_conflict {
        after.on_conflict = policy;
    }
    // The threshold raise, in the same place `hitch set` has always had it:
    // approval enabled with a threshold of zero would fail validation, and the
    // fix is a one rather than an error. The difference is that it is now a
    // *change in the plan* rather than a surprise in the commit.
    if after.requires_approval && after.min_approvals == 0 {
        after.min_approvals = 1;
    }
    // A base that is also a promoted branch is declared twice, and merging it
    // into itself is at best a no-op, so the list loses it. Also what `hitch
    // set` has always done; the difference is that the plan now names it.
    after.remove_branch(&after.base.clone());
    after
}

/// The difference between two declarations, one entry per field, in the order
/// the executor applies them.
///
/// One entry per *field* rather than one per flag, and that is the point:
/// `--add-approver a --add-approver b` against an empty list is one approver
/// change, not two, and a plan listing it twice would be a document about an
/// operation that does not exist. A field absent from the list did not change;
/// there is no "set to the same value" entry, because that is not a change.
fn field_changes(before: &Environment, after: &Environment) -> Vec<EnvironmentFieldChange> {
    let mut changes = Vec::new();
    fn push(
        changes: &mut Vec<EnvironmentFieldChange>,
        field: EnvironmentField,
        old: EnvironmentFieldValue,
        new: EnvironmentFieldValue,
    ) {
        if old != new {
            changes.push(EnvironmentFieldChange { field, old, new });
        }
    }

    push(
        &mut changes,
        EnvironmentField::Base,
        EnvironmentFieldValue::Branch(before.base.clone()),
        EnvironmentFieldValue::Branch(after.base.clone()),
    );
    if before.on_conflict != after.on_conflict {
        changes.push(EnvironmentFieldChange {
            field: EnvironmentField::OnConflict,
            old: EnvironmentFieldValue::ConflictPolicy(before.on_conflict),
            new: EnvironmentFieldValue::ConflictPolicy(after.on_conflict),
        });
    }
    push(
        &mut changes,
        EnvironmentField::RequiresApproval,
        EnvironmentFieldValue::Flag(before.requires_approval),
        EnvironmentFieldValue::Flag(after.requires_approval),
    );
    push(
        &mut changes,
        EnvironmentField::MinApprovals,
        EnvironmentFieldValue::Count(before.min_approvals),
        EnvironmentFieldValue::Count(after.min_approvals),
    );
    // Compared as a *set*, not a list, because the approver list has no order:
    // `--add-approver b --add-approver a` and the reverse resolve to the same
    // declaration, and a plan that called the second a change would be
    // describing a difference that does not exist. The order still survives into
    // the change's `new`, which is what the executor writes — so the written
    // value is the user's order even though the change itself is order-free.
    let before_set: BTreeSet<&String> = before.approvers.iter().collect();
    let after_set: BTreeSet<&String> = after.approvers.iter().collect();
    if before_set != after_set {
        changes.push(EnvironmentFieldChange {
            field: EnvironmentField::Approvers,
            old: EnvironmentFieldValue::Addresses(before.approvers.clone()),
            new: EnvironmentFieldValue::Addresses(after.approvers.clone()),
        });
    }
    changes
}

/// Whether an approval request is open against this environment.
///
/// A *read* of the declaration, so it participates in no fingerprint: the
/// declaration SHA already covers it, since a request lives on
/// `hitch-metadata` too.
fn has_pending_request(context: &GlobalContext, environment: &str) -> bool {
    access_metadata_read_only(context, |config| {
        Ok(crate::utils::approvals::get_approval_requests(
            config,
            Some(environment),
            Some(crate::types::ApprovalStatus::Pending),
        )
        .is_empty())
    })
    .map(|is_empty| !is_empty)
    .unwrap_or(false)
}

/// The plan's one-line description of the edit it resolved.
///
/// The *words* are the renderer's job (`core::render`); this is the operation
/// naming itself, in the same way `apply_declaration_plan` does and for the same
/// reason: there is no structure here for a renderer to work from, so a plan
/// that carried one would be a model holding prose.
fn describe_set(environment: &str, changes: &[EnvironmentFieldChange]) -> String {
    if changes.is_empty() {
        return format!("no settings change to '{}'", environment);
    }
    format!(
        "update {} of '{}'",
        changed_fields(changes).join(", "),
        environment
    )
}

/// The receipt's line, read back from the declaration rather than copied from
/// the plan.
///
/// The lock holder is the one case where they can differ — a plan resolves the
/// email at plan time and the declaration records whatever the write produced —
/// and a receipt is a record, so the record's value is the one that goes in.
fn describe_applied(
    plan: &OperationPlan<MetadataPlanDetail>,
    observed: Option<&Environment>,
) -> String {
    let environment = &plan.detail.environment;
    match plan.detail.edit {
        MetadataEdit::Lock => format!(
            "lock '{}' held by {}",
            environment,
            observed
                .and_then(|e| e.locked_by.clone())
                .unwrap_or_else(|| "you".to_string())
        ),
        MetadataEdit::Unlock => format!("release the lock on '{}'", environment),
        MetadataEdit::Create => {
            let base = observed.map(|e| e.base.as_str()).unwrap_or("");
            format!("declare environment '{}' on base {}", environment, base)
        }
        MetadataEdit::Destroy => {
            format!("remove environment '{}' from the declaration", environment)
        }
        MetadataEdit::Settings => {
            if plan.detail.changes.is_empty() {
                return format!("no settings change to '{}'", environment);
            }
            format!(
                "update {} of '{}'",
                changed_fields(&plan.detail.changes).join(", "),
                environment
            )
        }
    }
}

fn changed_fields(changes: &[EnvironmentFieldChange]) -> Vec<&'static str> {
    changes
        .iter()
        .map(|c| match c.field {
            EnvironmentField::Base => "base",
            EnvironmentField::OnConflict => "on-conflict",
            EnvironmentField::RequiresApproval => "approval requirement",
            EnvironmentField::MinApprovals => "approval threshold",
            EnvironmentField::Approvers => "approvers",
        })
        .collect()
}
