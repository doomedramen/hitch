use crate::commands::global_context::GlobalContext;
use crate::types::ApprovalStatus;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RefreshArgs {
    /// Approval request ID
    pub request_id: String,
}

/// Re-snapshot an approval request with the current branch SHAs.
///
/// When a branch has received new commits after the approval request was
/// created (SHA drift), approvers can no longer approve the request because
/// `validate_snapshot` will block them.  `hitch approvals refresh <id>`:
///
/// 1. Checks that the request is still Pending.
/// 2. Captures a new snapshot of all branches (base + promoted + target branch).
/// 3. Clears all existing approvals so reviewers must re-review the new code.
/// 4. Resets the status to Pending.
///
/// The request ID is preserved so references to it remain valid.
pub fn run(args: RefreshArgs, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Read the request
    let request =
        crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;

    if request.status != ApprovalStatus::Pending {
        return Err(anyhow::anyhow!(
            "Only Pending requests can be refreshed (current status: {})",
            request.status
        ));
    }

    let env_name = request.environment.clone();
    let branch_name = request.branch.clone();

    // Show what drifted
    let drifted =
        crate::utils::snapshot::drifted_branches(context, &request.rebuild_snapshot);
    if drifted.is_empty() {
        context.log_info("No SHA drift detected — snapshot is already current.");
        return Ok(());
    }
    context.log_info("Drifted since request was created:");
    for b in &drifted {
        context.log_warning(&format!("  {}", b));
    }
    context.log_info("");

    // Re-capture snapshot
    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &env_name)?;
    let new_snapshot =
        crate::utils::snapshot::capture_rebuild_snapshot(context, &environment, &branch_name)?;

    // Update the request: new snapshot, cleared approvals, reset to Pending
    crate::utils::prelude::modify_metadata(context, |config| {
        let request = config
            .get_approval_request_mut(&args.request_id)
            .ok_or_else(|| anyhow::anyhow!("Request '{}' not found", args.request_id))?;

        request.rebuild_snapshot = new_snapshot;
        request.approvals.clear();
        request.status = ApprovalStatus::Pending;
        request.requested_at = chrono::Utc::now();

        Ok(())
    })?;

    context.log_success(&format!(
        "Request '{}' refreshed. All previous approvals have been cleared — reviewers must re-approve.",
        args.request_id
    ));

    Ok(())
}
