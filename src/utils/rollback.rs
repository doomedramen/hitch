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

    let result = restore_previous_state(context, rollback_info);

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

/// Restore the pre-operation state. Promote and demote both simply revert the
/// captured configuration, so they share one implementation.
///
/// This uses the health-check-free write path (`modify_metadata_unchecked`) so
/// that recovery can proceed even when the repository is in the degraded state
/// that caused the original failure.
fn restore_previous_state(context: &GlobalContext, rollback_info: &RollbackInfo) -> Result<()> {
    // Prefer restoring the FULL configuration snapshot so that any change beyond
    // the target environment (an appended approval request, edits to other
    // environments) is reverted too — not just the single `Environment`.
    if let Some(previous_config) = rollback_info.previous_config.clone() {
        context.log_verbose("Restoring previous configuration snapshot...");
        crate::utils::prelude::modify_metadata_unchecked(
            context,
            move |config: &mut HitchConfig| {
                *config = previous_config;
                Ok(())
            },
        )?;
        context.log_verbose("✓ Configuration restored");
        return Ok(());
    }

    // Fallback: restore just the single environment (older capture path).
    if let Some(previous_state) = rollback_info.previous_state.clone() {
        context.log_verbose("Restoring previous environment state...");
        let env_name = rollback_info.env_name.clone();
        crate::utils::prelude::modify_metadata_unchecked(
            context,
            move |config: &mut HitchConfig| {
                config.environments.insert(env_name, previous_state);
                Ok(())
            },
        )?;
        context.log_verbose("✓ Environment state restored");
    } else {
        context.log_warning("No previous state available for rollback (this should not happen)");
    }

    Ok(())
}

/// Capture the full configuration before making changes, so rollback can restore
/// the entire state atomically (not just one environment).
pub fn capture_config_state(context: &GlobalContext) -> Result<Option<HitchConfig>> {
    context.log_verbose("Capturing current configuration for rollback...");

    let current =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;

    context.log_verbose("✓ Configuration captured");
    Ok(Some(current))
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
