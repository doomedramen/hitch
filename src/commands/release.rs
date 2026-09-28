use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::model::{ExecutionReceipt, OperationPlan};
use crate::operations::rebuild::PlanPurpose;
use crate::operations::release::{
    apply_release_plan, plan_release, ReleasePlanDetail, ReleasePlanOptions,
};
use crate::utils::command_helpers::{
    ensure_branch_exists, ensure_environment_exists, environment::get_locked_by_user,
    logging::validation_success,
};
use crate::utils::prelude::access_metadata_read_only;
use crate::utils::validation::validate_name;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ReleaseCommand {
    /// The name of the environment to release
    #[arg()]
    pub env_name: String,

    /// Target branch to merge to (overrides environment base branch)
    #[arg()]
    pub target_branch: Option<String>,

    /// Override the environment lock and approval requirements.
    /// This does NOT skip the confirmation prompt — use the global --yes for that.
    #[arg(long)]
    pub force: bool,

    /// Skip post-release pruning of promoted branches that are now integrated into their bases
    #[arg(long)]
    pub no_prune: bool,

    /// Skip rebuilding environments that depend on the released target branch (and any pruned envs)
    #[arg(long)]
    pub no_rebuild_dependents: bool,

    /// Use squash merges instead of merge commits (does NOT preserve ancestry for stacked branches;
    /// GitHub will NOT auto-detect merged PRs in squash mode — use --no-ff (default) for GitHub PR workflows)
    #[arg(long)]
    pub squash: bool,

    /// Show the plan — which branches would merge, into what, what would be
    /// tagged, pruned, and rebuilt — and stop. Changes nothing: no composition
    /// is anchored, no lock is taken, no writes.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: ReleaseCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name, args.force)?;

    // Step 2: Resolve target branch
    let target_branch = resolve_target_branch(context, &args.env_name, args.target_branch)?;

    let options = ReleasePlanOptions {
        squash: args.squash,
        no_prune: args.no_prune,
        no_rebuild_dependents: args.no_rebuild_dependents,
    };

    if args.dry_run {
        // Preview, planned outside the environment lock — see `promote::run`
        // for why that inversion of the real path's rule is safe, and what would
        // have to change for it to stop being safe.
        //
        // The old prompt asked the user to confirm *before* the plan existed,
        // and spelled the branch list out by hand from the declaration. That
        // list is what the plan's Composition section now says, with the SHAs
        // it will actually merge and the target it will merge them into — so the
        // prompt no longer has to be right about a fact it read separately, and
        // cannot be wrong about it in a second vocabulary.
        if nothing_to_release(context, &args.env_name)? {
            return Ok(());
        }
        let plan = plan_release(
            context,
            &args.env_name,
            &target_branch,
            options,
            PlanPurpose::Preview,
            &mut |step| context.log_verbose(step),
        )?;
        emit_plan(context, &plan)?;
        return Ok(());
    }

    // Step 3-4: Execute release. In force mode the environment may already be
    // locked, so we don't take the environment lock; otherwise we lock it for the
    // duration of the release. The core logic is identical either way — and
    // identical to the point of *where* the plan is built, which is inside the
    // lock on both arms. `with_locked_env` commits the lock to `hitch-metadata`
    // before its closure runs, so a plan built outside the closure would be
    // stale on arrival by construction.
    let run = if args.force {
        context.log_info(&format!(
            "Force releasing locked environment '{}' to '{}'...",
            args.env_name, target_branch
        ));
        perform_release_core(context, &args.env_name, &target_branch, options)?
    } else {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            perform_release_core(context, &args.env_name, &target_branch, options)
        })?
    };

    let Some(run) = run else {
        // Declined at the gate. Nothing was written — including no tag, since
        // the tag rides the plan's own transaction — so there is nothing to
        // report and nothing to undo. Exit 0.
        return Ok(());
    };

    emit_receipt(context, &run.plan, &run.receipt)?;
    Ok(())
}

/// Validate that environment exists and is ready for release
fn validate_preconditions(context: &GlobalContext, env_name: &str, force: bool) -> Result<()> {
    context.log_verbose("Running release validation...");

    // Basic pre-checks
    crate::utils::prelude::pre_check(context)?;

    // Validate environment name
    validate_name(env_name, "Environment")?;

    // Check environment exists
    ensure_environment_exists(context, env_name)?;

    // Check environment lock status (unless force)
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = &config.environments[env_name];

    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by {}. Use --force to override.",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    // Releasing an approval-gated environment merges its promoted branches into a
    // real target/deploy branch. The per-promote approval workflow does not cover
    // release, so require an explicit --force to acknowledge that the release is
    // not itself approval-gated.
    if environment.requires_approval && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' requires approval. Releasing it merges its promoted branches \
             into the target branch and is NOT covered by the per-promote approval workflow.\n\
             Re-run with --force to release anyway.",
            env_name
        ));
    }

    validation_success(context, env_name, "Release validation");
    Ok(())
}

