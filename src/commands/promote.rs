use crate::commands::global_context::GlobalContext;
use crate::types::{RollbackInfo, RollbackOperation};
use crate::utils::command_helpers::{
    ensure_environment_exists, environment::get_locked_by_user, logging::validation_success,
    validate_branch_for_promotion,
};
use crate::utils::validation::validate_name;
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
}

pub fn run(args: PromoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Promoting branch '{}' to environment '{}'...",
        args.branch, args.env_name
    ));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to promotion
    validate_preconditions(context, &args.branch, &args.env_name)?;

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Execute promotion with automatic locking and unlocking and rollback capability
    let result = crate::utils::prelude::with_locked_env(context, &args.env_name, || {
        promote_branch_to_environment(context, &args.branch, &args.env_name, &mut rollback_info)
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok(()) => {
            context.log_success(&format!(
                "Successfully promoted '{}' to environment '{}'!",
                args.branch, args.env_name
            ));
            Ok(())
        }
        Err(e) => {
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

/// Validate that branch and environment are ready for promotion
fn validate_preconditions(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> anyhow::Result<()> {
    context.log_verbose("Validating promotion preconditions...");

    // Validate input names
    validate_name(branch, "Branch")?;
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    ensure_environment_exists(context, env_name)?;

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

    // Check if branch is already promoted to this environment
    if environment.branches.contains(&branch.to_string()) {
        return Err(anyhow::anyhow!(
            "Branch '{}' is already promoted to environment '{}'",
            branch,
            env_name
        ));
    }

    // Check if branch exists and is valid for promotion
    validate_branch_for_promotion(context, branch)?;

    validation_success(
        context,
        &format!("'{}' to '{}'", branch, env_name),
        "Promotion validation",
    );
    Ok(())
}

/// Add branch to environment and trigger rebuild
fn promote_branch_to_environment(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
    rollback_info: &mut RollbackInfo,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Adding '{}' to environment '{}'...",
        branch, env_name
    ));

    // Capture pre-operation environment state for rollback
    rollback_info.previous_state =
        crate::utils::rollback::capture_environment_state(context, env_name)?;

    // Modify metadata to add the branch with rollback tracking
    crate::utils::prelude::modify_metadata_with_rollback(context, rollback_info, |config| {
        let available_envs = config.get_environment_names().join(", ");
        let environment = config.get_environment_mut(env_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' not found in hitch configuration. Available environments: {}",
                env_name,
                available_envs
            )
        })?;

        // Add the branch to the environment's branches list
        environment.add_branch(branch.to_string());

        context.log_verbose(&format!(
            "✓ Added '{}' to environment '{}'",
            branch, env_name
        ));
        Ok(())
    })?;

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
    Ok(())
}
