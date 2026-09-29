use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::model::OperationOutcome;
use crate::operations::rebuild::{plan_rebuild, PlanPurpose, RebuildPlanOptions};
use crate::types::OnConflict;
use crate::utils::prelude::{
    access_metadata_read_only, rebuild_environment_gated, with_locked_env,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RebuildCommand {
    /// The name of the environment to rebuild
    #[arg()]
    pub env_name: String,

    /// Force rebuild even if environment is locked
    #[arg(long)]
    pub force: bool,

    /// Show every branch that would compose cleanly and every branch that
    /// would conflict (and with what), without building, locking, or
    /// publishing anything
    #[arg(long)]
    pub dry_run: bool,

    /// Override the environment's on_conflict policy for this run: eject
    /// conflicting branches and continue with the rest (default), or halt
    /// the whole rebuild on the first conflict
    #[arg(long)]
    pub on_conflict: Option<OnConflict>,

    /// Report each promoted branch's held/re-included status as a comment
    /// on its GitHub PR (best-effort: silently does nothing without `gh`,
    /// auth, or an open PR — never fails the rebuild). Off by default so a
    /// plain rebuild never depends on GitHub.
    #[arg(long)]
    pub pr_comments: bool,

    /// Before holding a conflicting branch, try to compose it from a
    /// recorded resolution matching the exact conflict (see `hitch resolve
    /// --record` and `hitch resolutions`). Opt-in per run — this flag can't
    /// live in HITCH_YES, so it is itself the authorization to apply
    /// someone's recorded content. Without `--yes` each distinct resolution
    /// is confirmed once; under `--yes` (CI) every application is logged.
    #[arg(long)]
    pub replay_resolutions: bool,
}

/// Runs the rebuild. Returns `Ok(true)` if it succeeded but held one or more
/// conflicting branches (or, under `--dry-run`, would have) — the caller
/// uses this to choose a distinct "succeeded with holds" exit code (2)
/// instead of the plain-success 0, so CI can warn without failing the build.
/// `Ok(false)` is a fully clean success; `Err` covers both a halt-policy
/// refusal and any other failure (both exit 1, matching prior behavior).
pub fn run(args: RebuildCommand, context: &GlobalContext) -> Result<bool> {
    // Step 1: Precondition checks (require clean working tree)
    crate::utils::prelude::pre_check(context)?;
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    // Only the branch list is needed here. The environment's `on_conflict` and
    // the repo's `require_signed_resolutions` are both read by the planner
    // itself, from the same config read it composes against — threading them
    // through the command would be a second copy of the declaration to keep in
    // step with the one the planner actually uses.
    let environment = access_metadata_read_only(context, |config| {
        config
            .environments
            .get(&args.env_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", args.env_name))
    })?;
    let promoted_branches = environment.branches.clone();

    context.log_info(&format!(
        "Checking compatibility of {} promoted branch{}...",
        promoted_branches.len(),
        if promoted_branches.len() == 1 {
            ""
        } else {
            "es"
        }
    ));

    let options = RebuildPlanOptions {
        replay: args.replay_resolutions,
        on_conflict: args.on_conflict,
    };

    if args.dry_run {
        // Preview by planning, not by composing separately. Both paths call
        // `plan_rebuild`, which calls `compose_environment` — the same one
        // implementation, over the same pinned inputs, so a preview cannot
        // disagree with the build it previews.
        //
        // `preflight_compatibility_report` used to answer this question here,
        // and it could not see recorded resolutions — so `rebuild dev
        // --dry-run --replay-resolutions` reported branches as held that replay
        // would have composed. It also went through `merge-tree
        // --write-tree-name-only` with a hand-passed `--merge-base`, i.e. a
        // different door into the merge engine than the build.
        //
        // `PlanPurpose::Preview` is what keeps the preview offline and
        // side-effect-free, and it is also why planning here happens *outside*
        // the environment lock, inverting P5's rule. That inversion is safe for
        // one reason only: **a preview plan is never applied, so nothing can go
        // stale between planning and applying it.** The rule is safe because
        // the planner does not consult `is_locked()` — which is why the human
        // lock refusal above, and not the planner, is what enforces it. Were
        // the planner ever to gate on the lock, a preview would have to take the
        // lock too, and a read-only command would start mutating metadata.
        let plan = plan_rebuild(context, &args.env_name, options, PlanPurpose::Preview)?;

        emit_plan(context, &plan)?;

        // Exit 2 for "would hold", matching a real rebuild that held: the
        // signal is about the composition, and a preview that cannot express
        // it would make `--dry-run` a worse predictor than it is meant to be.
        return Ok(!plan.detail.held.is_empty());
    }

    // No pre-check here. `compose_environment` makes the conflict decision
    // in-loop, over the same pinned SHAs the build consumes, and that is the
    // only place it is made — for the dry-run and the real run alike. The
    // pre-check that used to live at this point was a second opinion from a
    // different implementation, which is precisely what could disagree with the
    // merge that followed it.
    //
    // The gate is the caller's, and it sees the finished plan — which is the
    // point of a plan. `hitch rebuild` asks about it; the nested rebuilds inside
    // promote/demote/release do not, because the user already answered the
    // question that produced them.
    let gate =
        |plan: &crate::operations::model::OperationPlan<
            crate::operations::rebuild::RebuildPlanDetail,
        >|
         -> Result<bool> { confirm_plan(context, &render_plan(plan), &plan.confirmation) };

    let run = if args.force {
        context.log_info(&format!(
            "Force rebuilding locked environment '{}'...",
            args.env_name
        ));
        rebuild_environment_gated(
            context,
            &args.env_name,
            options.replay,
            options.on_conflict,
            gate,
        )?
    } else {
        with_locked_env(context, &args.env_name, || {
            rebuild_environment_gated(
                context,
                &args.env_name,
                options.replay,
                options.on_conflict,
                gate,
            )
        })?
    };

    let Some(run) = run else {
        // The gate already showed the plan, and it already logged the answer.
        // Nothing was written, so there is nothing further to report and — more
        // to the point — no reason to print a "rolled back" line for a no-op.
        // Exit 0: declining is not a failure.
        return Ok(false);
    };

    if args.pr_comments {
        crate::utils::pr_status::report_held_status(
            context,
            &args.env_name,
            &promoted_branches,
            &run.plan.detail.held,
        );
    }

    emit_receipt(context, &run.plan, &run.receipt)?;

    // Exit 2 for holds, and only for holds. `Ok(true)` here is the CI contract:
    // the build succeeded with the rest, and a pipeline should be able to warn
    // without failing. The typed form of the same fact is the receipt's
    // `OperationOutcome::AppliedWithHolds`, and this reads it rather than
    // re-deriving it from `detail.held` — two places computing one verdict is
    // how the exit code and the rendered outcome drift apart.
    Ok(run.receipt.outcome == OperationOutcome::AppliedWithHolds)
}

/// Validate that environment exists and is not locked (unless force flag is used)
fn validate_environment_exists_and_unlocked(
    context: &GlobalContext,
    env_name: &str,
    force: bool,
) -> Result<()> {
    context.log_verbose("Validating environment preconditions...");

    // Check if environment exists
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    let environment = &config.environments[env_name];

    // Check if environment is locked (unless force is used)
    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by {}. Use --force to override.",
            env_name,
            environment
                .locked_by
                .as_ref()
                .unwrap_or(&"unknown".to_string())
        ));
    }

    context.log_verbose(&format!("✓ Environment '{}' validation passed", env_name));
    Ok(())
}
