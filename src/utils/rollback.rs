use crate::commands::global_context::GlobalContext;
use crate::types::{HitchConfig, RollbackInfo, RollbackOperation};
use anyhow::Result;

/// Main rollback function that dispatches to operation-specific rollback
pub fn rollback_metadata_changes(
    context: &GlobalContext,
    rollback_info: &RollbackInfo,
) -> Result<()> {
    context.log_warning("Operation failed, attempting automatic rollback...");
    context.log_info(&format!(
        "Rolling back {} operation for branch '{}' in environment '{}'",
        match rollback_info.operation {
            RollbackOperation::Promote => "promote",
            RollbackOperation::Demote => "demote",
        },
        rollback_info.branch,
        rollback_info.env_name
    ));

    let result = match rollback_info.operation {
        RollbackOperation::Promote => rollback_promote(context, rollback_info),
        RollbackOperation::Demote => rollback_demote(context, rollback_info),
    };

    match result {
        Ok(()) => {
            context.log_success("✓ Automatic rollback completed successfully");
            context.log_info(&format!(
                "You can now retry the {} command",
                match rollback_info.operation {
                    RollbackOperation::Promote => "promote",
                    RollbackOperation::Demote => "demote",
                }
            ));
            Ok(())
        }
        Err(e) => {
            context.log_error(&format!(
                "CRITICAL: Failed to rollback metadata changes: {}",
                e
            ));
            context
                .log_error("Manual intervention may be required to restore metadata consistency");
            context.log_info("You can try running 'hitch status' to check the current state");
            Err(e)
        }
    }
}

/// Rollback a failed promote operation by restoring the previous environment state
fn rollback_promote(context: &GlobalContext, rollback_info: &RollbackInfo) -> Result<()> {
    context.log_info("Rolling back promote operation...");

    // Restore the previous environment state
    if let Some(ref previous_state) = rollback_info.previous_state {
        context.log_verbose("Restoring previous environment state...");

        // Use modify_metadata to restore the environment
        crate::utils::prelude::modify_metadata(context, |config: &mut HitchConfig| {
            config
                .environments
                .insert(rollback_info.env_name.clone(), previous_state.clone());
            Ok(())
        })?;

        context.log_verbose("✓ Environment state restored");
    } else {
        context.log_warning("No previous state available for rollback (this should not happen)");
    }

    Ok(())
}

/// Rollback a failed demote operation by restoring the previous environment state
fn rollback_demote(context: &GlobalContext, rollback_info: &RollbackInfo) -> Result<()> {
    context.log_info("Rolling back demote operation...");

    // Restore the previous environment state
    if let Some(ref previous_state) = rollback_info.previous_state {
        context.log_verbose("Restoring previous environment state...");

        // Use modify_metadata to restore the environment
        crate::utils::prelude::modify_metadata(context, |config: &mut HitchConfig| {
            config
                .environments
                .insert(rollback_info.env_name.clone(), previous_state.clone());
            Ok(())
        })?;

        context.log_verbose("✓ Environment state restored");
    } else {
        context.log_warning("No previous state available for rollback (this should not happen)");
    }

    Ok(())
}

/// Capture the current environment state before making changes
pub fn capture_environment_state(
    context: &GlobalContext,
    env_name: &str,
) -> Result<Option<crate::types::Environment>> {
    context.log_verbose(&format!(
        "Capturing current state for environment '{}'...",
        env_name
    ));

    let current_state = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.get_environment(env_name).cloned())
    })?;

    context.log_verbose("✓ Environment state captured");
    Ok(current_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Environment, RollbackOperation};

    #[test]
    fn test_rollback_info_creation() {
        let rollback_info = RollbackInfo::new(
            RollbackOperation::Promote,
            "dev".to_string(),
            "feature-branch".to_string(),
        );

        assert!(matches!(
            rollback_info.operation,
            RollbackOperation::Promote
        ));
        assert_eq!(rollback_info.env_name, "dev");
        assert_eq!(rollback_info.branch, "feature-branch");
        assert!(rollback_info.previous_state.is_none());
    }

    #[test]
    fn test_rollback_info_with_state() {
        let env = Environment::new("main".to_string());
        let mut rollback_info = RollbackInfo::new(
            RollbackOperation::Demote,
            "staging".to_string(),
            "old-feature".to_string(),
        );

        rollback_info.previous_state = Some(env.clone());

        assert!(matches!(rollback_info.operation, RollbackOperation::Demote));
        assert_eq!(rollback_info.env_name, "staging");
        assert_eq!(rollback_info.branch, "old-feature");
        assert!(rollback_info.previous_state.is_some());
    }
}
