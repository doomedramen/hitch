use crate::commands::global_context::GlobalContext;
use crate::utils::command_helpers::environment::get_locked_by_user;
use crate::utils::validation::validate_name;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct LockCommand {
    /// The environment to lock
    #[arg()]
    pub env_name: String,
}

pub fn run(
    args: LockCommand,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Locking environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name)?;

    // Step 2: Lock the environment
    crate::utils::prelude::lock_environment(context, &args.env_name)?;

    context.log_success(&format!("Successfully locked environment '{}'!", args.env_name));
    Ok(())
}


/// Validate that environment exists and is ready for locking
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
) -> Result<()> {
    context.log_verbose("Validating lock preconditions...");

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

    // Check if environment is already locked
    if environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is already locked by '{}'",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    context.log_verbose(&format!("✓ Lock validation passed for '{}'", env_name));
    Ok(())
}

