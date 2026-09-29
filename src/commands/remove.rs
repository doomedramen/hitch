use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_preview_note, emit_receipt, render_plan};
use crate::operations::metadata::{apply_metadata_plan, plan_remove_environment};
use crate::utils::validation::validate_name;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RemoveCommand {
    /// The environment to remove
    #[arg()]
    pub env_name: String,

    /// Remove without asking, including from a locked environment
    #[arg(long)]
    pub force: bool,

    /// Print the plan without applying it
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: RemoveCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: pre-check() — a Git repository with a clean working tree.
    crate::utils::prelude::pre_check(context)?;

    // Step 2: A mistyped name is a usage error with no plan worth drawing. The
    // two objections a `remove` can raise that *are* about the environment — a
    // lock, and promoted branches — belong to the planner, so they arrive
    // above a plan the reader can see rather than as an error with nothing
    // above it. `plan_remove_environment` decides both, and takes `force` as an
    // input to that decision rather than letting this function overrule it.
    validate_name(&args.env_name, "Environment")?;
    crate::utils::command_helpers::ensure_environment_exists(context, &args.env_name)?;

    let plan = plan_remove_environment(context, &args.env_name, args.force)?;

    if args.dry_run {
        emit_plan(context, &plan)?;
        emit_preview_note(context, "nothing was changed");
        return Ok(());
    }

    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    let receipt = apply_metadata_plan(context, &plan)?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
