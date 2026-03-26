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
}

pub fn run(args: RebuildCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Rebuilding environment '{}'...", args.env_name));

    // Step 1: Precondition checks (allow dirty tree — we'll stash it)
    // Only check git repo, not working tree cleanliness
    if let Err(e) = context.git().get_current_branch() {
        return Err(anyhow::anyhow!("Not in a Git repository: {}", e));
    }
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    // Step 2-5: Auto-stash dirty changes, execute rebuild, pop stash
    crate::utils::prelude::with_auto_stash(context, || {
        if args.force {
            context.log_info(&format!(
                "Force rebuilding locked environment '{}'...",
                args.env_name
            ));
            crate::utils::prelude::rebuild_environment(context, &args.env_name)
        } else {
            with_locked_env(context, &args.env_name, || {
                crate::utils::prelude::rebuild_environment(context, &args.env_name)
            })
        }
    })?;

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
