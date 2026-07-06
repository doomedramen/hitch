use crate::commands::global_context::GlobalContext;
use crate::utils::command_helpers::{ensure_environment_exists, environment::get_locked_by_user};
use crate::utils::validation::{validate_base_branch_exists, validate_name};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct SetCommand {
    /// The environment to update
    #[arg()]
    pub env_name: String,

    /// Update the base branch for this environment
    #[arg(long)]
    pub base: Option<String>,

    /// Enable approval requirement for this environment
    #[arg(long)]
    pub requires_approval: Option<bool>,

    /// Set minimum number of approvals required
    #[arg(long)]
    pub min_approvals: Option<usize>,

    /// Add an approver email (can be specified multiple times)
    #[arg(long)]
    pub add_approver: Vec<String>,

    /// Remove an approver email (can be specified multiple times)
    #[arg(long)]
    pub remove_approver: Vec<String>,

    /// Set the complete list of approvers (replaces existing)
    #[arg(long)]
    pub set_approvers: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: SetCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Updating environment '{}'...", args.env_name));

    // Step 1: Pre-check - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Validate preconditions
    validate_preconditions(context, &args.env_name, &args)?;

    // Step 3: Check if any changes are being made
    if !has_changes(&args) {
        context.log_warning("No changes specified. Use --help to see available options.");
        return Ok(());
    }

    // Step 4: Show what will change and confirm
    if !args.force && !show_changes(context, &args.env_name, &args)? {
        context.log_info("Update cancelled by user.");
        return Ok(());
    }

    // Step 5: Apply the changes
    apply_changes(context, &args)?;

    context.log_success(&format!(
        "Successfully updated environment '{}'!",
        args.env_name
    ));
    Ok(())
}

/// Check if any changes are specified
fn has_changes(args: &SetCommand) -> bool {
    args.base.is_some()
        || args.requires_approval.is_some()
        || args.min_approvals.is_some()
        || !args.add_approver.is_empty()
        || !args.remove_approver.is_empty()
        || !args.set_approvers.is_empty()
}

/// Validate that environment is ready for update
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
    args: &SetCommand,
) -> Result<()> {
    context.log_verbose("Validating set preconditions...");

    // Validate environment name
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    ensure_environment_exists(context, env_name)?;

    // Validate base branch if provided
    if let Some(ref base) = args.base {
        validate_name(base, "Base branch")?;
        validate_base_branch_exists(context, base)?;
    }

    // Validate min_approvals if provided
    if let Some(min_approvals) = args.min_approvals {
        if min_approvals == 0 {
            return Err(anyhow::anyhow!("Minimum approvals must be at least 1"));
        }
    }

    // Validate approver email formats
    for email in &args.add_approver {
        if !email.contains('@') || !email.contains('.') {
            return Err(anyhow::anyhow!(
                "Invalid email format for approver: {}",
                email
            ));
        }
    }
    for email in &args.set_approvers {
        if !email.contains('@') || !email.contains('.') {
            return Err(anyhow::anyhow!(
                "Invalid email format for approver: {}",
                email
            ));
        }
    }

    context.log_verbose(&format!("✓ Set validation passed for '{}'", env_name));
    Ok(())
}

