use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::declaration::{
    apply_declaration_plan, apply_may_write, plan_demote, DeclarationPlanOptions,
};
use crate::operations::model::OperationOutcome;
use crate::types::{RollbackInfo, RollbackOperation};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct DemoteCommand {
    /// The branch to demote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to demote the branch from
    #[arg()]
    pub env_name: String,

    /// Skip the automatic rebuild after demotion.
    /// Use this to batch multiple demotes and then run 'hitch rebuild <env>' once.
    #[arg(long)]
    pub no_rebuild: bool,

    /// Show the plan — what would be un-declared, what would be rebuilt, which
    /// environments would follow — and stop. Changes nothing: no stash, no
    /// lock, no writes.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: DemoteCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Ensure we are in a Git repository
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Step 2: The environment must exist, and a lock set by a *human* is a refusal; the lock this command takes
    // for itself is not. Checked before `with_locked_env`, which would otherwise
    // see its own lock. See `promote::run` for the same shape.
    // Checked here, not in the planner: the planner runs inside the lock, and
    // `with_locked_env`'s own first act is to lock the environment — which
    // reports a missing one as "not found" rather than "does not exist".
    crate::utils::command_helpers::ensure_environment_exists(context, &args.env_name)?;
    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;
    if config
        .environments
        .get(&args.env_name)
        .is_some_and(|e| e.is_locked())
    {
        anyhow::bail!(
            "Environment '{}' is currently locked by '{}'",
            args.env_name,
            crate::utils::command_helpers::environment::get_locked_by_user(
                context,
                &args.env_name
            )?
        );
    }

    let options = DeclarationPlanOptions {
        no_rebuild: args.no_rebuild,
    };

    if args.dry_run {
        // Preview, planned outside the lock — see `promote::run` for why that
        // inversion is safe, and why it would stop being safe if the planner
        // ever consulted `is_locked()`.
        let plan = plan_demote(context, &args.branch, &args.env_name, options)?;
        emit_plan(context, &plan)?;
        return Ok(());
    }

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Demote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Plan, then apply, both under the environment lock. Planning inside
    // the lock is forced, not stylistic: `with_locked_env` commits the lock to
    // `hitch-metadata` first, so a plan built outside it is stale on arrival.
    //
    // The snapshot is captured before the lock and armed inside it — see
    // `promote::run` for why both halves of that are load-bearing.
    let snapshot = crate::utils::rollback::capture_config_state(context)?;
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            let plan = plan_demote(context, &args.branch, &args.env_name, options)?;

            if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
                return Ok((plan, None));
            }
            if apply_may_write(&plan) {
                rollback_info.previous_config = snapshot;
            }
            let receipt = apply_declaration_plan(context, &plan)?;
            Ok((plan, Some(receipt)))
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok((plan, Some(receipt))) => {
            emit_receipt(context, &plan, &receipt)?;
            if receipt.outcome == OperationOutcome::ApprovalRequested {
                // Printed by the command, not the planner — see `promote::run`.
                context.log_info(
                    "Run 'hitch approvals list' to see the request, then 'hitch approvals approve <id>' to grant it.",
                );
            }
            Ok(())
        }
        // Declined: nothing was written, so nothing to roll back. See
        // `promote::run`.
        Ok((_plan, None)) => Ok(()),
        Err(e) => {
            // The cause is reported once, by `main` — see `promote::run`. This
            // arm used to print it as `❌ Error: …` and hand the same error back
            // for `Error: …`, so a refused demote put one sentence on stderr
            // twice under two prefixes.
            //
            // An unarmed snapshot means the apply was never reached: nothing was
            // written, so there is nothing to undo and no repair to report.
            if rollback_info.previous_config.is_none() {
                return Err(e);
            }
            if let Err(rollback_err) =
                crate::utils::rollback::rollback_metadata_changes(context, &rollback_info)
            {
                context.log_error(&format!(
                    "CRITICAL: failed to roll back the declaration: {}. Manual intervention may be required.",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}
