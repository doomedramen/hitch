use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
use crate::utils::prelude::{modify_metadata, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CancelArgs {
    /// Approval request ID
    pub request_id: String,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: CancelArgs, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Cancelling request {}...", args.request_id));

    // Step 1: pre_check - Ensure git repository is in good state
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Get request details for validation and confirmation
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;

    // Step 3: Validate cancellation authorization
    validate_cancellation_authorization(context, &request)?;

    // Step 4: Show request details and ask for confirmation (unless --force)
    if !args.force {
        show_cancellation_confirmation(context, &request)?;
    }

    // Step 5: Process the cancellation
    process_cancellation(context, &args)?;

    // Step 6: Show completion status
    context.log_success(&format!(
        "✓ Request {} cancelled successfully!",
        args.request_id
    ));

    // Suggest next actions
    let _ = suggest_next_actions(context, &request);

    Ok(())
}

fn validate_cancellation_authorization(
    context: &GlobalContext,
    request: &crate::types::ApprovalRequest,
) -> Result<()> {
    let current_user = crate::utils::authorization::get_current_user(context)?;

    if current_user != request.requested_by {
        return Err(anyhow::anyhow!(
            "Only the requester can cancel an approval request\n\n\
            Requested by: {}\n\
            Current user: {}",
            request.requested_by,
            current_user
        ));
    }

    Ok(())
}

fn show_cancellation_confirmation(
    context: &GlobalContext,
    request: &crate::types::ApprovalRequest,
) -> Result<()> {
    context.log_info("Request Details:");
    context.log_info(&format!("  Environment: {}", request.environment));
    context.log_info(&format!("  Branch: {}", request.branch));
    context.log_info(&format!("  Operation: {}", request.operation));
    context.log_info(&format!("  Status: {}", request.status));
    context.log_info(&format!(
        "  Requested at: {}",
        request.requested_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));

    if !request.approvals.is_empty() {
        context.log_info(&format!(
            "  Current approvals: {}",
            request.approval_count()
        ));
        for approval in &request.approvals {
            context.log_info(&format!(
                "    - {} ({})",
                approval.approved_by,
                approval.approved_at.format("%Y-%m-%d %H:%M")
            ));
        }
    }

    context.log_info("");
    context.log_info("⚠️  Are you sure you want to cancel this request?");
    context.log_info("  This action cannot be undone.");

    // Simple confirmation - in a real implementation, you might want a more sophisticated prompt
    println!();
    println!("Type 'yes' to confirm cancellation, or anything else to abort:");

    // Read user input from stdin
    use std::io::{self, Write};
    let mut input = String::new();
    print!("> ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut input)?;

    match input.trim().to_lowercase().as_str() {
        "yes" | "y" => {
            context.log_info("Cancellation confirmed.");
            Ok(())
        }
        _ => {
            context.log_info("Cancellation aborted.");
            Err(anyhow::anyhow!("Operation cancelled by user"))
        }
    }
}

fn process_cancellation(context: &GlobalContext, args: &CancelArgs) -> Result<()> {
    // Get environment name for locking
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
    let environment_name = request.environment.clone();

    // Perform the cancellation
    with_locked_env(context, &environment_name, || {
        modify_metadata(context, |config: &mut HitchConfig| {
            // Cancel the request
            crate::utils::approvals::cancel_request(context, config, &args.request_id)?;

            Ok(())
        })
    })?;

    context.log_verbose("✓ Request cancellation completed successfully");
    Ok(())
}

fn suggest_next_actions(
    context: &GlobalContext,
    request: &crate::types::ApprovalRequest,
) -> Result<()> {
    context.log_info("");
    context.log_info("Next steps:");

    // Get the current user
    let current_user = crate::utils::authorization::get_current_user(context)?;

    if current_user == request.requested_by {
        context.log_info("• Review your changes and address any feedback received:");

        if !request.approvals.is_empty() {
            context.log_info("  The request had approvals, so consider:");
            context.log_info("    - Why you decided to cancel");
            context.log_info("    - What changes you want to make");
        }

        context.log_info("");
        context.log_info("• Create a new approval request when ready:");
        context.log_info(&format!(
            "  hitch promote {} {}",
            request.branch, request.environment
        ));
    }

    context.log_info("");
    context.log_info("View all requests:");
    context.log_info("  hitch approvals list");
    context.log_info(&format!(
        "  hitch approvals list --environment {}",
        request.environment
    ));
    context.log_info("  hitch approvals list --status pending");

    // If there were approvals, suggest acknowledging them
    if !request.approvals.is_empty() {
        context.log_info("");
        context.log_info("• Consider reaching out to the approvers who had already approved:");
        for approval in &request.approvals {
            context.log_info(&format!(
                "  - {} (approved at {})",
                approval.approved_by,
                approval.approved_at.format("%Y-%m-%d %H:%M")
            ));
        }
    }

    Ok(())
}
