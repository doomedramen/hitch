use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_receipt, render_plan};
use crate::operations::metadata::{apply_metadata_plan, plan_lock};
use crate::utils::validation::{validate_environment_exists, validate_name};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct LockCommand {
    /// The environment to lock
    #[arg()]
    pub env_name: String,
}

pub fn run(args: LockCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Precondition checks. Only the two that are *not* about the lock
    // itself — a name that is not a name, and an environment that does not
    // exist. The lock's own preconditions belong to the planner, which is what
    // puts them in a plan above the refusal rather than in an error with
    // nothing above it; see `plan_lock`.
    validate_name(&args.env_name, "Environment")?;
    validate_environment_exists(context, &args.env_name)?;

    // Step 2: Plan, then apply. No `with_locked_env`: the environment lock is
    // what this command *is*, and taking it through the mechanism meant to
    // protect an environment would have `hitch lock dev` deadlock on its own
    // pre-check. No rollback either, and none is needed — the apply is one
    // `modify_metadata` closure that runs before the write. See
    // `operations::metadata`'s header.
    let plan = plan_lock(context, &args.env_name)?;

    // A lock asks for no confirmation: it is one keystroke, and §10.2 is
    // explicit that showing a plan is not conditional on a prompt existing.
    // `confirm_plan` therefore takes `decide_gate`'s `Proceed` arm and prints.
    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    // Step 3: Apply and report. The refusal — already locked — arrives as an
    // `Err` from here, not from a pre-check, so the plan above it is the one
    // the reader was just shown.
    let receipt = apply_metadata_plan(context, &plan)?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
