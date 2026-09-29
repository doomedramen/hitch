use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::cleanup::{apply_cleanup_plan, plan_cleanup};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CleanupCommand {
    /// Actually delete the candidate branches.
    /// Without this flag the command only shows what would be deleted (dry-run).
    #[arg(long)]
    pub apply: bool,

    /// Limit this environment's archive refs to the sweep.
    ///
    /// Does *not* narrow which branches are protected: a branch promoted
    /// anywhere is spared regardless of scope, and so is every environment's
    /// own branch. A flag named after one environment is not a statement about
    /// the others, and a branch is a repository-wide object.
    #[arg(long)]
    pub env: Option<String>,

    /// Delete without asking.
    ///
    /// A cleanup that removes anything asks first — the same `--yes` every
    /// other destructive operation takes. Non-interactive callers (CI, a
    /// Makefile) need it; a human re-running what they just read does not.
    #[arg(long)]
    pub yes: bool,
}

pub fn run(args: CleanupCommand, context: &GlobalContext) -> Result<()> {
    // Everything else lives in the planner: which branches are prunable, which
    // archive refs are stale, and what each of those depends on. The candidate
    // rules in particular have been written in this file for the whole life of
    // the command, and `refs/hitch/state/` is the ref they got wrong once.
    let plan = plan_cleanup(context, args.env.as_deref())?;

    // Nothing to do is an *answer*, and it is an answer the empty plan cannot
    // give well: a plan whose effect list is empty renders as a headline and
    // nothing else, which is true and reads as a command that did not run. So
    // the empty case is answered in the only words that fit, and no document is
    // printed — there was no operation to document.
    if plan.detail.is_empty() {
        context.log_success("Nothing to clean up.");
        return Ok(());
    }

    if !args.apply {
        emit_plan(context, &plan)?;
        context.log_info("(preview — nothing was deleted; re-run with --apply)");
        return Ok(());
    }

    if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
        return Ok(());
    }

    // A failed delete is an owed effect in the receipt, not an `Err`: see
    // `operations::cleanup`'s header. The exit code stays 0 because a sweep
    // that removed 59 of 60 refs succeeded at what it set out to do, and a
    // non-zero code would make every caller treat a mostly-finished cleanup as
    // a failed one.
    let receipt = apply_cleanup_plan(context, &plan)?;
    emit_receipt(context, &plan, &receipt)?;
    Ok(())
}
