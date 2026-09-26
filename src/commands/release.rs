use crate::commands::global_context::GlobalContext;
use crate::operations::rebuild::PlanPurpose;
use crate::operations::release::{apply_release_plan, plan_release, ReleasePlanOptions};
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
}

pub fn run(args: ReleaseCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Releasing environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name, args.force)?;

    // Step 2: Resolve target branch
    let target_branch = resolve_target_branch(context, &args.env_name, args.target_branch)?;

    // Step 3: User confirmation (skip with the global --yes)
    //
    // Deliberately *before* planning. The prompt has to name the branches, and
    // naming them by reading the declaration is the only way it can be right
    // without the plan existing first — which would put a composition (real
    // merges, an anchor ref, a fetch) behind a prompt the user is still
    // deciding whether to see. The plan is built after, inside the lock, where
    // it is fresh.
    if !confirm_release(context, &args.env_name, &target_branch)? {
        context.log_info("Release cancelled by user.");
        return Ok(());
    }

    // Step 4-7: Execute release. In force mode the environment may already be
    // locked, so we don't take the environment lock; otherwise we lock it for the
    // duration of the release. The core logic is identical either way — and
    // identical to the point of *where* the plan is built, which is inside the
    // lock on both arms. `with_locked_env` commits the lock to `hitch-metadata`
    // before its closure runs, so a plan built outside the closure would be
    // stale on arrival by construction.
    if args.force {
        context.log_info(&format!(
            "Force releasing locked environment '{}' to '{}'...",
            args.env_name, target_branch
        ));
        perform_release_core(
            context,
            &args.env_name,
            &target_branch,
            ReleasePlanOptions {
                squash: args.squash,
                no_prune: args.no_prune,
                no_rebuild_dependents: args.no_rebuild_dependents,
            },
        )?;
    } else {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            perform_release_core(
                context,
                &args.env_name,
                &target_branch,
                ReleasePlanOptions {
                    squash: args.squash,
                    no_prune: args.no_prune,
                    no_rebuild_dependents: args.no_rebuild_dependents,
                },
            )
        })?;
    }

    context.log_success(&format!(
        "Environment '{}' released successfully to '{}'!",
        args.env_name, target_branch
    ));
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
fn perform_release_core(
    context: &GlobalContext,
    env_name: &str,
    target_branch: &str,
    options: ReleasePlanOptions,
) -> Result<()> {
    // A release of nothing is not a release. Kept here as an early `Ok(())`
    // rather than a plan-time refusal so the exit code stays 0 — the planner
    // refuses the same shape, and this guard is what means the degenerate case
    // is never reached from a user-facing path.
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
        return Ok(());
    }

    let mut report = |step: &str| context.log_info(step);

    let plan = plan_release(
        context,
        env_name,
        target_branch,
        options,
        PlanPurpose::Confirm,
        &mut report,
    )?;
    let _receipt = apply_release_plan(context, &plan, &mut report)?;

    Ok(())
}

/// Confirm release operation with user.
///
/// Returns `Ok(true)` if the user confirmed, `Ok(false)` if they declined.
fn confirm_release(context: &GlobalContext, env_name: &str, target_branch: &str) -> Result<bool> {
    // Get environment details to show user what will be released
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    context.log_info("🚨 DANGEROUS OPERATION DETECTED!");
    context.log_info(&format!(
        "About to release environment '{}' to '{}'",
        env_name, target_branch
    ));
    context.log_info(&format!(
        "  • {} promoted branches will be merged",
        environment.branches.len()
    ));

    if environment.branches.is_empty() {
        context.log_info("  • No branches currently promoted (empty release)");
    } else {
        context.log_info("  • Branches to be merged:");
        for branch in &environment.branches {
            context.log_info(&format!("    - {}", branch));
        }
    }

    context.log_info(&format!("  • Target branch: {}", target_branch));
    context.log_info("  • This will merge changes permanently");

    if environment.is_locked() {
        context.log_warning("  • Environment is currently locked");
    }

    // Prompt for confirmation
    if !context.confirm("Do you want to continue?")? {
        return Ok(false);
    }

    context.log_info("User confirmed release - proceeding...");
    Ok(true)
}
