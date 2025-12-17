use crate::commands::global_context::GlobalContext;
use crate::utils::authorization::get_current_user;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct StatusArgs {
    /// Approval request ID
    pub request_id: String,
}

pub fn run(args: StatusArgs, context: &GlobalContext) -> Result<()> {
    // Get the approval request
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;

    // Get environment configuration
    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &request.environment)?;

    // Display detailed status
    display_request_details(&request, &environment, context)?;

    // Show approval progress
    display_approval_progress(&request, &environment, context)?;

    // Show snapshot information
    display_snapshot_info(&request, context)?;

    // Show available actions
    display_available_actions(&request, &environment, context)?;

    Ok(())
}

fn display_request_details(
    request: &crate::types::ApprovalRequest,
    environment: &crate::types::Environment,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Approval Request: {}", request.id));
    println!("{}", "=".repeat(60));

    // Basic information
    println!("📋 Basic Information:");
    println!("  Environment: {}", request.environment);
    println!("  Branch: {}", request.branch);
    println!("  Operation: {}", request.operation);
    println!(
        "  Status: {} {}",
        get_status_indicator(&request.status),
        request.status
    );
    println!("  Requested by: {}", request.requested_by);
    println!(
        "  Requested at: {}",
        request.requested_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    // Environment requirements
    if environment.requires_approval_check() {
        println!();
        println!("🔐 Approval Requirements:");
        println!("  Approvals required: {}", environment.min_approvals);
        println!("  Authorized approvers:");
        for approver in environment.approvers.iter() {
            let status = if request.has_approved(approver) {
                "✓"
            } else {
                "⏳"
            };
            println!("    {} {}", status, approver);
        }
    }

    Ok(())
}

fn display_approval_progress(
    request: &crate::types::ApprovalRequest,
    environment: &crate::types::Environment,
    _context: &GlobalContext,
) -> Result<()> {
    if !environment.requires_approval_check() {
        return Ok(());
    }

    println!();
    println!("📊 Approval Progress:");

    if request.approvals.is_empty() {
        println!("  No approvals yet received");
    } else {
        for (i, approval) in request.approvals.iter().enumerate() {
            println!(
                "  {}. {} - {}",
                i + 1,
                approval.approved_by,
                approval.approved_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            if let Some(comment) = &approval.comment {
                for line in comment.lines() {
                    println!("      {}", line);
                }
            }
        }
    }

    // Show progress
    let current_approvals = request.approval_count();
    let required_approvals = environment.min_approvals;
    let remaining = required_approvals.saturating_sub(current_approvals);

    println!();
    println!(
        "  Progress: {}/{} approvals",
        current_approvals, required_approvals
    );

    // Visual progress bar
    let bar_width = 20;
    let filled = (current_approvals * bar_width) / required_approvals.min(bar_width);
    let empty = bar_width.saturating_sub(filled);

    print!("  [");
    for _ in 0..filled {
        print!("█");
    }
    for _ in 0..empty {
        print!("░");
    }
    println!("] {}%", (current_approvals * 100) / required_approvals);

    if remaining > 0 && request.status == crate::types::ApprovalStatus::Pending {
        println!("  Waiting for {} more approval(s)", remaining);
    }

    Ok(())
}

fn display_snapshot_info(
    request: &crate::types::ApprovalRequest,
    context: &GlobalContext,
) -> Result<()> {
    println!();
    println!("📸 Code Snapshot:");

    let snapshot = &request.rebuild_snapshot;
    println!("  Base branch SHA: {}", &snapshot.base_sha[..7]);

    if !snapshot.branch_shas.is_empty() {
        println!("  Branch snapshots:");
        for (branch, sha) in &snapshot.branch_shas {
            println!("    {}: {}", branch, &sha[..7]);
        }
    }

    println!(
        "  Merge conflicts: {}",
        if snapshot.merge_conflicts {
            "Yes ⚠️"
        } else {
            "No ✓"
        }
    );

    // Verify snapshot is current
    match crate::utils::snapshot::validate_snapshot(context, snapshot) {
        Ok(()) => println!("  Snapshot status: Current ✓"),
        Err(e) => {
            context.log_warning("  Snapshot status: Outdated ⚠️");
            context.log_warning(&format!("    {}", e));
        }
    }

    Ok(())
}

fn display_available_actions(
    request: &crate::types::ApprovalRequest,
    environment: &crate::types::Environment,
    context: &GlobalContext,
) -> Result<()> {
    println!();
    println!("🎯 Available Actions:");

    let current_user = get_current_user(context)?;
    let is_approver = environment.is_approver(&current_user);
    let is_requester = current_user == request.requested_by;

    match request.status {
        crate::types::ApprovalStatus::Pending => {
            if is_approver
                && !request.has_approved(&current_user)
                && current_user != request.requested_by
            {
                println!("  ✅ Approve this request:");
                println!(
                    "     hitch approvals approve {} --comment 'Your approval comment'",
                    request.id
                );

                if !request.approvals.is_empty() {
                    println!("  ❌ Reject this request:");
                    println!(
                        "     hitch approvals reject {} 'Your reason for rejection'",
                        request.id
                    );
                }
            } else if !is_approver {
                println!("  ⏳ Waiting for approval from authorized approvers");
                println!("     You are not in the approver list for this environment");
            } else if request.has_approved(&current_user) {
                println!("  ⏳ You have already approved this request");
                println!(
                    "     Waiting for {} more approval(s)",
                    environment
                        .min_approvals
                        .saturating_sub(request.approval_count())
                );
            }

            if is_requester {
                println!("  ❌ Cancel this request:");
                println!("     hitch approvals cancel {}", request.id);
            }
        }
        crate::types::ApprovalStatus::Approved => {
            if is_approver || is_requester {
                println!("  🚀 This request is approved and ready for execution");
                if request.approval_count() >= environment.min_approvals {
                    println!("     The approval threshold has been met");
                }
            }
        }
        crate::types::ApprovalStatus::Applied => {
            println!("  ✅ This request has been successfully applied");
            println!("     The {} operation was completed", request.operation);
        }
        crate::types::ApprovalStatus::Rejected => {
            if let Some(rejection) = &request.rejection {
                println!("  ❌ This request was rejected");
                println!("     Rejected by: {}", rejection.rejected_by);
                println!(
                    "     Rejected at: {}",
                    rejection.rejected_at.format("%Y-%m-%d %H:%M:%S UTC")
                );
                println!("     Reason: {}", rejection.reason);
            }
        }
        crate::types::ApprovalStatus::Cancelled => {
            println!("  ⏹ This request was cancelled by the requester");
            println!(
                "     Cancelled at: {}",
                request.requested_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
        }
    }

    // Show list command for more info
    println!();
    println!("📋 View all requests:");
    println!("     hitch approvals list");

    Ok(())
}

fn get_status_indicator(status: &crate::types::ApprovalStatus) -> &'static str {
    match status {
        crate::types::ApprovalStatus::Pending => "⏳",
        crate::types::ApprovalStatus::Approved => "✓",
        crate::types::ApprovalStatus::Applied => "🚀",
        crate::types::ApprovalStatus::Rejected => "✗",
        crate::types::ApprovalStatus::Cancelled => "⏹",
    }
}
