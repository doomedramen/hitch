use crate::commands::global_context::GlobalContext;
use crate::utils::validation::validate_name;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct UnlockCommand {
    /// The environment to unlock
    #[arg()]
    pub env_name: String,
}

pub fn run(
    args: UnlockCommand,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Unlocking environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name)?;

    // Step 2: Unlock the environment
    unlock_environment(context, &args.env_name)?;

    context.log_success(&format!("Successfully unlocked environment '{}'!", args.env_name));
    Ok(())
}


/// Validate that environment exists and is ready for unlocking
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
) -> Result<()> {
    context.log_verbose("Validating unlock preconditions...");

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

    // Check if environment is locked
    if !environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is not currently locked",
            env_name
        ));
    }

    // Check if environment is locked by the current user
    let current_user = context.git().get_user_email()?;
    if let Some(locked_by) = &environment.locked_by {
        if locked_by != &current_user {
            return Err(anyhow::anyhow!(
                "Environment '{}' is locked by '{}'. Only the locker can unlock it.",
                env_name, locked_by
            ));
        }
    }

    context.log_verbose(&format!("✓ Unlock validation passed for '{}'", env_name));
    Ok(())
}

/// Unlock an environment
fn unlock_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    crate::utils::prelude::unlock_environment(context, env_name)
}
