use crate::commands::global_context::GlobalContext;
use crate::utils::command_helpers::{environment::get_locked_by_user, logging::{validation_start, validation_success, operation_info, operation_success}};
use crate::utils::validation::{validate_name, validate_environment_exists};
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
    operation_info(context, "Locking", &args.env_name);

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name)?;

    // Step 2: Lock the environment
    crate::utils::prelude::lock_environment(context, &args.env_name)?;

    operation_success(context, "locked", &args.env_name);
    Ok(())
}


/// Validate that environment exists and is ready for locking
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
) -> Result<()> {
    validation_start(context, "lock");

    // Validate input name
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    validate_environment_exists(context, env_name)?;

    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;
    let environment = &config.environments[env_name];

    // Check if environment is already locked
    if environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is already locked by '{}'",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    validation_success(context, env_name, "Lock validation");
    Ok(())
}

