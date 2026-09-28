use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::metadata::{apply_metadata_plan, plan_add_environment};
use crate::utils::validation::{validate_base_branch_exists, validate_name};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct AddCommand {
    /// Environment name to add
    #[arg()]
    pub env_name: String,

    /// Base branch for the environment (defaults to main)
    #[arg(long)]
    base: Option<String>,

    /// Print the plan without applying it
    #[arg(long)]
    pub dry_run: bool,

    /// Apply the plan without asking for confirmation
    #[arg(long)]
    pub yes: bool,
}

pub fn run(args: AddCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: pre-check() — a Git repository with a clean working tree.
    crate::utils::prelude::pre_check(context)?;

    // Step 2: The two preconditions that are *not* about the declaration's
    // content. "It already exists" is not here: that is a refusal, and a
    // refusal raised as `Err` has no plan above it — which is the pre-P4
    // experience this programme exists to end. It belongs in
    // `plan_create_or_destroy`, and a reader who gets it sees what `dev`
    // currently is.
    validate_name(&args.env_name, "Environment")?;
    // Reading the declaration is the "is hitch set up here at all" check, and it
    // has to happen *before* the base check so an uninitialised repository says
    // `hitch init` rather than complaining that `main` is missing — which is
    // the branch the planner would have defaulted to, so the complaint would be
    // about our own default rather than about the repository.
    crate::utils::prelude::access_metadata_read_only(context, |_| Ok(()))?;
    // The default base is `main` whether or not `--base` was passed, so this
    // check resolves the same branch the plan will.
    validate_base_branch_exists(context, args.base.as_deref().unwrap_or("main"))?;

    // Step 3: Plan, then apply. No lock and no rollback; see
    // `operations::metadata`'s header for why a metadata edit needs neither.
    let plan = plan_add_environment(context, &args.env_name, args.base.as_deref())?;

    if args.dry_run {
        emit_plan(context, &plan)?;
        return Ok(());
    }

    // An `add` that needs no gate (nothing to push, nothing to confirm) makes
    // `confirm_plan` a no-op, so there is no `if` here for the caller to
    // remember to get right.
    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    let receipt = apply_metadata_plan(context, &plan, &mut |_| {})?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
