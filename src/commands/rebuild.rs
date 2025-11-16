use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{access_metadata_read_only, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RebuildCommand {
    /// The name of the environment to rebuild
    #[arg()]
    pub env_name: String,

    /// Force rebuild even if environment is locked
    #[arg(long)]
    pub force: bool,

    /// Replace the remote branch (force push) after rebuilding
    #[arg(long)]
    pub replace_remote: bool,
}

pub fn run(args: RebuildCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Rebuilding environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    // Step 2-5: Execute rebuild with automatic locking and unlocking
    // Using the reusable rebuild_environment function from prelude

    if args.force {
        // For force mode, we need to handle locking manually since the environment is already locked
        context.log_info(&format!(
            "Force rebuilding locked environment '{}'...",
            args.env_name
        ));
        crate::utils::prelude::rebuild_environment(context, &args.env_name, args.replace_remote)?;
    } else {
        // Normal mode: use automatic locking and unlocking
        with_locked_env(context, &args.env_name, || {
            crate::utils::prelude::rebuild_environment(context, &args.env_name, args.replace_remote)
        })?;
    }

    context.log_success(&format!(
        "Environment '{}' rebuilt successfully!",
        args.env_name
    ));
    Ok(())
}

/// Validate that environment exists and is not locked (unless force flag is used)
fn validate_environment_exists_and_unlocked(
    context: &GlobalContext,
    env_name: &str,
    force: bool,
) -> Result<()> {
    context.log_verbose("Validating environment preconditions...");

    // Check if environment exists
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    let environment = &config.environments[env_name];

    // Check if environment is locked (unless force is used)
    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by {}. Use --force to override.",
            env_name,
            environment
                .locked_by
                .as_ref()
                .unwrap_or(&"unknown".to_string())
        ));
    }

    context.log_verbose(&format!("✓ Environment '{}' validation passed", env_name));
    Ok(())
}