/// Show what changes will be made and confirm with user.
///
/// Returns `Ok(true)` if the user confirmed, `Ok(false)` if they declined.
fn show_changes(context: &GlobalContext, env_name: &str, args: &SetCommand) -> Result<bool> {
    use std::io::{self, Write};

    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = &config.environments[env_name];

    context.log_info("📋 Environment Update Preview");
    context.log_info(&format!("Environment: {}", env_name));
    context.log_info(&format!("  Current base: {}", environment.base));
    context.log_info(&format!(
        "  Current requires_approval: {}",
        environment.requires_approval
    ));
    context.log_info(&format!(
        "  Current min_approvals: {}",
        environment.min_approvals
    ));
    context.log_info(&format!(
        "  Current approvers: {}",
        if environment.approvers.is_empty() {
            "(none)".to_string()
        } else {
            environment.approvers.join(", ")
        }
    ));

    context.log_info("\n📝 Changes:");

    if let Some(ref base) = args.base {
        context.log_info(&format!("  • Base branch: {} → {}", environment.base, base));
    }

    if let Some(requires_approval) = args.requires_approval {
        context.log_info(&format!(
            "  • Requires approval: {} → {}",
            environment.requires_approval, requires_approval
        ));
    }

    if let Some(min_approvals) = args.min_approvals {
        context.log_info(&format!(
            "  • Min approvals: {} → {}",
            environment.min_approvals, min_approvals
        ));
    }

    if !args.add_approver.is_empty() {
        context.log_info(&format!(
            "  • Add approvers: +{}",
            args.add_approver.join(", +")
        ));
    }

    if !args.remove_approver.is_empty() {
        context.log_info(&format!(
            "  • Remove approvers: -{}",
            args.remove_approver.join(", -")
        ));
    }

    if !args.set_approvers.is_empty() {
        context.log_info(&format!(
            "  • Set approvers: [{}]",
            args.set_approvers.join(", ")
        ));
    }

    // Check if environment is locked
    if environment.is_locked() {
        context.log_warning(&format!(
            "  • Environment is currently locked by {}",
            get_locked_by_user(context, env_name)?
        ));
    }

    // Prompt for confirmation
    print!("\nDo you want to apply these changes? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim().to_lowercase();
    if input != "y" && input != "yes" {
        return Ok(false);
    }

    context.log_info("User confirmed update - proceeding...");
    Ok(true)
}

/// Apply the changes to the environment
fn apply_changes(context: &GlobalContext, args: &SetCommand) -> Result<()> {
    context.log_verbose(&format!(
        "Applying changes to environment '{}'...",
        args.env_name
    ));

    crate::utils::prelude::modify_metadata(context, |config| {
        // Warn if the approval policy is being changed while requests are in flight.
        // Approver-list changes take effect immediately (a removed approver's prior
        // approval still counts; a newly-added approver can approve), while each
        // request keeps the min-approvals threshold it was created with.
        let changing_approval_policy = args.min_approvals.is_some()
            || args.requires_approval.is_some()
            || !args.add_approver.is_empty()
            || !args.remove_approver.is_empty()
            || !args.set_approvers.is_empty();
        if changing_approval_policy {
            let pending = config
                .get_approval_requests_for_env(&args.env_name)
                .into_iter()
                .filter(|r| r.status == crate::types::ApprovalStatus::Pending)
                .count();
            if pending > 0 {
                context.log_warning(&format!(
                    "{} pending approval request(s) exist for '{}'. Approver-list changes apply \
                     immediately, but each request keeps the approval threshold it was created \
                     with. Review them with: hitch approvals list --status pending",
                    pending, args.env_name
                ));
            }
        }

        let environment = config
            .get_environment_mut(&args.env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", args.env_name))?;

        // Update base branch
        if let Some(ref base) = args.base {
            // If the new base branch is in the promoted branches list, remove it
            if environment.branches.contains(base) {
                environment.branches.retain(|b| b != base);
                context.log_verbose(&format!(
                    "  ✓ Removed '{}' from promoted branches (now base)",
                    base
                ));
            }
            environment.base = base.clone();
            context.log_verbose(&format!("  ✓ Updated base branch to '{}'", base));
        }

        // Update requires_approval
        if let Some(requires_approval) = args.requires_approval {
            environment.requires_approval = requires_approval;
            context.log_verbose(&format!(
                "  ✓ Updated requires_approval to {}",
                requires_approval
            ));
        }

        // Update min_approvals
        if let Some(min_approvals) = args.min_approvals {
            environment.min_approvals = min_approvals;
            context.log_verbose(&format!("  ✓ Updated min_approvals to {}", min_approvals));
        }

        // Add approvers
        for email in &args.add_approver {
            if !environment.approvers.contains(email) {
                environment.approvers.push(email.clone());
                context.log_verbose(&format!("  ✓ Added approver '{}'", email));
            }
        }

        // Remove approvers
        environment
            .approvers
            .retain(|e| !args.remove_approver.contains(e));
        if !args.remove_approver.is_empty() {
            context.log_verbose(&format!(
                "  ✓ Removed {} approver(s)",
                args.remove_approver.len()
            ));
        }

        // Set complete approver list (replaces existing)
        if !args.set_approvers.is_empty() {
            environment.approvers = args.set_approvers.clone();
            context.log_verbose(&format!(
                "  ✓ Set approvers to [{}]",
                environment.approvers.join(", ")
            ));
        }

        // Validate the updated environment AFTER all changes are applied
        // This allows atomic updates like enabling approval and adding approvers in one command
        if environment.requires_approval {
            // Auto-set min_approvals to 1 if not set and we're enabling approval
            if environment.min_approvals == 0 {
                environment.min_approvals = 1;
            }

            environment
                .validate_approval_config()
                .map_err(|e| anyhow::anyhow!("Invalid approval configuration: {}", e))?;
        }

        Ok(())
    })?;

    context.log_verbose(&format!(
        "✓ Environment '{}' updated successfully",
        args.env_name
    ));
    Ok(())
}
