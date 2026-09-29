use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_preview_note, emit_receipt, render_plan};
use crate::operations::metadata::{apply_metadata_plan, plan_set_environment, EnvironmentSet};
use crate::types::OnConflict;
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

    /// How a conflicting promoted branch is handled during rebuild: eject it
    /// and continue with the rest (default), or halt the whole rebuild
    #[arg(long)]
    pub on_conflict: Option<OnConflict>,

    /// Print the plan without applying it
    #[arg(long)]
    pub dry_run: bool,

    /// Apply the plan without asking for confirmation
    #[arg(long)]
    pub yes: bool,
}

pub fn run(args: SetCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Pre-check — a Git repository with a clean working tree.
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Preconditions. The environment's *own* state — locked, and the
    // approval configuration the resolved edit would produce — belongs to the
    // planner, so that a refusal arrives in a plan the reader can see above it
    // rather than as an error with nothing above it. See `plan_set_environment`.
    validate_name(&args.env_name, "Environment")?;
    crate::utils::command_helpers::ensure_environment_exists(context, &args.env_name)?;
    if let Some(base) = &args.base {
        validate_name(base, "Base branch")?;
        validate_base_branch_exists(context, base)?;
    }
    if let Some(0) = args.min_approvals {
        anyhow::bail!("Minimum approvals must be at least 1");
    }
    // The two flags whose *values* are themselves invalid, checked here because
    // clap has already parsed them and there is nothing for a plan to resolve:
    // an unparseable email is not a disagreement between two settings.
    for email in args.add_approver.iter().chain(&args.set_approvers) {
        if !email.contains('@') || !email.contains('.') {
            anyhow::bail!("Invalid email format for approver: {email}");
        }
    }

    let requested = EnvironmentSet {
        base: args.base.clone(),
        requires_approval: args.requires_approval,
        min_approvals: args.min_approvals,
        add_approver: args.add_approver.clone(),
        remove_approver: args.remove_approver.clone(),
        set_approvers: args.set_approvers.clone(),
        on_conflict: args.on_conflict,
    };

    if requested.is_empty() {
        context.log_warning("No changes specified. Use --help to see available options.");
        return Ok(());
    }

    // Step 3: Plan, then apply. No `with_locked_env` and no rollback; see
    // `operations::metadata`'s header for why a metadata edit needs neither.
    let plan = plan_set_environment(context, &args.env_name, &requested)?;

    if args.dry_run {
        // Previewed outside the lock, which is moot here: a metadata edit takes
        // no lock and composes nothing, so there is no lock to keep out of and
        // no composition for a preview to describe more optimistically than the
        // apply would.
        emit_plan(context, &plan)?;
        emit_preview_note(context, "nothing was changed");
        return Ok(());
    }

    // A `set` writes `hitch-metadata` and therefore can owe a push, so it asks
    // for confirmation on exactly the same condition every other mutation does
    // — see `ConfirmationRequirement::for_metadata_edit`.
    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    let receipt = apply_metadata_plan(context, &plan, &mut |_| {})?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
