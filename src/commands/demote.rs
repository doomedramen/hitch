use crate::commands::global_context::GlobalContext;
use crate::types::{RollbackInfo, RollbackOperation};
use crate::utils::command_helpers::{environment::get_locked_by_user, logging::validation_success};
use crate::utils::validation::validate_name;
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

    // Step 2: Additional validation specific to demotion.
    // When the environment requires approval, this creates an approval request
    // and returns `true` to signal that the demotion must NOT be applied now —
    // it is applied later by `hitch approvals approve` once the threshold is met.
    if validate_preconditions(context, &args.branch, &args.env_name)? {
        return Ok(());
    }

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Demote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Auto-stash dirty changes, execute demotion, then pop stash
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            demote_branch_from_environment(
                context,
                &args.branch,
                &args.env_name,
                &mut rollback_info,
                args.no_rebuild,
            )
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok(()) => {
            context.log_success(&format!(
                "Successfully demoted '{}' from environment '{}'!",
                args.branch, args.env_name
            ));
            Ok(())
        }
        Err(e) => {
            // Show the actual error FIRST so user knows why it failed
            context.log_error(&format!("Error: {}", e));

            // Attempt automatic rollback
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

/// Validate that branch and environment are ready for demotion.
///
/// Returns `Ok(true)` when the environment requires approval and an approval
/// request was created instead of applying the demotion — the caller must stop
/// and NOT apply the demotion. Returns `Ok(false)` when validation passed and
/// the caller should proceed to apply the demotion.
fn validate_preconditions(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> anyhow::Result<bool> {
    context.log_verbose("Validating demotion preconditions...");

    // Validate input names
    validate_name(branch, "Branch")?;
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    crate::utils::command_helpers::ensure_environment_exists(context, env_name)?;

    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = &config.environments[env_name];

    // Check if environment is locked
    if environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is currently locked by '{}'",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    // Check if branch is actually promoted to this environment
    if !environment.branches.contains(&branch.to_string()) {
        return Err(anyhow::anyhow!(
            "Branch '{}' is not promoted to environment '{}'",
            branch,
            env_name
        ));
    }

    // Check if approval is required for this environment
    if environment.requires_approval_check() {
        context.log_info(&format!(
            "Environment '{}' requires approval before demotion",
            env_name
        ));

        // Create approval request instead of executing demotion
        let request_id = crate::utils::prelude::create_approval_request_for_operation(
            context,
            env_name,
            branch,
            crate::types::Operation::Demote,
        )?;

        // Display approval request information
        crate::utils::prelude::display_approval_request_created(context, &request_id)?;

        // Signal the caller to stop: the demotion must wait for approval and
        // will be applied by `hitch approvals approve` once the threshold is met.
        return Ok(true);
    }

    validation_success(
        context,
        &format!("'{}' from '{}'", branch, env_name),
        "Demotion validation",
    );
    Ok(false)
}

/// Remove branch from environment and trigger rebuild
fn demote_branch_from_environment(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
    rollback_info: &mut RollbackInfo,
    no_rebuild: bool,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Removing '{}' from environment '{}'...",
        branch, env_name
    ));

    // Capture pre-operation configuration for rollback (full snapshot so any
    // side effects beyond this environment are also reverted on failure).
    rollback_info.previous_config = crate::utils::rollback::capture_config_state(context)?;

    // Modify metadata to remove the branch. The pre-operation state for rollback was
    // already captured above via `capture_environment_state`.
    crate::utils::prelude::modify_metadata(context, |config| {
        let available_envs = config.get_environment_names().join(", ");
        let environment = config.get_environment_mut(env_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' not found in hitch configuration. Available environments: {}",
                env_name,
                available_envs
            )
        })?;

        // Remove the branch from the environment's branches list
        environment.remove_branch(branch);

        context.log_verbose(&format!(
            "✓ Removed '{}' from environment '{}'",
            branch, env_name
        ));
        Ok(())
    })?;

    if no_rebuild {
        context.log_info(&format!(
            "Skipping rebuild for environment '{}' (--no-rebuild flag set). Run 'hitch rebuild {}' when ready.",
            env_name, env_name
        ));
    } else {
        // Trigger rebuild of the environment
        context.log_info(&format!(
            "Triggering rebuild for environment '{}'...",
            env_name
        ));
        crate::utils::prelude::rebuild_environment(context, env_name)?;

        context.log_verbose(&format!(
            "✓ Environment '{}' rebuilt successfully",
            env_name
        ));
    }
    Ok(())
}
