use crate::commands::global_context::GlobalContext;
use crate::types::{ApprovalRequest, Environment};
use anyhow::{anyhow, Result};

/// Get the current user's email from git configuration
pub fn get_current_user(context: &GlobalContext) -> Result<String> {
    context
        .git()
        .get_user_email()
        .map_err(|e| anyhow!("Failed to get user email from git config: {}", e))
}

/// Check if a user is authorized to approve requests for an environment
pub fn is_authorized_approver(environment: &Environment, user_email: &str) -> bool {
    environment.is_approver(user_email)
}

/// Validate that a user can approve a specific request
pub fn validate_approval_authorization(
    context: &GlobalContext,
    environment: &Environment,
    request: &ApprovalRequest,
) -> Result<()> {
    let user_email = get_current_user(context)?;

    // Check if user is in approvers list
    if !is_authorized_approver(environment, &user_email) {
        return Err(anyhow!(
            "You ({}) are not authorized to approve changes to '{}'\n\nAuthorized approvers:\n{}",
            user_email,
            request.environment,
            environment
                .approvers
                .iter()
                .map(|a| format!("  - {}", a))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    // Check if it's self-approval
    if user_email == request.requested_by {
        return Err(anyhow!(
            "Self-approval is not allowed\n\nYou requested this change, so you cannot approve it.\n\
            An authorized approver must review and approve this change."
        ));
    }

    // Check if user has already approved
    if request.has_approved(&user_email) {
        return Err(anyhow!(
            "You have already approved this request\n\nApprovals: {}/{}\n\
            Waiting for {} more approval(s) from:\n{}",
            request.approval_count(),
            environment.min_approvals,
            environment
                .min_approvals
                .saturating_sub(request.approval_count()),
            environment
                .approvers
                .iter()
                .filter(|a| !request.has_approved(a))
                .map(|a| format!("  - {}", a))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    // Check if request is still pending
    if request.status != crate::types::ApprovalStatus::Pending {
        return Err(anyhow!(
            "Request is not pending (current status: {})",
            request.status
        ));
    }

    Ok(())
}

/// Validate that a user can cancel a request
pub fn validate_cancellation_authorization(
    context: &GlobalContext,
    request: &ApprovalRequest,
) -> Result<()> {
    let user_email = get_current_user(context)?;

    // Only the requester can cancel their own request
    if user_email != request.requested_by {
        return Err(anyhow!(
            "You ({}) are not authorized to cancel this request\n\n\
            Only the requester ({}) can cancel this request.",
            user_email,
            request.requested_by
        ));
    }

    // Check if request is still pending
    if request.status != crate::types::ApprovalStatus::Pending {
        return Err(anyhow!(
            "Cannot cancel request with status: {}",
            request.status
        ));
    }

    Ok(())
}

/// Validate that a user can reject a request
pub fn validate_rejection_authorization(
    context: &GlobalContext,
    environment: &Environment,
    request: &ApprovalRequest,
) -> Result<()> {
    let user_email = get_current_user(context)?;

    // Check if user is in approvers list
    if !is_authorized_approver(environment, &user_email) {
        return Err(anyhow!(
            "You ({}) are not authorized to reject changes to '{}'",
            user_email,
            request.environment
        ));
    }

    // Check if request is still pending
    if request.status != crate::types::ApprovalStatus::Pending {
        return Err(anyhow!(
            "Cannot reject request with status: {}",
            request.status
        ));
    }

    Ok(())
}

/// Check if approval threshold is met
pub fn is_approval_threshold_met(request: &ApprovalRequest, min_approvals: usize) -> bool {
    request.threshold_met(min_approvals)
}

/// Get remaining approvals needed
pub fn get_remaining_approvals(request: &ApprovalRequest, min_approvals: usize) -> usize {
    min_approvals.saturating_sub(request.approval_count())
}

/// Get users who can still approve the request
pub fn get_remaining_approvers(
    environment: &Environment,
    request: &ApprovalRequest,
) -> Vec<String> {
    environment
        .approvers
        .iter()
        .filter(|approver| !request.has_approved(approver))
        .filter(|approver| **approver != request.requested_by) // Exclude requester
        .cloned()
        .collect()
}
