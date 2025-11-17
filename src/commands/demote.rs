use crate::commands::global_context::GlobalContext;
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
}

pub fn run(args: DemoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Demoting branch '{}' from environment '{}'...",
        args.branch, args.env_name
    ));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to demotion
    validate_preconditions(context, &args.branch, &args.env_name)?;

    // Step 2-4: Execute demotion with automatic locking and unlocking
    // This will remove the branch from environment, commit metadata, and trigger rebuild
    crate::utils::prelude::with_locked_env(context, &args.env_name, || {
        demote_branch_from_environment(context, &args.branch, &args.env_name)
    })?;

    context.log_success(&format!(
        "Successfully demoted '{}' from environment '{}'!",
        args.branch, args.env_name
    ));
    Ok(())
}

/// Validate that branch and environment are ready for demotion
fn validate_preconditions(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> anyhow::Result<()> {
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

    validation_success(
        context,
        &format!("'{}' from '{}'", branch, env_name),
        "Demotion validation",
    );
    Ok(())
}

/// Remove branch from environment and trigger rebuild
fn demote_branch_from_environment(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Removing '{}' from environment '{}'...",
        branch, env_name
    ));

    // Modify metadata to remove the branch
    crate::utils::prelude::modify_metadata(context, |config| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        // Remove the branch from the environment's branches list
        environment.remove_branch(branch);

        context.log_verbose(&format!(
            "✓ Removed '{}' from environment '{}'",
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
