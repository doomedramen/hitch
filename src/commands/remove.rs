use crate::commands::global_context::GlobalContext;
use crate::utils::validation::validate_name;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct RemoveCommand {
    /// The environment to remove
    #[arg()]
    pub env_name: String,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub fn run(
    args: RemoveCommand,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Removing environment '{}'...", args.env_name));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to remove
    validate_preconditions(context, &args.env_name, args.force)?;

    // Step 3: Remove the environment
    remove_environment(context, &args.env_name)?;

    context.log_success(&format!("Successfully removed environment '{}'!", args.env_name));
    Ok(())
}


/// Validate that environment is ready for removal
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
    force: bool,
) -> Result<()> {
    context.log_verbose("Validating remove preconditions...");

    // Validate input name
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    let environment = &config.environments[env_name];

    // Check if environment has branches (confirmation required unless force)
    if !force && !environment.branches.is_empty() {
        // In a real implementation, we would prompt for confirmation here
        // For now, we'll require the --force flag
        return Err(anyhow::anyhow!(
            "Environment '{}' has promoted branches. Use --force to remove it anyway. Note: This only removes the environment configuration, not the actual git branches.",
            env_name
        ));
    }

    // Check if environment is locked (but allow removal if force is used)
    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by '{}'. Cannot remove a locked environment. Use --force to override.",
            env_name,
            environment.locked_by.as_ref().unwrap_or(&"unknown".to_string())
        ));
    }

    context.log_verbose(&format!("✓ Remove validation passed for '{}'", env_name));
    Ok(())
}

/// Remove an environment from the configuration
fn remove_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    context.log_verbose(&format!("Removing environment '{}' from configuration...", env_name));

    // Modify metadata to remove the environment
    crate::utils::prelude::modify_metadata(context, |config| {
        config.remove_environment(env_name);

        context.log_verbose(&format!("✓ Removed environment '{}' from configuration", env_name));
        Ok(())
    })
}
