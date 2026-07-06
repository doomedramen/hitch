use crate::commands::global_context::GlobalContext;
use crate::types::{ApprovalRequest, ApprovalStatus, HitchConfig, Operation};
use anyhow::{anyhow, Result};

/// Create an approval request
pub fn create_approval_request(
    context: &GlobalContext,
    config: &mut HitchConfig,
    environment_name: &str,
    branch_name: &str,
    operation: Operation,
) -> Result<String> {
    context.log_verbose(&format!(
        "Creating approval request for {} {} to {}",
        operation, branch_name, environment_name
    ));

    // Get environment configuration
    let environment = config
        .get_environment(environment_name)
        .ok_or_else(|| anyhow!("Environment '{}' not found", environment_name))?;

    // Check if approval is required
    if !environment.requires_approval {
        return Err(anyhow!(
            "Environment '{}' does not require approval",
            environment_name
        ));
    }

    // Check if there's already a pending request for this branch/environment
    let existing_pending = config
        .get_approval_requests_for_env(environment_name)
        .into_iter()
        .find(|r| r.branch == branch_name && r.status == ApprovalStatus::Pending);

    if let Some(existing) = existing_pending {
        return Err(anyhow!(
            "A pending approval request already exists for {} to {}\n\n\
            Request ID: {}\n\
            Created: {}\n\n\
            Check status: hitch approvals status {}",
            branch_name,
            environment_name,
            existing.id,
            existing.requested_at.format("%Y-%m-%d %H:%M:%S UTC"),
            existing.id
        ));
    }

    // Get current user (the requester).
    let requester_email = crate::utils::authorization::get_current_user(context)?;

    // Feasibility check: because self-approval is forbidden, the requester cannot
    // count toward the threshold. If the number of *other* approvers is below
    // min_approvals, the request could never be satisfied — reject it up front
    // with an actionable message instead of creating an unfillable request.
    let eligible_approvers = environment
        .approvers
        .iter()
        .filter(|a| **a != requester_email)
        .count();
    if environment.min_approvals > eligible_approvers {
        return Err(anyhow!(
            "Cannot create approval request: {} approval(s) required, but only {} eligible \
             approver(s) are available.\n\
             The requester ({}) cannot approve their own request, so they don't count toward \
             the threshold.\n\
             Add more approvers (hitch set {} --add-approver <email>) or lower the threshold \
             (hitch set {} --min-approvals <n>).",
            environment.min_approvals,
            eligible_approvers,
            requester_email,
            environment_name,
            environment_name
        ));
    }

    // Freeze the threshold at creation time so later `hitch set --min-approvals`
    // can't retroactively change what this request needs.
    let frozen_min_approvals = environment.min_approvals;

    // Create snapshot
    let snapshot =
        crate::utils::snapshot::capture_rebuild_snapshot(context, environment, branch_name)?;

    // Create the approval request
    let mut request = ApprovalRequest::new(
        environment_name.to_string(),
        branch_name.to_string(),
        operation,
        requester_email,
        snapshot,
    );
    request.snapshot_min_approvals = Some(frozen_min_approvals);

    let request_id = request.id.clone();

    // Add to config
    config.add_approval_request(request);

    context.log_verbose(&format!("Approval request created: {}", request_id));

    Ok(request_id)
}

/// Resolve a user-supplied request identifier to a single request's full ID.
///
/// Accepts either the full UUID or an unambiguous prefix of it (the `list`/status
/// commands display truncated IDs, so users naturally copy a prefix). Errors if
/// nothing matches or if a prefix is ambiguous.
pub fn resolve_request_id(config: &HitchConfig, request_id: &str) -> Result<String> {
    if request_id.is_empty() {
        return Err(anyhow!("No approval request ID provided"));
    }

    // Exact match wins outright.
    if let Some(r) = config.get_approval_request(request_id) {
        return Ok(r.id.clone());
    }

    // Otherwise, an unambiguous prefix match.
    let matches: Vec<&ApprovalRequest> = config
        .get_approval_requests()
        .iter()
        .filter(|r| r.id.starts_with(request_id))
        .collect();

    match matches.as_slice() {
        [] => Err(anyhow!("Approval request '{}' not found", request_id)),
        [only] => Ok(only.id.clone()),
        many => Err(anyhow!(
            "Approval request ID '{}' is ambiguous — it matches {} requests. \
             Use more characters of the ID.",
            request_id,
            many.len()
        )),
    }
}

/// Find an approval request by ID (full UUID or unambiguous prefix).
pub fn find_approval_request<'a>(
    config: &'a HitchConfig,
    request_id: &str,
) -> Result<&'a ApprovalRequest> {
    let full_id = resolve_request_id(config, request_id)?;
    config
        .get_approval_request(&full_id)
        .ok_or_else(|| anyhow!("Approval request '{}' not found", request_id))
}

/// Find an approval request by ID (full UUID or unambiguous prefix), mutable.
pub fn find_approval_request_mut<'a>(
    config: &'a mut HitchConfig,
    request_id: &str,
) -> Result<&'a mut ApprovalRequest> {
    // Resolve with an immutable borrow first, then re-borrow mutably by full ID.
    let full_id = resolve_request_id(config, request_id)?;
    config
        .get_approval_request_mut(&full_id)
        .ok_or_else(|| anyhow!("Approval request '{}' not found", request_id))
}

