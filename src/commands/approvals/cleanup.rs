use crate::commands::global_context::GlobalContext;
use crate::types::{ApprovalStatus, HitchConfig};
use crate::utils::prelude::modify_metadata;
use anyhow::Result;
use chrono::{Duration, Utc};
use clap::Args;

#[derive(Args)]
pub struct CleanupArgs {
    /// Remove requests older than this many days (default: 90)
    #[arg(long, default_value = "90")]
    pub older_than: i64,

    /// Also remove pending requests (default: false, only removes completed/rejected/cancelled)
    #[arg(long)]
    pub include_pending: bool,

    /// Dry run - show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: CleanupArgs, context: &GlobalContext) -> Result<()> {
    context.log_info("Checking for old approval requests to clean up...");

    // Validate older_than parameter
    if args.older_than < 1 {
        return Err(anyhow::anyhow!("older_than must be at least 1 day"));
    }

    // Get the cutoff date
    let cutoff_date = Utc::now() - Duration::days(args.older_than);

    // Get requests to clean up
    let (to_delete, total_count) =
        identify_requests_to_delete(context, &cutoff_date, args.include_pending)?;

    if to_delete.is_empty() {
        context.log_info(&format!(
            "No approval requests found older than {} days matching the criteria.",
            args.older_than
        ));
        return Ok(());
    }

    // Show what will be deleted
    display_cleanup_summary(context, &to_delete, args.older_than, total_count)?;

    // Confirm deletion unless --dry-run (the global --yes auto-confirms)
    if !args.dry_run && !confirm_cleanup(context, to_delete.len())? {
        context.log_info("Cleanup cancelled.");
        return Ok(());
    }

    // Perform cleanup
    if args.dry_run {
        context.log_info("");
        context.log_info("DRY RUN: No requests were actually deleted.");
        context.log_info(&format!(
            "Run without --dry-run to delete {} requests.",
            to_delete.len()
        ));
    } else {
        let deleted_count = perform_cleanup(context, &to_delete)?;
        context.log_success(&format!(
            "✓ Successfully cleaned up {} approval requests",
            deleted_count
        ));
    }

    Ok(())
}

fn identify_requests_to_delete(
    context: &GlobalContext,
    cutoff_date: &chrono::DateTime<Utc>,
    include_pending: bool,
) -> Result<(Vec<String>, usize)> {
    let mut to_delete = Vec::new();

    crate::utils::prelude::access_metadata_read_only(context, |config| {
        let total_count = config.approval_requests.len();

        for request in &config.approval_requests {
            // Check if request is old enough
            if request.requested_at > *cutoff_date {
                continue;
            }

            // Check status
            let should_delete = match request.status {
                ApprovalStatus::Pending => include_pending,
                ApprovalStatus::Applied | ApprovalStatus::Rejected | ApprovalStatus::Cancelled => {
                    true
                }
                ApprovalStatus::Approved => {
                    // Approved but not applied - probably stale, include in cleanup
                    true
                }
            };

            if should_delete {
                to_delete.push(request.id.clone());
            }
        }

        Ok((to_delete, total_count))
    })
}

fn display_cleanup_summary(
    context: &GlobalContext,
    to_delete: &[String],
    days: i64,
    total_count: usize,
) -> Result<()> {
    context.log_info(&format!(
        "Found {} approval request(s) to clean up (out of {} total):",
        to_delete.len(),
        total_count
    ));

    // Get detailed info about requests to delete
    crate::utils::prelude::access_metadata_read_only(context, |config| {
        // Group by status
        let mut by_status = std::collections::HashMap::new();

        for request_id in to_delete {
            if let Some(request) = config.get_approval_request(request_id) {
                *by_status.entry(request.status).or_insert(0) += 1;
            }
        }

        context.log_info("");
        context.log_info("Breakdown by status:");
        for (status, count) in &by_status {
            let indicator = match status {
                ApprovalStatus::Pending => "⏳",
                ApprovalStatus::Approved => "✓",
                ApprovalStatus::Applied => "🚀",
                ApprovalStatus::Rejected => "✗",
                ApprovalStatus::Cancelled => "⏹",
            };
            context.log_info(&format!("  {} {}: {}", indicator, status, count));
        }

        context.log_info("");
        context.log_info(&format!(
            "These requests are older than {} days and will be permanently deleted.",
            days
        ));

        Ok(())
    })
}

fn confirm_cleanup(context: &GlobalContext, count: usize) -> Result<bool> {
    context.log_info("");
    context.log_info("⚠️  This action cannot be undone!");

    context.confirm(&format!("Confirm cleanup of {} requests?", count))
}

fn perform_cleanup(context: &GlobalContext, to_delete: &[String]) -> Result<usize> {
    let mut deleted_count = 0;

    modify_metadata(context, |config: &mut HitchConfig| {
        let initial_count = config.approval_requests.len();

        // Remove all requests in to_delete list
        config
            .approval_requests
            .retain(|request| !to_delete.contains(&request.id));

        deleted_count = initial_count - config.approval_requests.len();

        context.log_verbose(&format!("Removed {} requests from metadata", deleted_count));

        Ok(())
    })?;

    Ok(deleted_count)
}
