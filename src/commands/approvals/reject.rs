use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
use crate::utils::prelude::{modify_metadata, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RejectArgs {
    /// Approval request ID
    pub request_id: String,

    /// Reason for rejection (required)
    #[arg()]
    pub reason: String,
}

pub fn run(args: RejectArgs, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Rejecting request {}...", args.request_id));

    // Step 1: pre_check - Ensure git repository is in good state
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Validate and process the rejection
    validate_and_reject(context, &args)?;

    // Step 3: Show completion status
    context.log_success(&format!(
        "✓ Request {} rejected successfully!",
        args.request_id
    ));

    context.log_info(&format!("Reason: {}", args.reason));

    // Suggest next actions
    let _ = suggest_next_actions(context, &args);

    Ok(())
}

fn validate_and_reject(context: &GlobalContext, args: &RejectArgs) -> Result<()> {
    // Get request details first for validation
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
    let environment_name = request.environment.clone();

    // Get environment configuration
    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &environment_name)?;

    // Validate authorization
    validate_rejection_authorization(context, &environment, &request)?;

    // Validate reason
    validate_rejection_reason(&args.reason)?;

    // Validate request state
    validate_request_state_for_rejection(&request)?;

    // Perform the rejection
    with_locked_env(context, &environment_name, || {
        modify_metadata(context, |config: &mut HitchConfig| {
            // Reject the request
            crate::utils::approvals::reject_request(
                context,
                config,
                &args.request_id,
                args.reason.clone(),
            )?;

            Ok(())
        })
    })?;

    context.log_verbose("✓ Request rejection completed successfully");
    Ok(())
}

fn validate_rejection_authorization(
    context: &GlobalContext,
    environment: &crate::types::Environment,
    request: &crate::types::ApprovalRequest,
) -> Result<()> {
    // Check if user is authorized to reject
    let current_user = crate::utils::authorization::get_current_user(context)?;

    if !environment.is_approver(&current_user) {
        return Err(anyhow::anyhow!(
            "You are not authorized to reject requests for environment '{}'\n\n\
            Authorized approvers:\n{}",
            request.environment,
            environment
                .approvers
                .iter()
                .map(|a| format!("  - {}", a))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    // Check if self-rejection (allowed but should be discouraged)
    if current_user == request.requested_by {
        context.log_warning("⚠️  You are rejecting your own request");
    }

    Ok(())
}

fn validate_rejection_reason(reason: &str) -> Result<()> {
    if reason.trim().is_empty() {
        return Err(anyhow::anyhow!("Rejection reason cannot be empty"));
    }

    if reason.len() < 10 {
        return Err(anyhow::anyhow!(
            "Rejection reason must be at least 10 characters long to provide meaningful feedback"
        ));
    }

    if reason.len() > 1000 {
        return Err(anyhow::anyhow!(
            "Rejection reason must be less than 1000 characters long"
        ));
    }

    // Check for common non-informative reasons
    let non_informative_patterns = [
        "no",
        "nah",
        "bad",
        "wrong",
        "not good",
        "fix it",
        "needs work",
        "revise",
        "changes needed",
    ];

    let lower_reason = reason.to_lowercase();
    for pattern in &non_informative_patterns {
        if lower_reason.trim() == *pattern {
            return Err(anyhow::anyhow!(
                "Rejection reason must be more specific. Please provide detailed feedback about what needs to be changed."
            ));
        }
    }

    Ok(())
}

fn validate_request_state_for_rejection(request: &crate::types::ApprovalRequest) -> Result<()> {
    match request.status {
        crate::types::ApprovalStatus::Pending => {
            // This is the only state where rejection is allowed
            Ok(())
        }
        crate::types::ApprovalStatus::Approved => Err(anyhow::anyhow!(
            "Cannot reject request that is already approved. Status: {}",
            request.status
        )),
        crate::types::ApprovalStatus::Applied => Err(anyhow::anyhow!(
            "Cannot reject request that has already been applied. Status: {}",
            request.status
        )),
        crate::types::ApprovalStatus::Rejected => Err(anyhow::anyhow!(
            "Request is already rejected. Status: {}",
            request.status
        )),
        crate::types::ApprovalStatus::Cancelled => Err(anyhow::anyhow!(
            "Cannot reject request that has been cancelled. Status: {}",
            request.status
        )),
    }
}

fn suggest_next_actions(context: &GlobalContext, args: &RejectArgs) -> Result<()> {
    context.log_info("");
    context.log_info("Next steps:");

    // Get the updated request to show current status
    let updated_request =
        crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;

    if let Some(rejection) = &updated_request.rejection {
        context.log_info("  • Rejection details:");
        context.log_info(&format!("    - Rejected by: {}", rejection.rejected_by));
        context.log_info(&format!(
            "    - Rejected at: {}",
            rejection.rejected_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        context.log_info(&format!("    - Reason: {}", rejection.reason));
    }

    context.log_info("");
    context.log_info("For the requester:");

    // Get the current user to suggest actions
    let current_user = crate::utils::authorization::get_current_user(context)?;

    if current_user == updated_request.requested_by {
        context.log_info("  • Review the feedback and make the suggested changes");
        context.log_info("  • Create a new approval request when ready:");
        context.log_info(&format!(
            "    hitch promote {} {}",
            updated_request.branch, updated_request.environment
        ));
    } else {
        context.log_info(&format!(
            "  • Communicate the rejection details to: {}",
            updated_request.requested_by
        ));
        context.log_info("  • Guide them on the necessary changes");
    }

    context.log_info("");
    context.log_info("View all requests:");
    context.log_info("  hitch approvals list");
    context.log_info(&format!(
        "  hitch approvals list --environment {}",
        updated_request.environment
    ));

    Ok(())
}