/// Approve a request
pub fn approve_request(
    context: &GlobalContext,
    config: &mut HitchConfig,
    request_id: &str,
    comment: Option<String>,
) -> Result<bool> {
    context.log_verbose(&format!("Approving request: {}", request_id));

    // Get request details first
    let request = find_approval_request(config, request_id)?;
    let environment_name = request.environment.clone();

    // Get environment for validation and clone it
    let environment = config
        .get_environment(&environment_name)
        .cloned()
        .ok_or_else(|| anyhow!("Environment '{}' not found", environment_name))?;

    // Threshold to satisfy — frozen at request creation (falls back to the
    // environment's current value for legacy requests).
    let min_approvals = request.required_approvals(&environment);

    // Validate authorization before getting mutable reference
    crate::utils::authorization::validate_approval_authorization(context, &environment, request)?;

    // Get current user
    let approver_email = crate::utils::authorization::get_current_user(context)?;

    // Now get mutable reference for modifications
    let request = find_approval_request_mut(config, request_id)?;

    // Add approval
    request.add_approval(approver_email, comment);

    context.log_info(&format!("✓ Approval recorded for request {}", request_id));

    // Check if threshold is met
    if request.threshold_met(min_approvals) {
        request.mark_approved();
        context.log_info(&format!(
            "✓ Approval threshold met ({} approvals)",
            request.approval_count()
        ));
        return Ok(true); // Threshold met, ready to execute
    }

    // Show remaining approvals needed
    let remaining = min_approvals - request.approval_count();
    let remaining_approvers =
        crate::utils::authorization::get_remaining_approvers(&environment, request);

    context.log_info(&format!(
        "Approvals: {}/{}\nWaiting for {} more approval(s) from:\n{}",
        request.approval_count(),
        min_approvals,
        remaining,
        remaining_approvers
            .iter()
            .map(|a| format!("  - {}", a))
            .collect::<Vec<_>>()
            .join("\n")
    ));

    Ok(false) // Threshold not yet met
}

/// Reject a request
pub fn reject_request(
    context: &GlobalContext,
    config: &mut HitchConfig,
    request_id: &str,
    reason: String,
) -> Result<()> {
    context.log_verbose(&format!("Rejecting request: {}", request_id));

    // Get request details first
    let request = find_approval_request(config, request_id)?;
    let environment_name = request.environment.clone();

    // Get environment for validation
    let environment = config
        .get_environment(&environment_name)
        .ok_or_else(|| anyhow!("Environment '{}' not found", environment_name))?;

    // Validate authorization before getting mutable reference
    crate::utils::authorization::validate_rejection_authorization(context, environment, request)?;

    // Get current user
    let rejecter_email = crate::utils::authorization::get_current_user(context)?;

    // Now get mutable reference for modifications
    let request = find_approval_request_mut(config, request_id)?;

    // Reject the request
    request.reject(rejecter_email, reason);

    context.log_success(&format!("✓ Request {} rejected", request_id));

    Ok(())
}

/// Cancel a request
pub fn cancel_request(
    context: &GlobalContext,
    config: &mut HitchConfig,
    request_id: &str,
) -> Result<()> {
    context.log_verbose(&format!("Cancelling request: {}", request_id));

    let request = find_approval_request_mut(config, request_id)?;

    // Validate authorization
    crate::utils::authorization::validate_cancellation_authorization(context, request)?;

    // Cancel the request
    request.cancel();

    context.log_success(&format!("✓ Request {} cancelled", request_id));

    Ok(())
}

/// Mark a request as applied (after successful execution)
pub fn mark_request_applied(config: &mut HitchConfig, request_id: &str) -> Result<()> {
    let request = find_approval_request_mut(config, request_id)?;

    if request.status != ApprovalStatus::Approved {
        return Err(anyhow!(
            "Cannot mark request as applied - current status: {}",
            request.status
        ));
    }

    request.mark_applied();
    Ok(())
}

/// Get approval requests with optional filtering
pub fn get_approval_requests<'a>(
    config: &'a HitchConfig,
    environment: Option<&str>,
    status: Option<ApprovalStatus>,
) -> Vec<&'a ApprovalRequest> {
    let requests = config.get_approval_requests();

    let mut result: Vec<&'a ApprovalRequest> = requests
        .iter()
        .filter(|r| {
            // Filter by environment
            if let Some(env) = environment {
                if r.environment != env {
                    return false;
                }
            }
            // Filter by status
            if let Some(s) = status {
                if r.status != s {
                    return false;
                }
            }
            true
        })
        .collect();

    // Sort by requested_at (newest first)
    result.sort_by_key(|a| std::cmp::Reverse(a.requested_at));

    result
}

/// Display approval request information after creation
pub fn display_approval_request_info(
    context: &GlobalContext,
    config: &HitchConfig,
    request_id: &str,
) -> Result<()> {
    let request = find_approval_request(config, request_id)?;
    let environment = config
        .get_environment(&request.environment)
        .ok_or_else(|| anyhow!("Environment '{}' not found", request.environment))?;

    context.log_success(&format!("✓ Approval request created: {}", request_id));
    context.log_info("");
    context.log_info(&format!("Environment: {}", request.environment));
    context.log_info(&format!("Branch: {}", request.branch));
    context.log_info(&format!("Operation: {}", request.operation));
    context.log_info("");

    context.log_info(&format!(
        "Waiting for {} approval(s) from:",
        environment.min_approvals
    ));

    for approver in &environment.approvers {
        let status = if request.has_approved(approver) {
            "✓"
        } else {
            "⏳"
        };
        context.log_info(&format!("  {} {}", status, approver));
    }

    context.log_info("");
    context.log_info(&format!(
        "Check status: hitch approvals status {}",
        request_id
    ));

    Ok(())
}
