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
    context.log_info(&format!("Approving request {}...", args.request_id));

    // Step 1: pre_check - Ensure git repository is in good state
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Validate and process the approval
    let (request_details, environment_name, min_approvals) = validate_and_approve(context, &args)?;

    // Step 3: If threshold is met, execute the operation
    let mut executed = false;
    if request_details.threshold_met(min_approvals) {
        context.log_info("✓ Approval threshold met - executing operation");
        executed = execute_approved_operation(context, &args.request_id, &environment_name)?;
    }

    // Step 4: Show completion status
    if executed {
        context.log_success(&format!(
            "✓ Request {} approved and operation executed successfully!",
            args.request_id
        ));
    } else {
        context.log_success(&format!(
            "✓ Request {} approved successfully!",
            args.request_id
        ));
        context.log_info("Waiting for more approvals to execute the operation.");
    }

    Ok(())
}

fn validate_and_approve(
    context: &GlobalContext,
    args: &ApproveArgs,
) -> Result<(crate::types::ApprovalRequest, String, usize)> {
    // Get request details first for validation
    let request = crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
    let environment_name = request.environment.clone();

    // Get environment configuration
    let environment =
        crate::utils::prelude::get_environment_config_for_approval(context, &environment_name)?;
    let min_approvals = environment.min_approvals;

    // Validate authorization
    if !environment.is_approver(&get_current_user_email(context)?) {
        return Err(anyhow::anyhow!(
            "You are not authorized to approve requests for environment '{}'",
            environment_name
        ));
    }

    if request.requested_by == get_current_user_email(context)? {
        return Err(anyhow::anyhow!("Self-approval is not allowed"));
    }

    if request.has_approved(&get_current_user_email(context)?) {
        return Err(anyhow::anyhow!("You have already approved this request"));
    }

    if request.status != crate::types::ApprovalStatus::Pending {
        return Err(anyhow::anyhow!(
            "Cannot approve request with status: {}",
            request.status
        ));
    }

    // Validate snapshot BEFORE recording approval - if the snapshot is stale,
    // we shouldn't record an approval that can never be executed
    crate::utils::snapshot::validate_snapshot(context, &request.rebuild_snapshot)?;

    // Perform the approval with rollback protection
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote, // Use Promote as default for approval operations
        environment_name.clone(),
        request.branch.clone(),
    );

    let result = with_locked_env(context, &environment_name, || {
        modify_metadata(context, |config: &mut HitchConfig| {
            // Store the original state for rollback
            if let Some(env) = config.get_environment(&environment_name) {
                rollback_info.previous_state = Some(env.clone());
            }

            // Approve the request
            let threshold_met = crate::utils::approvals::approve_request(
                context,
                config,
                &args.request_id,
                args.comment.clone(),
            )?;

            if threshold_met {
                context.log_info("✓ Approval threshold met");
            }

            Ok(())
        })
    });

    // Handle potential errors with rollback
    match result {
        Ok(()) => {
            // Create a copy of the updated request for return and check threshold
            let updated_request =
                crate::utils::prelude::get_approval_request_by_id(context, &args.request_id)?;
            let _threshold_met = updated_request.threshold_met(min_approvals);
            Ok((updated_request, environment_name, min_approvals))
        }
        Err(e) => {
            // Attempt rollback if possible
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
    let result = with_locked_env(context, environment_name, || {
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
                context.log_verbose(&format!(
                    "✓ Added '{}' to environment '{}'",
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
            context.log_verbose(&format!(
                "✓ Removed '{}' from environment '{}'",
                request.branch, request.environment
            ));
        }
    }

    // Rebuild the environment
    crate::utils::prelude::rebuild_environment(context, &request.environment)?;

    Ok(())
}

fn get_current_user_email(context: &GlobalContext) -> Result<String> {
    crate::utils::authorization::get_current_user(context)
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