/// Resolve the target branch for release (use override or environment base)
fn resolve_target_branch(
    context: &GlobalContext,
    env_name: &str,
    target_override: Option<String>,
) -> Result<String> {
    context.log_verbose("Resolving target branch for release...");

    // Validate target branch name if provided as override
    if let Some(ref target) = target_override {
        validate_name(target, "Target branch")?;
    }

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    let target = target_override.unwrap_or_else(|| environment.base.clone());

    context.log_verbose(&format!("Target branch resolved to: '{}'", target));

    // Use the existing branch validation helper
    ensure_branch_exists(context, &target)?;

    Ok(target)
}

/// A release that planned and applied, with both halves handed back.
struct ReleaseRun {
    plan: OperationPlan<ReleasePlanDetail>,
    receipt: ExecutionReceipt,
}

/// Whether there is nothing to release, said once so both paths say it once.
///
/// A release of nothing is not a release. Kept as an early return rather than a
/// plan-time refusal so the exit code stays 0, and shared between the preview and
/// the real path so a dry run of an empty environment cannot describe something
/// the real run would refuse. The cost is that a `--json` dry run of an empty
/// release prints a diagnostic and no document — stdout is empty, which is a
/// valid "nothing to do", and the alternative was a plan whose every list is
/// empty, which says strictly less.
fn nothing_to_release(context: &GlobalContext, env_name: &str) -> Result<bool> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let promoted = config
        .environments
        .get(env_name)
        .map(|e| e.branches.len())
        .unwrap_or(0);
    if promoted == 0 {
        context.log_info(&format!(
            "No branches promoted to environment '{}', nothing to release",
            env_name
        ));
        return Ok(true);
    }
    Ok(false)
}

/// Plan the release, then land it.
///
/// Everything this used to do inline — synchronise, pin, compose, anchor, tag,
/// publish, tag-push, prune, dependent rebuilds — now lives in
/// `operations::release`, split across a planner and an executor so that the
/// "what will happen" and the "what happened" cannot be produced by the same
/// code. What is left here is only the two facts the planner cannot work out
/// for itself: that the user confirmed, and that the environment is locked.
///
/// The confirmation and the lock are *not* re-checked by the planner. The lock
/// cannot be — by plan time the environment is always locked, since
/// `with_locked_env` committed it before this closure. The human-lock refusal
/// stays in `validate_preconditions`, before the lock, where it can still
/// produce "locked by <user>" rather than the planner's "locked by hitch".
///
/// `Ok(None)` is a declined gate. The release's composed commit was anchored
/// under `refs/hitch/release/*` by the `Confirm`-purpose plan, and nothing
/// prunes that family, so the decline path discards the anchor itself; that is
/// the one exit from the plan's lifetime that `apply_release_plan` does not own.
fn perform_release_core(
    context: &GlobalContext,
    env_name: &str,
    target_branch: &str,
    options: ReleasePlanOptions,
) -> Result<Option<ReleaseRun>> {
    if nothing_to_release(context, env_name)? {
        return Ok(None);
    }

    // Narrated to stdout on neither arm. The dry-run already went quiet here
    // (`log_verbose`) and the apply went quiet at its own call site; the plan's
    // Composition section says `✓ <branch> at <sha>` for each merge, in
    // declaration order, which is the same list these lines carried in a second
    // vocabulary. The one thing that genuinely needed saying before the plan —
    // which environment is releasing to which target — is the plan's title,
    // printed immediately below.
    let plan = plan_release(
        context,
        env_name,
        target_branch,
        options,
        PlanPurpose::Confirm,
        &mut |step| context.log_verbose(step),
    )?;

    // The discard precedes the return on *both* non-applying arms. Writing it as
    // `if !confirm_plan(...)?` discarded on a decline and not on an error, so
    // `--json` without `--yes` — where `decide_gate` refuses instead of
    // prompting — propagated the refusal and left the anchor behind. Nothing
    // prunes `refs/hitch/release/*`, so that is one leaked ref per refusal.
    let approved = match confirm_plan(context, &render_plan(&plan), &plan.confirmation) {
        Ok(approved) => approved,
        Err(error) => {
            crate::operations::release::discard_release_plan(context, &plan);
            return Err(error);
        }
    };
    if !approved {
        crate::operations::release::discard_release_plan(context, &plan);
        return Ok(None);
    }

    // No narration here, exactly as in `promote::run` / `demote::run`. Every
    // step the apply would narrate — tagging, publishing, updating metadata,
    // pruning, rebuilding dependents — is a line in the receipt printed a few
    // lines below, in the same vocabulary as the plan the user just read.
    let receipt = apply_release_plan(context, &plan, &mut |_| {})?;
    Ok(Some(ReleaseRun { plan, receipt }))
}
