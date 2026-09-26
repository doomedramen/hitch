use crate::commands::global_context::GlobalContext;
use crate::operations::declaration::{apply_declaration_plan, plan_demote, DeclarationPlanOptions};
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
}

pub fn run(args: DemoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Demoting branch '{}' from environment '{}'...",
        args.branch, args.env_name
    ));

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

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Demote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Plan, then apply, both under the environment lock. Planning inside
    // the lock is forced, not stylistic: `with_locked_env` commits the lock to
    // `hitch-metadata` first, so a plan built outside it is stale on arrival.
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            rollback_info.previous_config = crate::utils::rollback::capture_config_state(context)?;
            let plan = plan_demote(
                context,
                &args.branch,
                &args.env_name,
                DeclarationPlanOptions {
                    no_rebuild: args.no_rebuild,
                },
                &mut |_| {},
            )?;

            let branches = match &plan.intent {
                crate::operations::model::OperationIntent::DemoteBranches { branches, .. } => {
                    branches.clone()
                }
                // Unreachable: `plan_demote` is the only constructor of this
                // intent. An error rather than an empty list, which would read
                // as "demoted 0 branches" and exit 0.
                _ => anyhow::bail!("internal error: demote produced a non-demote plan"),
            };
            if branches.len() != 1 || branches.first().map(|b| b.as_str()) != Some(&args.branch) {
                context.log_info(&format!(
                    "Resolved '{}' → {} branch(es): {}",
                    args.branch,
                    branches.len(),
                    branches.join(", ")
                ));
            }

            // What will really be removed, for the success message. A demote
            // no-ops branches that are not present, and resolving a source
            // environment can name some of those — so the count the user is told
            // about is the planner's narrowed list, not the argument's.
            let demoted = plan.detail.removed.clone();
            let receipt = apply_declaration_plan(context, &plan, &mut |_| {})?;
            Ok((demoted, receipt))
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok((demoted, receipt)) => {
            match receipt.outcome {
                // An approval gate is not a demotion and not a failure:
                // nothing was declared, and `hitch approve` is the next command.
                crate::operations::model::OperationOutcome::ApprovalRequested => Ok(()),
                _ if demoted.len() == 1 => {
                    context.log_success(&format!(
                        "Successfully demoted '{}' from environment '{}'!",
                        demoted[0], args.env_name
                    ));
                    Ok(())
                }
                _ => {
                    context.log_success(&format!(
                        "Successfully demoted {} branches from environment '{}'!",
                        demoted.len(),
                        args.env_name
                    ));
                    Ok(())
                }
            }
        }
        Err(e) => {
            // Show the actual error FIRST so user knows why it failed
            context.log_error(&format!("Error: {}", e));

            // A failure before the lock was taken captured no config, and
            // rolling back to "nothing" is not a rollback.
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
