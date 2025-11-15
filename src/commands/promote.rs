use crate::commands::global_context::GlobalContext;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct PromoteCommand {
    /// The branch to promote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to promote the branch to
    #[arg()]
    pub env_name: String,
}

pub fn run(
    args: PromoteCommand,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Promoting branch '{}' to environment '{}'...", args.branch, args.env_name));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to promotion
    validate_preconditions(context, &args.branch, &args.env_name)?;

    // Step 2-4: Execute promotion with automatic locking and unlocking
    // This will add the branch to environment, commit metadata, and trigger rebuild
    crate::utils::prelude::with_locked_env(context, &args.env_name, || {
        promote_branch_to_environment(context, &args.branch, &args.env_name)
    })?;

    context.log_success(&format!("Successfully promoted '{}' to environment '{}'!", args.branch, args.env_name));
    Ok(())
}

/// Validate that a name is valid for git branches/environments
fn validate_name(name: &str, name_type: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("{} name cannot be empty", name_type));
    }

    if name.len() > 100 {
        return Err(anyhow::anyhow!("{} name cannot exceed 100 characters", name_type));
    }

    // Check for invalid characters that would cause issues in git
    let invalid_chars = ["..", "@{", ":", "[", "]", "\\", "^", "~", "?", "*"];
    for invalid in &invalid_chars {
        if name.contains(invalid) {
            return Err(anyhow::anyhow!(
                "{} name cannot contain '{}': '{}'",
                name_type,
                invalid,
                name
            ));
        }
    }

    // Cannot start or end with slash
    if name.starts_with('/') || name.ends_with('/') {
        return Err(anyhow::anyhow!(
            "{} name cannot start or end with '/': '{}'",
            name_type,
            name
        ));
    }

    // Cannot have consecutive slashes
    if name.contains("//") {
        return Err(anyhow::anyhow!(
            "{} name cannot contain consecutive slashes: '{}'",
            name_type,
            name
        ));
    }

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
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    let environment = &config.environments[env_name];

    // Check if environment is locked
    if environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is currently locked by '{}'",
            env_name,
            environment.locked_by.as_ref().unwrap_or(&"unknown".to_string())
        ));
    }

    // Check if branch is already promoted to this environment
    if environment.branches.contains(&branch.to_string()) {
        return Err(anyhow::anyhow!(
            "Branch '{}' is already promoted to environment '{}'",
            branch, env_name
        ));
    }

    // Check if branch exists (locally or remotely)
    if !context.git().branch_exists_anywhere(branch)? {
        return Err(anyhow::anyhow!("Branch '{}' does not exist", branch));
    }

    context.log_verbose(&format!("✓ Promotion validation passed for '{}' to '{}'", branch, env_name));
    Ok(())
}

/// Add branch to environment and trigger rebuild
fn promote_branch_to_environment(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> anyhow::Result<()> {
    context.log_verbose(&format!("Adding '{}' to environment '{}'...", branch, env_name));

    // Modify metadata to add the branch
    crate::utils::prelude::modify_metadata(context, |config| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        // Add the branch to the environment's branches list
        environment.add_branch(branch.to_string());

        context.log_verbose(&format!("✓ Added '{}' to environment '{}'", branch, env_name));
        Ok(())
    })?;

    // Trigger rebuild of the environment
    context.log_info(&format!("Triggering rebuild for environment '{}'...", env_name));
    crate::utils::prelude::rebuild_environment(context, env_name)?;

    context.log_verbose(&format!("✓ Environment '{}' rebuilt successfully", env_name));
    Ok(())
}
