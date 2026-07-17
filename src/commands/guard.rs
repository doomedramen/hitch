use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct GuardCommand {
    /// Environment to guard against (optional, checks all if not provided)
    pub env_name: Option<String>,
}

pub fn run(args: GuardCommand, context: &GlobalContext) -> Result<()> {
    // Get current branch first
    let current_branch = context.git().get_current_branch()?;

    if current_branch.starts_with("detached-HEAD-") {
        return Err(anyhow::anyhow!(
            "Cannot guard in detached HEAD state. Please checkout a branch first."
        ));
    }

    context.log_info(&format!(
        "Guarding against environment branches from current branch '{}'...",
        current_branch
    ));

    // Step 1: Check if hitch is initialized
    validate_preconditions(context)?;

    // Step 2: Perform guard check
    perform_guard_check(context, &current_branch, args.env_name.as_deref())?;

    context.log_info(&format!(
        "Guard check passed. Current branch '{}' is not an environment branch.",
        current_branch
    ));
    Ok(())
}

/// Validate that hitch is properly initialized
fn validate_preconditions(context: &GlobalContext) -> Result<()> {
    // Check if hitch-metadata branch exists locally, bootstrapping it from
    // origin first if a teammate already initialized Hitch (see
    // `ensure_hitch_metadata_branch`).
    if !crate::utils::prelude::ensure_hitch_metadata_branch(context)? {
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
    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;

    let mut conflicting_environments = Vec::new();

    // Check specific environment or all environments
    let environments_to_check: Vec<String> = if let Some(env_name) = specific_env {
        vec![env_name.to_string()]
    } else {
        config.get_environment_names()
    };

    for env_name in &environments_to_check {
        if config.get_environment(env_name).is_some() {
            // Check if current branch matches the environment branch name
            // This blocks direct commits to environment branches like 'dev', 'qa', 'staging'
            if current_branch == env_name {
                conflicting_environments.push(env_name.clone());
            }
        }
    }

    if !conflicting_environments.is_empty() {
        // Check if any of the conflicting environments require approval
        let approval_required_envs: Vec<String> = conflicting_environments
            .iter()
            .filter(|env_name| {
                if let Some(env) = config.get_environment(env_name) {
                    env.requires_approval_check()
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        if !approval_required_envs.is_empty() {
            // Environment requires approval - show approval-specific guidance
            let env_name = &approval_required_envs[0];
            if let Some(environment) = config.get_environment(env_name) {
                let mut message = format!(
                    "✗ Error: Cannot commit directly to environment branch '{}'\n\n\
                    This environment requires approval for changes.\n\n\
                    To promote a branch to {}:\n",
                    env_name, env_name
                );

                message.push_str(&format!(
                    "  1. Commit your changes on a feature branch\n\
                    2. Run: hitch promote <your-branch> {}\n\
                    3. Wait for approval from:\n",
                    env_name
                ));

                for approver in &environment.approvers {
                    message.push_str(&format!("       - {}\n", approver));
                }

                if environment.min_approvals > 1 {
                    message.push_str(&format!(
                        "     ({} approvals required)\n",
                        environment.min_approvals
                    ));
                }

                message.push_str(
                    "\nLearn more: hitch help promote\n\
                    Check status: hitch approvals list",
                );

                return Err(anyhow::anyhow!("{}", message));
            }
        }

        // Regular environment - show standard message
        let _env_list = conflicting_environments.join(", ");
        return Err(anyhow::anyhow!(
            "✗ Error: Cannot commit directly to environment branch '{}'\n\n\
            Environment branches are managed by hitch.\n\n\
            To promote a branch to {}:\n\
              hitch promote <your-branch> {}\n\n\
            Learn more: hitch help promote",
            conflicting_environments[0],
            conflicting_environments[0],
            conflicting_environments[0]
        ));
    }

    context.log_verbose("✓ No environment branch conflicts detected");
    Ok(())
}
