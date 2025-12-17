use crate::types::{ApprovalRequest, ApprovalStatus, Environment};
use anyhow::{anyhow, Result};

/// Validate email format
pub fn validate_email_format(email: &str) -> Result<()> {
    if !email.contains('@') {
        return Err(anyhow!("Email '{}' is missing '@' symbol", email));
    }

    if !email.contains('.') {
        return Err(anyhow!("Email '{}' is missing domain", email));
    }

    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Email '{}' has invalid format", email));
    }

    if parts[0].is_empty() {
        return Err(anyhow!("Email '{}' has empty local part", email));
    }

    if parts[1].is_empty() {
        return Err(anyhow!("Email '{}' has empty domain part", email));
    }

    if parts[1].contains('@') {
        return Err(anyhow!("Email '{}' has multiple '@' symbols", email));
    }

    Ok(())
}

/// Validate approver list
pub fn validate_approvers(approvers: &[String]) -> Result<()> {
    if approvers.is_empty() {
        return Err(anyhow!("Approvers list cannot be empty"));
    }

    for approver in approvers {
        validate_email_format(approver)?;
    }

    // Check for duplicates
    let mut seen = std::collections::HashSet::new();
    for approver in approvers {
        if !seen.insert(approver.clone()) {
            return Err(anyhow!("Duplicate approver: {}", approver));
        }
    }

    Ok(())
}

/// Validate approval configuration
pub fn validate_approval_configuration(
    requires_approval: bool,
    approvers: &[String],
    min_approvals: usize,
) -> Result<()> {
    if requires_approval {
        if approvers.is_empty() {
            return Err(anyhow!(
                "Environment requires approval but no approvers are specified"
            ));
        }

        validate_approvers(approvers)?;

        if min_approvals == 0 {
            return Err(anyhow!("Minimum approvals must be at least 1"));
        }

        if min_approvals > approvers.len() {
            return Err(anyhow!(
                "Minimum approvals ({}) cannot be greater than number of approvers ({})",
                min_approvals,
                approvers.len()
            ));
        }
    } else if !approvers.is_empty() {
        // If approval is not required, approvers list should be empty
        return Err(anyhow!(
            "Approvers list should be empty when approval is not required"
        ));
    } else if min_approvals != 1 {
        // If approval is not required, min_approvals should be default (1)
        return Err(anyhow!(
            "Minimum approvals should be 1 when approval is not required"
        ));
    }

    Ok(())
}

/// Validate approval request state
pub fn validate_approval_request_state(
    request: &ApprovalRequest,
    environment: &Environment,
) -> Result<()> {
    // Note: We don't validate environment.base against request.environment because
    // environment.base is the base branch name (e.g., "main") and request.environment
    // is the environment name (e.g., "production"). They're different things.

    // Validate approvers (for requests created after approval was enabled)
    if request.requested_at > environment.rebuilt_at.unwrap_or_default()
        && environment.requires_approval
        && environment.approvers.is_empty()
    {
        return Err(anyhow!(
                "Request was created for environment requiring approval but no approvers are configured"
            ));
    }

    // Validate status transitions
    match request.status {
        ApprovalStatus::Pending => {
            // Pending requests should not have rejection details
            if request.rejection.is_some() {
                return Err(anyhow!("Pending request should not have rejection details"));
            }

            // Check if approvals make sense for pending status
            if request.approval_count() >= environment.min_approvals {
                return Err(anyhow!(
                    "Pending request already has sufficient approvals ({} >= {})",
                    request.approval_count(),
                    environment.min_approvals
                ));
            }
        }
        ApprovalStatus::Approved => {
            // Approved requests should have sufficient approvals
            if request.approval_count() < environment.min_approvals {
                return Err(anyhow!(
                    "Approved request has insufficient approvals ({} < {})",
                    request.approval_count(),
                    environment.min_approvals
                ));
            }

            // Should not have rejection details
            if request.rejection.is_some() {
                return Err(anyhow!(
                    "Approved request should not have rejection details"
                ));
            }
        }
        ApprovalStatus::Applied => {
            // Applied requests should have been approved first
            if request.approval_count() < environment.min_approvals {
                return Err(anyhow!(
                    "Applied request has insufficient approvals ({} < {})",
                    request.approval_count(),
                    environment.min_approvals
                ));
            }

            // Should not have rejection details
            if request.rejection.is_some() {
                return Err(anyhow!("Applied request should not have rejection details"));
            }
        }
        ApprovalStatus::Rejected => {
            // Rejected requests must have rejection details
            if request.rejection.is_none() {
                return Err(anyhow!("Rejected request must have rejection details"));
            }

            // Note: Rejected requests CAN have approvals - a request might receive
            // some approvals before being rejected (e.g., 1 approval before rejection
            // in a 2-approval required environment). This is a valid state.
        }
        ApprovalStatus::Cancelled => {
            // Cancelled requests should not have rejection details
            if request.rejection.is_some() {
                return Err(anyhow!(
                    "Cancelled request should not have rejection details"
                ));
            }

            // Should not have sufficient approvals
            if request.approval_count() >= environment.min_approvals {
                return Err(anyhow!(
                    "Cancelled request has sufficient approvals ({} >= {})",
                    request.approval_count(),
                    environment.min_approvals
                ));
            }
        }
    }

    Ok(())
}

