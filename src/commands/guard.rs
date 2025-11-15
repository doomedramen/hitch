use crate::commands::global_context::GlobalContext;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct GuardCommand {
    /// Environment to guard against (optional, checks all if not provided)
    pub env_name: Option<String>,
}

pub fn run(
    args: GuardCommand,
    context: &GlobalContext,
) -> Result<()> {
    // Get current branch first
    let current_branch = context.git().get_current_branch()?;

    if current_branch.starts_with("detached-HEAD-") {
        return Err(anyhow::anyhow!(
            "Cannot guard in detached HEAD state. Please checkout a branch first."
        ));
    }

    context.log_info(&format!("Guarding against environment branches from current branch '{}'...", current_branch));

    // Step 1: Check if hitch is initialized
    validate_preconditions(context)?;

    // Step 2: Perform guard check
    perform_guard_check(context, &current_branch, args.env_name.as_deref())?;

    context.log_info(&format!("Guard check passed. Current branch '{}' is not an environment branch.", current_branch));
    Ok(())
}

/// Validate that hitch is properly initialized
fn validate_preconditions(context: &GlobalContext) -> Result<()> {
    // Check if hitch-metadata branch exists
    if !context.git().branch_exists("hitch-metadata")? {
        return Err(anyhow::anyhow!(
            "Hitch is not initialized. Run 'hitch init' first."
        ));
    }

    Ok(())
}

/// Perform the guard check against environment branches
fn perform_guard_check(
    context: &GlobalContext,
    current_branch: &str,
    specific_env: Option<&str>,
) -> Result<()> {
    // Read environment configuration using read-only access
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    let mut conflicting_environments = Vec::new();

    // Check specific environment or all environments
    let environments_to_check: Vec<String> = if let Some(env_name) = specific_env {
        vec![env_name.to_string()]
    } else {
        config.get_environment_names()
    };

    for env_name in &environments_to_check {
        if let Some(environment) = config.get_environment(env_name) {
            // Check if current branch matches the environment branch name
            if current_branch == env_name {
                conflicting_environments.push(env_name.clone());
            }

            // Also check if current branch is one of the promoted branches
            if environment.branches.contains(&current_branch.to_string()) {
                conflicting_environments.push(env_name.clone());
            }
        }
    }

    if !conflicting_environments.is_empty() {
        let env_list = conflicting_environments.join(", ");
        return Err(anyhow::anyhow!(
            "Current branch '{}' conflicts with environment(s): {}. This appears to be an environment branch that should not be directly modified.",
            current_branch,
            env_list
        ));
    }

    context.log_verbose("✓ No environment branch conflicts detected");
    Ok(())
}
