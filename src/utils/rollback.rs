use crate::commands::global_context::GlobalContext;
use crate::types::{HitchConfig, RollbackInfo, RollbackOperation};
use anyhow::Result;

/// Main rollback function that dispatches to operation-specific rollback
pub fn rollback_metadata_changes(
    context: &GlobalContext,
    rollback_info: &RollbackInfo,
) -> Result<()> {
    // One line, not two. This used to open with `log_warning("Operation failed,
    // attempting automatic rollback...")` and then `log_info("Rolling back
    // promote operation for branch 'fc' in environment 'dev'")` — the same
    // sentence at two levels, neither carrying anything the other did not.
    // The reason it was worth printing at all is that it runs *after* the error
    // is already on its way out, so it cannot be the user's first clue.
    context.log_warning(&format!(
        "Rolling back {} for '{}' in environment '{}'",
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
            // No leading `✓`: `log_success` already prefixes one, so this used
            // to print `✅ ✓ Automatic rollback completed successfully`. And no
            // "You can now retry the promote command" after it — the error the
            // caller returns already names what to do, and on the one failure
            // that reaches here (a metadata write that half-landed) "just retry
            // it" is the wrong advice, because retrying is what produced the
            // half-landed write.
            context.log_success("Declaration restored — nothing was changed");
            Ok(())
        }
        Err(e) => {
            // Reported once, by the caller. This used to log two `CRITICAL:`
            // lines here *and* hand the error back for the caller to log a third,
            // all spelling the same failure at two verbosities.
            context.log_verbose(&format!("Rollback failed: {}", e));
            context
                .log_verbose("Manual intervention may be required to restore metadata consistency");
            context.log_verbose("Run 'hitch status' to check the current state");
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
    // Only the full-configuration snapshot. The single-`Environment` fallback
    // that used to sit below this is dead for every current caller — promote and
    // demote capture a snapshot and set nothing else — and it was worse than
    // dead: with no state of either kind it warned and returned `Ok(())`, so
    // the caller went on to report `Declaration restored — nothing was changed`
    // about a restore that had not been attempted. A rollback that cannot happen
    // is an error, not a quiet success.
    let previous_config = rollback_info
        .previous_config
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no pre-operation configuration was captured"))?;

    context.log_verbose("Restoring previous configuration snapshot...");
    crate::utils::prelude::modify_metadata_unchecked(context, move |config: &mut HitchConfig| {
        *config = previous_config;
        Ok(())
    })?;
    context.log_verbose("✓ Configuration restored");
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
