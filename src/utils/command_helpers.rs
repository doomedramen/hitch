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

    let config = access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    Ok(())
}

/// Reusable error message patterns
pub mod errors {
    /// Standard error for empty names
    pub fn empty_name(name_type: &str) -> anyhow::Error {
        anyhow::anyhow!("{} name cannot be empty", name_type)
    }

    /// Standard error for names that are too long
    pub fn name_too_long(name_type: &str) -> anyhow::Error {
        anyhow::anyhow!("{} name cannot exceed 100 characters", name_type)
    }

    /// Standard error for names with invalid characters
    pub fn invalid_char(name_type: &str, name: &str, invalid_char: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{} name cannot contain '{}': '{}'",
            name_type,
            invalid_char,
            name
        )
    }

    /// Standard error for environment does not exist
    pub fn environment_not_exists(env_name: &str) -> anyhow::Error {
        anyhow::anyhow!("Environment '{}' does not exist", env_name)
    }

    /// Standard error for environment already exists
    pub fn environment_already_exists(env_name: &str) -> anyhow::Error {
        anyhow::anyhow!("Environment '{}' already exists", env_name)
    }

    /// Standard error for branch does not exist
    pub fn branch_not_exists(branch: &str) -> anyhow::Error {
        anyhow::anyhow!("Branch '{}' does not exist", branch)
    }

    /// Standard error for base branch does not exist
    pub fn base_branch_not_exists(base_branch: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "Base branch '{}' does not exist locally or remotely",
            base_branch
        )
    }
}

/// Reusable environment state helpers
pub mod environment {
    use crate::commands::global_context::GlobalContext;
    use anyhow::Result;

    /// Get the user who locked the environment, with a default "unknown" fallback
    pub fn get_locked_by_user(context: &GlobalContext, env_name: &str) -> Result<String> {
        let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
            Ok(config.clone())
        })?;

        if let Some(environment) = config.environments.get(env_name) {
            Ok(environment.locked_by.as_ref().unwrap_or(&"unknown".to_string()).clone())
        } else {
            Ok("unknown".to_string())
        }
    }
}

/// Reusable logging patterns
pub mod logging {
    use crate::commands::global_context::GlobalContext;

    /// Standard validation success message
    pub fn validation_success(context: &GlobalContext, item: &str, item_type: &str) {
        context.log_verbose(&format!("✓ {} validation passed for '{}'", item_type, item));
    }

    /// Standard validation start message
    pub fn validation_start(context: &GlobalContext, operation: &str) {
        context.log_verbose(&format!("Validating {} preconditions...", operation));
    }

    /// Standard operation success message
    pub fn operation_success(context: &GlobalContext, operation: &str, target: &str) {
        context.log_success(&format!("Successfully {} '{}'!", operation, target));
    }

    /// Standard operation info message
    pub fn operation_info(context: &GlobalContext, operation: &str, target: &str) {
        context.log_info(&format!("{} '{}'...", operation, target));
    }

    /// Standard verbose operation message
    pub fn operation_verbose(context: &GlobalContext, operation: &str, target: &str) {
        context.log_verbose(&format!("{} '{}'...", operation, target));
    }
}