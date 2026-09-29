use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_receipt, render_plan};
use crate::operations::metadata::{apply_metadata_plan, plan_unlock};
use crate::utils::validation::{validate_environment_exists, validate_name};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct UnlockCommand {
    /// The environment to unlock
    #[arg()]
    pub env_name: String,
}

pub fn run(args: UnlockCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Precondition checks, and only those that are not about the lock.
    // "Not locked" and "locked by someone else" are the planner's business, so
    // that both arrive in a plan the reader can see above the refusal. See
    // `plan_unlock`.
    validate_name(&args.env_name, "Environment")?;
    validate_environment_exists(context, &args.env_name)?;

    let plan = plan_unlock(context, &args.env_name)?;

    // No confirmation, for the same reason `hitch lock` takes none.
    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    let receipt = apply_metadata_plan(context, &plan)?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
