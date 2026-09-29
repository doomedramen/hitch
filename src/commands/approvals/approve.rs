use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, decide_gate, emit_receipt, render_plan, GateDecision};
use crate::operations::declaration::{
    apply_declaration_plan, plan_approved_declaration_change, DeclarationChange,
    DeclarationPlanOptions,
};
use crate::types::HitchConfig;
use crate::utils::prelude::{modify_metadata, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ApproveArgs {
    /// Approval request ID
    pub request_id: String,

    /// Approval comment (optional)
    #[arg(long)]
    pub comment: Option<String>,
}

pub fn run(args: ApproveArgs, context: &GlobalContext) -> Result<()> {
    use crate::types::ApprovalStatus;

    // The gate's refusal comes first, ahead of the vote's own write: the apply
    // that follows a met threshold reaches the same gate, and by then the vote
    // is committed. Refusing here keeps "exit 1" meaning "nothing happened".
    if let GateDecision::Refuse(reason) = decide_gate(context.assume_yes, context.json, true) {
        anyhow::bail!(reason);
    }

    // Step 1: pre_check - Ensure git repository is in good state
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Resolve the request (accepts a full ID or an unambiguous prefix) and
    // branch on its current status.
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
    let request_id = request.id.clone();
    let environment_name = request.environment.clone();

    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &environment_name)?;
    let min_approvals = request.required_approvals(&environment);

    match request.status {
        ApprovalStatus::Applied => {
            return Err(anyhow::anyhow!(
                "Request {} has already been applied — nothing to do.",
                request_id
            ));
        }
        ApprovalStatus::Rejected => {
            return Err(anyhow::anyhow!(
                "Request {} was rejected and can no longer be approved.",
                request_id
            ));
        }
        ApprovalStatus::Cancelled => {
            return Err(anyhow::anyhow!(
                "Request {} was cancelled and can no longer be approved.",
                request_id
            ));
        }
        ApprovalStatus::Approved => {
            // The threshold was already met but the operation was never applied
            // (e.g. a previous apply was interrupted). Re-drive execution so an
            // approved request isn't a dead end.
            // No vote is recorded on this path, which is the one fact the plan
            // and receipt cannot say.
            context.log_info("Request already meets its approval threshold; applying it now.");
            execute_approved_operation(context, &request_id, &environment_name)?;
            return Ok(());
        }
        ApprovalStatus::Pending => { /* normal approval flow below */ }
    }

    // Step 3: Record this approval (validation happens under the lock).
    let request_details = validate_and_approve(context, &args, &request_id, &environment_name)?;

    // Step 4: If the (frozen) threshold is now met, execute the operation.
    //
    // "Executing …" used to be printed here, one line above the plan that
    // followed it. It said the same thing as that plan — a promotion is a
    // promotion — so it was a second voice for the same operation, and the only
    // thing it added was the word "Executing", which the plan's own heading and
    // the receipt's "Applied" section both carry. The approval's *own* narration
    // below is kept, because the request's transition from Pending to Applied is
    // a fact about the approval rather than about the declaration, and nothing
    // else in the document says it.
    //
    // Once the threshold is met the plan and the receipt are the whole account,
    // so nothing is narrated around them. Only the vote that did *not* reach the
    // threshold has no plan, and gets one line.
    if request_details.threshold_met(min_approvals) {
        execute_approved_operation(context, &request_id, &environment_name)?;
    } else {
        let have = request_details.approvals.len();
        let remaining =
            crate::utils::authorization::get_remaining_approvers(&environment, &request_details);
        context.log_info(&format!(
            "Approval recorded ({have}/{min_approvals}). Waiting for {} more approval(s) from:\n{}",
            min_approvals.saturating_sub(have),
            remaining
                .iter()
                .map(|a| format!("  - {a}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    Ok(())
}

/// Record the current user's approval for a Pending request.
///
/// All authorization and freshness checks (approver membership, no self-approval,
/// no double-approval, status == Pending, snapshot unchanged) are performed on the
/// locked, freshly-read config inside `approve_request`/`validate_snapshot` — this
/// function deliberately does NOT re-check them on a stale pre-lock clone.
///
/// **No rollback, and that is the whole change.** There used to be one:
/// `attempt_approval_rollback` restored a snapshot of the environment taken at
/// the start of the closure, on the error path. But a closure `Err` means
/// `modify_metadata` never committed anything, so the snapshot described a
/// repository this operation had not modified — and restoring it cost an extra
/// `hitch-metadata` commit on every refused approval, writing back a config
/// identical to the one already there. The same defect the promote/demote path
/// carried, found here by the same reasoning: a repair narrated for something
/// that was never broken is paid for in commits, and this branch's history is
/// part of the deployment pipeline.
///
/// What a refusal still costs is two commits — the lock and its unlock, because
/// `with_locked_env` commits the lock before the closure and the unlock on the
/// way out. Those are the visible-lock signal the crash-recovery tests read, and
/// they are not tidied away.
fn validate_and_approve(
    context: &GlobalContext,
    args: &ApproveArgs,
    request_id: &str,
    environment_name: &str,
) -> Result<crate::types::ApprovalRequest> {
    with_locked_env(context, environment_name, || {
        modify_metadata(context, |config: &mut HitchConfig| {
            // Validate snapshot freshness on the locked, freshly-read request
            // before recording an approval that could never execute.
            let snapshot = crate::utils::approvals::find_approval_request(config, request_id)?
                .rebuild_snapshot
                .clone();
            crate::utils::snapshot::validate_snapshot(context, &snapshot)?;

            // Record the approval. This performs authorization on the fresh config.
            crate::utils::approvals::approve_request(
                context,
                config,
                request_id,
                args.comment.clone(),
            )?;
            Ok(())
        })
    })?;

    crate::utils::prelude::get_approval_request_by_id(context, request_id)
}

/// Apply the declaration change a met approval threshold authorised.
///
/// The ordinary plan → gate → apply → receipt shape, through the same
/// [`plan_declaration_change`] promote and demote use. The difference is the
/// *authorisation*, not the mechanism: the approval is the gate, so
/// `DeclarationChange::ApprovedApply` is what keeps this planner from asking
/// again, and the `ApprovalStatus::Approved` re-drive arm is what makes a
/// partially applied approval safe to retry.
///
/// **The order is load-bearing, and it is the reverse of the order this used to
/// use.** The old sequence marked the request `Applied` in the *same*
/// `modify_metadata` transaction as the declaration edit, and then ran a rebuild.
/// That has no plan to show and no receipt to print, because the edit had already
/// happened by the time anything could describe it — and it is why this command
/// was the last one in the CLI narrating a nested `StepLogger` transcript in the
/// gap where a plan and a receipt belong.
///
/// Two orders are possible here and only one of them survives a crash:
///
/// - *Mark applied first.* Then a process that dies before the edit leaves the
///   request `Applied` with its own change missing — and `run` refuses an
///   `Applied` request outright, so there is no command that finishes the job.
///   A self-inflicted wedge with no way out.
/// - *Mark applied last* (this one). Then a crash anywhere before the final
///   write leaves the request `Approved`, and `hitch approve <id>` re-drives it:
///   the re-run's plan reports `NoChange` because the branch is already declared,
///   and it still applies, so the rebuild — the part that was owed — happens.
///   Idempotent and self-healing.
///
/// Planning inside the lock is forced rather than chosen, for the same reason
/// promote's is: `with_locked_env` commits the lock to `hitch-metadata` *before*
/// running its closure, and `PlanFingerprint` carries `metadata_sha`, so a plan
/// built outside would refuse itself the moment it was validated.
fn execute_approved_operation(
    context: &GlobalContext,
    request_id: &str,
    environment_name: &str,
) -> Result<bool> {
    context.log_verbose(&format!(
        "Executing operation for approved request {}",
        request_id
    ));

    let request = crate::utils::prelude::get_approval_request_by_id(context, request_id)?;
    let change = DeclarationChange::ApprovedApply {
        operation: request.operation,
        branches: vec![request.branch.clone()],
        request_id: request_id.to_string(),
    };

    let (plan, receipt) = with_locked_env(context, environment_name, || {
        // The request's own claim about the world it was filed against, checked
        // on the freshly-read request rather than the pre-lock clone. This is the
        // *request's* freshness — "what you reviewed has not changed" — and it is
        // a different question from the plan's, which asks whether this apply is
        // still current. It used to live inside the `modify_metadata` closure,
        // where it was a pre-check ahead of a write that is no longer there.
        let snapshot = crate::utils::prelude::get_approval_request_by_id(context, request_id)?
            .rebuild_snapshot;
        crate::utils::snapshot::validate_snapshot(context, &snapshot)?;

        let plan = plan_approved_declaration_change(
            context,
            environment_name,
            change,
            request_id,
            DeclarationPlanOptions { no_rebuild: false },
            &mut |_| {},
        )?;

        if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
            return Ok((plan, None));
        }

        // The declaration edit and the nested rebuild, as one operation. The
        // rebuild's own plan and receipt are nested inside this one and are
        // deliberately not printed: the receipt's
        // `DependentEnvironmentRebuild` effect is what reports it, including any
        // branches it held and any work it still owes.
        let receipt = apply_declaration_plan(context, &plan, &mut |_| {})?;

        // Last, and only because it is the only ordering a crash survives: the
        // request is `Applied` once its change is real, not before.
        modify_metadata(context, |config| {
            crate::utils::approvals::mark_request_applied(config, request_id)
        })?;

        Ok((plan, Some(receipt)))
    })?;

    match receipt {
        Some(receipt) => {
            emit_receipt(context, &plan, &receipt)?;
            Ok(true)
        }
        // Declined at the gate. The plan and the answer were already shown, the
        // request stays `Approved` so the re-drive arm can resume, and nothing
        // was written. Exit 0: declining is not a failure.
        None => Ok(false),
    }
}
