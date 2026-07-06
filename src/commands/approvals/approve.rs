use crate::commands::global_context::GlobalContext;
use crate::types::{HitchConfig, RollbackInfo, RollbackOperation};
use crate::utils::prelude::{modify_metadata, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ApproveArgs {
    /// Approval request ID
    pub request_id: String,

    /// Approval comment (optional)
    #[arg(long)]
    pub comment: Option<String>,
}

pub fn run(args: ApproveArgs, context: &GlobalContext) -> Result<()> {
    use crate::types::ApprovalStatus;

    context.log_info(&format!("Approving request {}...", args.request_id));

    // Step 1: pre_check - Ensure git repository is in good state
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Resolve the request (accepts a full ID or an unambiguous prefix) and
    // branch on its current status.
    context.log_info("");
    context.log_info("Fetching approval request...");
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
    let request_id = request.id.clone();
    let environment_name = request.environment.clone();
    context.log_info(&format!("  ✓ Request found: {}", short_id(&request_id)));

    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &environment_name)?;
    let min_approvals = request.required_approvals(&environment);

    match request.status {
        ApprovalStatus::Applied => {
            return Err(anyhow::anyhow!(
                "Request {} has already been applied — nothing to do.",
                request_id
            ));
        }
        ApprovalStatus::Rejected => {
            return Err(anyhow::anyhow!(
                "Request {} was rejected and can no longer be approved.",
                request_id
            ));
        }
        ApprovalStatus::Cancelled => {
            return Err(anyhow::anyhow!(
                "Request {} was cancelled and can no longer be approved.",
                request_id
            ));
        }
        ApprovalStatus::Approved => {
            // The threshold was already met but the operation was never applied
            // (e.g. a previous apply was interrupted). Re-drive execution so an
            // approved request isn't a dead end.
            context.log_info("");
            context.log_info("Request already meets its approval threshold; applying it now...");
            let executed = execute_approved_operation(context, &request_id, &environment_name)?;
            context.log_info("");
            if executed {
                context.log_success(&format!("✓ Request {} applied successfully!", request_id));
            }
            return Ok(());
        }
        ApprovalStatus::Pending => { /* normal approval flow below */ }
    }

    // Step 3: Record this approval (validation happens under the lock).
    let request_details = validate_and_approve(context, &args, &request_id, &environment_name)?;

    // Step 4: If the (frozen) threshold is now met, execute the operation.
    let mut executed = false;
    if request_details.threshold_met(min_approvals) {
        context.log_info("");
        context.log_info(&format!(
            "Approvals: {}/{} - Threshold met!",
            request_details.approvals.len(),
            min_approvals
        ));
        context.log_info("");
        context.log_info(&format!(
            "Executing {}...",
            if request_details.operation == crate::types::Operation::Promote {
                "promotion"
            } else {
                "demotion"
            }
        ));
        executed = execute_approved_operation(context, &request_id, &environment_name)?;
    }

    // Step 5: Show completion status
    context.log_info("");
    if executed {
        context.log_success(&format!(
            "✓ Request {} approved and operation executed successfully!",
            request_id
        ));
    } else {
        context.log_success(&format!("✓ Request {} approved successfully!", request_id));
        context.log_info(&format!(
            "Waiting for {} more approval(s) to execute the operation.",
            min_approvals.saturating_sub(request_details.approvals.len())
        ));
    }

    Ok(())
}

