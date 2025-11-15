//! Reusable validation utilities for Hitch commands
//!
//! Following SPEC.md principles: use reusable functions to avoid code duplication

use crate::commands::global_context::GlobalContext;
use anyhow::Result;

/// Reusable name validation for environments and branches
///
/// This function eliminates the massive code duplication that existed across
/// add.rs, remove.rs, lock.rs, unlock.rs, demote.rs, and promote.rs
pub fn validate_name(name: &str, name_type: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("{} name cannot be empty", name_type));
    }

    if name.len() > 100 {
        return Err(anyhow::anyhow!("{} name cannot exceed 100 characters", name_type));
    }

    // Check for invalid characters that would cause issues in git
    let invalid_chars = ["..", "@{", ":", "[", "]", "\\", "^", "~", "?", "*"];
    for invalid in &invalid_chars {
        if name.contains(invalid) {
            return Err(anyhow::anyhow!(
                "{} name cannot contain '{}': '{}'",
                name_type,
                invalid,
                name
            ));
        }
    }

    // Cannot start or end with slash
    if name.starts_with('/') || name.ends_with('/') {
        return Err(anyhow::anyhow!(
            "{} name cannot start or end with '/': '{}'",
            name_type,
            name
        ));
    }

    // Cannot have consecutive slashes
    if name.contains("//") {
        return Err(anyhow::anyhow!(
            "{} name cannot contain consecutive slashes: '{}'",
            name_type,
            name
        ));
    }

    Ok(())
}

/// Reusable environment existence validation
pub fn validate_environment_exists(context: &GlobalContext, env_name: &str) -> Result<()> {
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    Ok(())
}

/// Reusable environment non-existence validation
pub fn validate_environment_not_exists(context: &GlobalContext, env_name: &str) -> Result<()> {
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' already exists", env_name));
    }

    Ok(())
}

/// Reusable base branch validation
pub fn validate_base_branch_exists(context: &GlobalContext, base_branch: &str) -> Result<()> {
    if !context.git().branch_exists_anywhere(base_branch)? {
        return Err(anyhow::anyhow!(
            "Base branch '{}' does not exist locally or remotely",
            base_branch
        ));
    }
    Ok(())
}