use crate::commands::global_context::GlobalContext;
use crate::operations::declaration::{
    apply_declaration_plan, plan_promote, DeclarationPlanOptions,
};
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
}

pub fn run(args: PromoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Promoting branch '{}' to environment '{}'...",
        args.branch, args.env_name
    ));

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
            let plan = plan_promote(
                context,
                &args.branch,
                &args.env_name,
                DeclarationPlanOptions {
                    no_rebuild: args.no_rebuild,
                },
                &mut |_| {},
            )?;
            let branches = resolved_branches(context, &plan, &args.branch)?;
            let receipt = apply_declaration_plan(context, &plan, &mut |_| {})?;
            Ok((branches, receipt))
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok((branches, receipt)) => {
            match receipt.outcome {
                // An approval gate is not a failure and not a promotion
                // either: nothing was declared, and the user's next command is
                // `hitch approve`. Exit 0, and say what is now waiting.
                crate::operations::model::OperationOutcome::ApprovalRequested => Ok(()),
                _ if branches.len() == 1 => {
                    context.log_success(&format!(
                        "Successfully promoted '{}' to environment '{}'!",
                        branches[0], args.env_name
                    ));
                    Ok(())
                }
                _ => {
                    context.log_success(&format!(
                        "Successfully promoted {} branches to environment '{}'!",
                        branches.len(),
                        args.env_name
                    ));
                    Ok(())
                }
            }
        }
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

/// The branches the plan actually resolved, plus the "Resolved '<env>' → N
/// branch(es)" line when the argument named an environment rather than a
/// branch. Both are read off the plan rather than recomputed, so the message
/// cannot describe a different promotion than the one that ran.
fn resolved_branches(
    context: &GlobalContext,
    plan: &crate::operations::model::OperationPlan<
        crate::operations::declaration::DeclarationPlanDetail,
    >,
    argument: &str,
) -> Result<Vec<String>> {
    let branches = match &plan.intent {
        crate::operations::model::OperationIntent::PromoteBranches { branches, .. } => {
            branches.clone()
        }
        // Unreachable: `plan_promote` is the only constructor of this intent.
        // An error rather than an empty list, which would read as "promoted 0
        // branches" and exit 0.
        _ => anyhow::bail!("internal error: promote produced a non-promote plan"),
    };
    if branches.len() != 1 || branches.first().map(|b| b.as_str()) != Some(argument) {
        context.log_info(&format!(
            "Resolved '{}' → {} branch(es): {}",
            argument,
            branches.len(),
            branches.join(", ")
        ));
    }
    Ok(branches)
}
