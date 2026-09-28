//! Reusable helper functions for Hitch commands
//!
//! Following SPEC.md principles: use reusable functions to avoid code duplication
//! and provide consistent patterns across all commands

use crate::commands::global_context::GlobalContext;
use anyhow::Result;

/// Reusable git branch existence check
///
/// This eliminates duplication across promote.rs, status.rs, and other commands
pub fn ensure_branch_exists(context: &GlobalContext, branch: &str) -> Result<()> {
    if !context.git().branch_exists_anywhere(branch)? {
        return Err(anyhow::anyhow!("Branch '{}' does not exist", branch));
    }
    Ok(())
}

/// Reusable git branch validation for commands
///
/// Used by promote.rs and other commands that need to verify branches
pub fn validate_branch_for_promotion(context: &GlobalContext, branch: &str) -> Result<()> {
    ensure_branch_exists(context, branch)?;

    // Additional validation could be added here in the future
    // For example: check if branch has commits, is reachable, etc.

    Ok(())
}

/// Reusable environment existence check with custom error message
pub fn ensure_environment_exists(context: &GlobalContext, env_name: &str) -> Result<()> {
    use crate::utils::prelude::access_metadata_read_only;

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    Ok(())
}

/// Reusable environment state helpers
pub mod environment {
    use crate::commands::global_context::GlobalContext;
    use anyhow::Result;

    /// Get the user who locked the environment, with a default "unknown" fallback
    pub fn get_locked_by_user(context: &GlobalContext, env_name: &str) -> Result<String> {
        let config =
            crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;

        if let Some(environment) = config.environments.get(env_name) {
            Ok(environment
                .locked_by
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .clone())
        } else {
            Ok("unknown".to_string())
        }
    }
}

/// Reusable logging patterns
///
/// One function survives, and it is a `--verbose` line for a pre-plan check —
/// the only kind of check that is allowed to narrate, because it is the only
/// kind that reaches a reader *before* there is a plan to reach. `validation_start`,
/// `operation_info` and `operation_success` were deleted with `hitch lock`, their
/// last caller: each was a second voice for an operation that has a plan and a
/// receipt, and the whole point of those two documents is that they are the only
/// ones.
pub mod logging {
    use crate::commands::global_context::GlobalContext;

    pub fn validation_success(context: &GlobalContext, item: &str, item_type: &str) {
        context.log_verbose(&format!("✓ {} validation passed for '{}'", item_type, item));
    }
}
