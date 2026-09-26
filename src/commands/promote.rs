use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::declaration::{
    apply_declaration_plan, plan_promote, DeclarationPlanOptions,
};
use crate::operations::model::OperationOutcome;
use crate::types::{RollbackInfo, RollbackOperation};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PromoteCommand {
    /// The branch to promote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to promote the branch to
    #[arg()]
    pub env_name: String,

    /// Skip the automatic rebuild after promotion.
    /// Use this to batch multiple promotes and then run 'hitch rebuild <env>' once.
    #[arg(long)]
    pub no_rebuild: bool,

    /// Show the plan — what would be declared, what would be rebuilt, which
    /// environments would follow — and stop. Changes nothing: no stash, no
    /// lock, no writes.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: PromoteCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Ensure we are in a Git repository
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Step 2: The environment must exist, and a lock set by a *human* (`hitch lock`) is a refusal; the lock
    // this command takes for itself is not. So the check happens here, before
    // `with_locked_env`, which would otherwise see its own lock and mistake it
    // for someone else's. Same shape as `commands/rebuild.rs`.
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
        // Preview. Planned *outside* the environment lock, which inverts the
        // rule the real path below follows, and is safe for exactly one reason:
        // a preview plan is never applied, so nothing can go stale between
        // planning and applying it. The rule the real path follows is safe
        // because the planner does not consult `is_locked()` — which is why the
        // human-lock refusal above, and not the planner, enforces it.
        //
        // No `with_auto_stash` either. It stashes the working tree, so a
        // "changes nothing" flag that rearranged the user's uncommitted work
        // would not be describing itself honestly.
        let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut |_| {})?;
        emit_plan(context, &plan)?;
        return Ok(());
    }

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Plan, then apply, both under the environment lock.
    //
    // Planning *inside* the lock is not incidental, it is forced: `with_locked_env`
    // commits the lock to `hitch-metadata` before running its closure, so a plan
    // built outside it is stale the moment it is applied — every promotion would
    // refuse itself. Planning under the lock is also the right shape, because the
    // repo-wide lock is already held for the whole window (see `operations`'s
    // module header) and this narrows that to the environment being edited.
    //
    // `capture_config_state` stays *after* the lock is taken: capturing before
    // would record a pre-lock declaration, and rolling back to that would undo
    // the lock's own commit along with the edit.
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            rollback_info.previous_config = crate::utils::rollback::capture_config_state(context)?;
            let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut |_| {})?;
            if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
                return Ok((plan, None));
            }
            let receipt = apply_declaration_plan(context, &plan, &mut |_| {})?;
            Ok((plan, Some(receipt)))
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok((plan, Some(receipt))) => {
            emit_receipt(context, &plan, &receipt)?;
            if receipt.outcome == OperationOutcome::ApprovalRequested {
                // The remedy is printed *here* and not by the plan, because the
                // plan cannot know it: the request is created by the apply, and a
                // planner that named a request id would be naming one that does
                // not exist yet. A refusal to promote with no next step is the
                // same dead end the held-branch remedy exists to avoid.
                context.log_info(
                    "Run 'hitch approvals list' to see the request, then 'hitch approvals approve <id>' to grant it.",
                );
            }
            Ok(())
        }
        // Declined. The gate already showed the plan and the answer, and nothing
        // was written, so there is nothing to report and nothing to roll back —
        // a "rolled back" line for a no-op is a scare, not an account. Exit 0:
        // declining is not a failure.
        Ok((_plan, None)) => Ok(()),
        Err(e) => {
            // Show the actual error FIRST so user knows why it failed
            context.log_error(&format!("Error: {}", e));

            // Attempt automatic rollback. The plan does not make this
            // unnecessary: the declaration edit is what it undoes, and a
            // failure after that edit landed still needs undoing. A failure
            // before the lock was taken captured no config, and rolling back to
            // "nothing" is not a rollback.
            if rollback_info.previous_config.is_none() {
                return Err(e);
            }
            if let Err(rollback_err) =
                crate::utils::rollback::rollback_metadata_changes(context, &rollback_info)
            {
                context.log_error(&format!(
                    "CRITICAL: Failed to rollback metadata changes: {}. Manual intervention may be required.",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}
