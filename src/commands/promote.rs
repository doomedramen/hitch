use crate::commands::global_context::GlobalContext;
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

    /// Replace the remote branch (force push) after rebuilding
    #[arg(long)]
    pub replace_remote: bool,
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

    // Step 2-4: Execute promotion with automatic locking and unlocking
    // This will add the branch to environment, commit metadata, and trigger rebuild
    crate::utils::prelude::with_locked_env(context, &args.env_name, || {
        promote_branch_to_environment(context, &args.branch, &args.env_name, args.replace_remote)
    })?;

    context.log_success(&format!(
        "Successfully promoted '{}' to environment '{}'!",
        args.branch, args.env_name
    ));
    Ok(())
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
    replace_remote: bool,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Adding '{}' to environment '{}'...",
        branch, env_name
    ));

    // Modify metadata to add the branch
    crate::utils::prelude::modify_metadata(context, |config| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

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
    crate::utils::prelude::rebuild_environment(context, env_name, replace_remote)?;

    context.log_verbose(&format!(
        "✓ Environment '{}' rebuilt successfully",
        env_name
    ));
    Ok(())
}
