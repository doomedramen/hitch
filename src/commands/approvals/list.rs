use crate::commands::global_context::GlobalContext;
use crate::types::ApprovalStatus;
use anyhow::Result;
use clap::Args;
use serde_json;
use std::collections::HashMap;

#[derive(Args)]
pub struct ListArgs {
    /// Show all requests (default: pending only)
    #[arg(long)]
    pub all: bool,

    /// Filter by environment
    #[arg(long)]
    pub environment: Option<String>,

    /// Filter by status
    #[arg(long)]
    pub status: Option<String>,

    /// Output format (table|json)
    #[arg(long, default_value = "table")]
    pub format: String,
}

pub fn run(args: ListArgs, context: &GlobalContext) -> Result<()> {
    // Parse status filter if provided
    let status_filter = if let Some(status_str) = &args.status {
        match status_str.to_lowercase().as_str() {
            "pending" => Some(ApprovalStatus::Pending),
            "approved" => Some(ApprovalStatus::Approved),
            "applied" => Some(ApprovalStatus::Applied),
            "rejected" => Some(ApprovalStatus::Rejected),
            "cancelled" => Some(ApprovalStatus::Cancelled),
            _ => return Err(anyhow::anyhow!(
                "Invalid status '{}'. Valid statuses: pending, approved, applied, rejected, cancelled",
                status_str
            )),
        }
    } else {
        // Default to pending only unless --all is specified
        if !args.all {
            Some(ApprovalStatus::Pending)
        } else {
            None
        }
    };

    // Get approval requests
    let requests = crate::utils::prelude::get_approval_requests(
        context,
        args.environment.as_deref(),
        status_filter,
    )?;

    if requests.is_empty() {
        context.log_info("No approval requests found matching the criteria.");
        return Ok(());
    }

    // Get config to access environment min_approvals
    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;

    // Output based on format
    match args.format.to_lowercase().as_str() {
        "json" => output_json(&requests, &config, context),
        _ => output_table(&requests, &config, context),
    }
}

fn output_table(
    requests: &[crate::types::ApprovalRequest],
    config: &crate::types::HitchConfig,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info("Approval Requests:\n");

    // Calculate column widths
    let mut id_width = 10; // "ID" header
    let mut env_width = 11; // "Environment"
    let mut branch_width = 6; // "Branch"
    let mut operation_width = 9; // "Operation"
    let mut status_width = 6; // "Status"
    let mut requested_width = 8; // "Requested"
    let mut approvals_width = 8; // "Approvals"

    for request in requests {
        id_width = id_width.max(request.id.len());
        env_width = env_width.max(request.environment.len());
        branch_width = branch_width.max(request.branch.len());
        operation_width = operation_width.max(request.operation.to_string().len());
        status_width = status_width.max(request.status.to_string().len());
        requested_width = requested_width.max(request.requested_by.len());

        // Get min_approvals from environment config
        let min_approvals = config
            .get_environment(&request.environment)
            .map(|env| env.min_approvals)
            .unwrap_or(1);
        let approvals_str = format!("{}/{}", request.approval_count(), min_approvals);
        approvals_width = approvals_width.max(approvals_str.len());
    }

    // Print header
    let header = format!(
        "{:<id_width$} {:<env_width$} {:<branch_width$} {:<operation_width$} {:<status_width$} {:<requested_width$} {:<approvals_width$} {:<12}",
        "ID",
        "Environment",
        "Branch",
        "Operation",
        "Status",
        "Requested",
        "Approvals",
        "Requested At"
    );

    println!("{}", header);
    println!("{}", "-".repeat(header.len()));

    // Print each request
    for request in requests {
        let status_indicator = match request.status {
            ApprovalStatus::Pending => "⏳",
            ApprovalStatus::Approved => "✓",
            ApprovalStatus::Applied => "🚀",
            ApprovalStatus::Rejected => "✗",
            ApprovalStatus::Cancelled => "⏹",
        };

        // Get min_approvals from environment config
        let min_approvals = config
            .get_environment(&request.environment)
            .map(|env| env.min_approvals)
            .unwrap_or(1);
        let approvals_str = format!("{}/{}", request.approval_count(), min_approvals);

        println!(
            "{:<id_width$} {:<env_width$} {:<branch_width$} {:<operation_width$} {} {:<status_width$} {:<requested_width$} {:<approvals_width$} {:<12}",
            &request.id[..std::cmp::min(request.id.len(), id_width)],
            &request.environment[..std::cmp::min(request.environment.len(), env_width)],
            &request.branch[..std::cmp::min(request.branch.len(), branch_width)],
            request.operation.to_string(),
            status_indicator,
            request.status.to_string(),
            &request.requested_by[..std::cmp::min(request.requested_by.len(), requested_width)],
            approvals_str,
            request.requested_at.format("%Y-%m-%d %H:%M")
        );
    }

    println!(); // Empty line for spacing

    // Print summary
    let mut status_counts = HashMap::new();
    for request in requests {
        *status_counts.entry(request.status).or_insert(0) += 1;
    }

    context.log_info("Summary:");
    for (status, count) in status_counts {
        let indicator = match status {
            ApprovalStatus::Pending => "⏳",
            ApprovalStatus::Approved => "✓",
            ApprovalStatus::Applied => "🚀",
            ApprovalStatus::Rejected => "✗",
            ApprovalStatus::Cancelled => "⏹",
        };
        context.log_info(&format!("  {} {}: {}", indicator, status, count));
    }

    Ok(())
}

fn output_json(
    requests: &[crate::types::ApprovalRequest],
    _config: &crate::types::HitchConfig,
    context: &GlobalContext,
) -> Result<()> {
    let json_output: Vec<serde_json::Value> = requests
        .iter()
        .map(|request| {
            serde_json::json!({
                "id": request.id,
                "environment": request.environment,
                "branch": request.branch,
                "operation": request.operation.to_string(),
                "requested_by": request.requested_by,
                "requested_at": request.requested_at.to_rfc3339(),
                "status": request.status.to_string(),
                "approvals": request.approvals.iter().map(|approval| {
                    serde_json::json!({
                        "approved_by": approval.approved_by,
                        "approved_at": approval.approved_at.to_rfc3339(),
                        "comment": approval.comment
                    })
                }).collect::<Vec<_>>(),
                "rejection": request.rejection.as_ref().map(|rejection| {
                    serde_json::json!({
                        "rejected_by": rejection.rejected_by,
                        "rejected_at": rejection.rejected_at.to_rfc3339(),
                        "reason": rejection.reason
                    })
                }),
                "approval_count": request.approval_count(),
                "snapshot": {
                    "base_sha": request.rebuild_snapshot.base_sha,
                    "branch_shas": request.rebuild_snapshot.branch_shas,
                    "merge_conflicts": request.rebuild_snapshot.merge_conflicts
                }
            })
        })
        .collect();

    let json_str = serde_json::to_string_pretty(&json_output)?;
    println!("{}", json_str);

    context.log_info(&format!(
        "Exported {} approval requests in JSON format",
        requests.len()
    ));

    Ok(())
}