/// Validate approval execution prerequisites
pub fn validate_approval_execution_prerequisites(
    request: &ApprovalRequest,
    environment: &Environment,
) -> Result<()> {
    // Must be approved
    if request.status != ApprovalStatus::Approved {
        return Err(anyhow!(
            "Cannot execute request - current status: {} (must be Approved)",
            request.status
        ));
    }

    // Must have sufficient approvals
    if request.approval_count() < environment.min_approvals {
        return Err(anyhow!(
            "Cannot execute request - insufficient approvals ({} < {})",
            request.approval_count(),
            environment.min_approvals
        ));
    }

    // Branch must still exist (basic validation)
    if request.branch.is_empty() {
        return Err(anyhow!("Branch name cannot be empty"));
    }

    // Environment must exist
    if environment.base.is_empty() {
        return Err(anyhow!("Environment base branch cannot be empty"));
    }

    Ok(())
}

/// Validate snapshot for execution
pub fn validate_snapshot_for_execution(snapshot: &crate::types::RebuildSnapshot) -> Result<()> {
    // Base SHA must not be empty
    if snapshot.base_sha.is_empty() {
        return Err(anyhow!("Snapshot base SHA cannot be empty"));
    }

    // Must have at least the branch being promoted
    if snapshot.branch_shas.is_empty() {
        return Err(anyhow!("Snapshot must contain at least one branch"));
    }

    // All SHAs must be valid git commit SHA format (basic validation)
    if snapshot.base_sha.len() != 40 {
        return Err(anyhow!("Invalid base SHA format"));
    }

    for (branch, sha) in &snapshot.branch_shas {
        if sha.len() != 40 {
            return Err(anyhow!("Invalid SHA format for branch '{}'", branch));
        }
    }

    Ok(())
}

/// Check if two snapshots are compatible for execution
pub fn check_snapshot_compatibility(
    original_snapshot: &crate::types::RebuildSnapshot,
    current_snapshot: &crate::types::RebuildSnapshot,
) -> Result<Vec<String>> {
    let mut changes = Vec::new();

    // Check base branch
    if original_snapshot.base_sha != current_snapshot.base_sha {
        changes.push(format!(
            "Base branch changed: {} → {}",
            &original_snapshot.base_sha[..7],
            &current_snapshot.base_sha[..7]
        ));
    }

    // Check branch SHAs
    for (branch, original_sha) in &original_snapshot.branch_shas {
        match current_snapshot.branch_shas.get(branch) {
            Some(current_sha) => {
                if original_sha != current_sha {
                    changes.push(format!(
                        "Branch '{}' changed: {} → {}",
                        branch,
                        &original_sha[..7],
                        &current_sha[..7]
                    ));
                }
            }
            None => {
                changes.push(format!("Branch '{}' no longer exists", branch));
            }
        }
    }

    // Check for new branches
    for branch in current_snapshot.branch_shas.keys() {
        if !original_snapshot.branch_shas.contains_key(branch) {
            changes.push(format!("New branch '{}' added", branch));
        }
    }

    Ok(changes)
}

/// Generate validation error message with context
pub fn format_validation_error(field: &str, value: &str, error: &str) -> String {
    format!("Invalid {} '{}': {}", field, value, error)
}

/// Check if environment configuration is valid for approval workflow
pub fn validate_environment_for_approval_workflow(environment: &Environment) -> Result<()> {
    if environment.requires_approval {
        // All approval-specific validations
        environment
            .validate_approval_config()
            .map_err(|e| anyhow::anyhow!("Approval validation failed: {}", e))?;

        // Additional workflow-specific validations
        if environment.approvers.len() < 2 && environment.min_approvals > 1 {
            return Err(anyhow::anyhow!(
                "Environment requires {} approvals but only has {} approver(s). \
                Add more approvers or reduce minimum approvals.",
                environment.min_approvals,
                environment.approvers.len()
            ));
        }
    }

    Ok(())
}