/// First 8 characters of an ID for display (safe on short/odd IDs).
fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// Record the current user's approval for a Pending request.
///
/// All authorization and freshness checks (approver membership, no self-approval,
/// no double-approval, status == Pending, snapshot unchanged) are performed on the
/// locked, freshly-read config inside `approve_request`/`validate_snapshot` — this
/// function deliberately does NOT re-check them on a stale pre-lock clone.
fn validate_and_approve(
    context: &GlobalContext,
    args: &ApproveArgs,
    request_id: &str,
    environment_name: &str,
) -> Result<crate::types::ApprovalRequest> {
    context.log_info("");
    context.log_info("Recording approval...");

    let branch = crate::utils::prelude::get_approval_request_by_id(context, request_id)?
        .branch
        .clone();

    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote, // Use Promote as default for approval operations
        environment_name.to_string(),
        branch,
    );

    let result = with_locked_env(context, environment_name, || {
        modify_metadata(context, |config: &mut HitchConfig| {
            // Store the original environment state for rollback.
            if let Some(env) = config.get_environment(environment_name) {
                rollback_info.previous_state = Some(env.clone());
            }

            // Validate snapshot freshness on the locked, freshly-read request
            // before recording an approval that could never execute.
            let snapshot = crate::utils::approvals::find_approval_request(config, request_id)?
                .rebuild_snapshot
                .clone();
            crate::utils::snapshot::validate_snapshot(context, &snapshot)?;

            // Record the approval. This performs authorization on the fresh config.
            let threshold_met = crate::utils::approvals::approve_request(
                context,
                config,
                request_id,
                args.comment.clone(),
            )?;

            context.log_info("  ✓ Approval recorded");

            let updated_request = config
                .get_approval_request(request_id)
                .ok_or_else(|| anyhow::anyhow!("Request not found after approval"))?;
            let required = updated_request.required_approvals(
                config
                    .get_environment(environment_name)
                    .ok_or_else(|| anyhow::anyhow!("Environment not found"))?,
            );

            context.log_info("");
            if threshold_met {
                context.log_info(&format!(
                    "Approvals: {}/{} - Threshold met!",
                    updated_request.approvals.len(),
                    required
                ));
            } else {
                context.log_info(&format!(
                    "Approvals: {}/{}",
                    updated_request.approvals.len(),
                    required
                ));
                context.log_info(&format!(
                    "Waiting for {} more approval(s)",
                    required.saturating_sub(updated_request.approvals.len())
                ));
            }

            Ok(())
        })
    });

    match result {
        Ok(()) => Ok(crate::utils::prelude::get_approval_request_by_id(
            context, request_id,
        )?),
        Err(e) => {
            if let Err(rollback_err) = attempt_approval_rollback(context, &rollback_info) {
                context.log_error(&format!(
                    "CRITICAL: Failed to rollback approval changes: {}",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}

fn execute_approved_operation(
    context: &GlobalContext,
    request_id: &str,
    environment_name: &str,
) -> Result<bool> {
    context.log_verbose(&format!(
        "Executing operation for approved request {}",
        request_id
    ));

    // Create rollback info for the actual operation
    let request = crate::utils::prelude::get_approval_request_by_id(context, request_id)?;
    let mut rollback_info = RollbackInfo::new(
        match request.operation {
            crate::types::Operation::Promote => RollbackOperation::Promote,
            crate::types::Operation::Demote => RollbackOperation::Demote,
        },
        environment_name.to_string(),
        request.branch.to_string(),
    );

    // Execute the operation with rollback protection
    context.log_info(&format!(
        "  ⏳ Locking environment '{}'...",
        environment_name
    ));
    let result = with_locked_env(context, environment_name, || {
        context.log_info(&format!("  ✓ Environment '{}' locked", environment_name));

        context.log_info("");
        context.log_info("  ⏳ Updating environment metadata...");

        modify_metadata(context, |config| {
            // Store current environment state for rollback
            if let Some(env) = config.get_environment(environment_name) {
                rollback_info.previous_state = Some(env.clone());
            }

            // Validate snapshot before execution
            let request_for_validation =
                crate::utils::prelude::get_approval_request_by_id(context, request_id)?;
            crate::utils::snapshot::validate_snapshot(
                context,
                &request_for_validation.rebuild_snapshot,
            )?;

            // Execute the approved operation
            execute_operation_based_on_request(context, config, request_id)?;

            // Mark request as applied
            crate::utils::approvals::mark_request_applied(config, request_id)?;

            Ok(())
        })
    });

    match result {
        Ok(()) => {
            context.log_info(&format!("  ✓ Environment '{}' unlocked", environment_name));
            context.log_verbose("✓ Operation executed successfully");
            Ok(true)
        }
        Err(e) => {
            // Attempt rollback
            if let Err(rollback_err) = attempt_operation_rollback(context, &rollback_info) {
                context.log_error(&format!(
                    "CRITICAL: Failed to rollback operation: {}",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}

fn execute_operation_based_on_request(
    context: &GlobalContext,
    config: &mut crate::types::HitchConfig,
    request_id: &str,
) -> Result<()> {
    let request = crate::utils::prelude::get_approval_request_by_id(context, request_id)?;

    match request.operation {
        crate::types::Operation::Promote => {
            // Add branch to environment
            let environment = config
                .get_environment_mut(&request.environment)
                .ok_or_else(|| {
                    anyhow::anyhow!("Environment '{}' not found", request.environment)
                })?;

            if !environment.branches.contains(&request.branch) {
                environment.add_branch(request.branch.clone());
                context.log_info(&format!(
                    "  ✓ Branch '{}' added to environment '{}'",
                    request.branch, request.environment
                ));
            } else {
                context.log_verbose(&format!(
                    "Branch '{}' already in environment '{}'",
                    request.branch, request.environment
                ));
            }
        }
        crate::types::Operation::Demote => {
            // Remove branch from environment
            let environment = config
                .get_environment_mut(&request.environment)
                .ok_or_else(|| {
                    anyhow::anyhow!("Environment '{}' not found", request.environment)
                })?;

            environment.remove_branch(&request.branch);
            context.log_info(&format!(
                "  ✓ Branch '{}' removed from environment '{}'",
                request.branch, request.environment
            ));
        }
    }

    // Rebuild the environment
    crate::utils::prelude::rebuild_environment(context, &request.environment)?;

    Ok(())
}

fn attempt_approval_rollback(context: &GlobalContext, rollback_info: &RollbackInfo) -> Result<()> {
    context.log_warning("Attempting to rollback approval changes...");

    modify_metadata(context, |config| {
        if let (Some(previous_state), Some(env)) = (
            &rollback_info.previous_state,
            config.get_environment_mut(&rollback_info.env_name),
        ) {
            *env = previous_state.clone();
            context.log_info("✓ Restored previous environment state");
        }
        Ok(())
    })
}

fn attempt_operation_rollback(context: &GlobalContext, rollback_info: &RollbackInfo) -> Result<()> {
    context.log_warning("Attempting to rollback operation...");

    modify_metadata(context, |config| {
        if let (Some(previous_state), Some(env)) = (
            &rollback_info.previous_state,
            config.get_environment_mut(&rollback_info.env_name),
        ) {
            *env = previous_state.clone();
            context.log_info("✓ Restored previous environment state");
        }
        Ok(())
    })
}
