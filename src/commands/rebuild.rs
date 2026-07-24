use crate::commands::global_context::GlobalContext;
use crate::types::OnConflict;
use crate::utils::prelude::{
    access_metadata_read_only, preflight_compatibility_report, with_locked_env,
    CompatibilityConflict,
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
    context.log_info(&format!("Rebuilding environment '{}'...", args.env_name));

    // Step 1: Precondition checks (require clean working tree)
    crate::utils::prelude::pre_check(context)?;
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    let (base_branch, promoted_branches, env_on_conflict) =
        access_metadata_read_only(context, |config| {
            let env = config
                .environments
                .get(&args.env_name)
                .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", args.env_name))?;
            Ok((env.base.clone(), env.branches.clone(), env.on_conflict))
        })?;
    let on_conflict = args.on_conflict.unwrap_or(env_on_conflict);

    context.log_info(&format!(
        "Checking compatibility of {} promoted branches...",
        promoted_branches.len()
    ));

    if args.dry_run {
        let conflicts = preflight_compatibility_report(context, &base_branch, &promoted_branches)?;
        if conflicts.is_empty() {
            context.log_success(&format!(
                "'{}' would rebuild cleanly ({} branch{}).",
                args.env_name,
                promoted_branches.len(),
                if promoted_branches.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            return Ok(false);
        } else if on_conflict == OnConflict::Halt {
            return Err(anyhow::anyhow!(format_compatibility_report_for_rebuild(
                &args.env_name,
                &conflicts
            )));
        } else {
            context.log_warning(&format_held_report(&args.env_name, &conflicts));
            context.log_success(&format!(
                "'{}' would rebuild with {} of {} branches ({} held).",
                args.env_name,
                promoted_branches.len() - conflicts.len(),
                promoted_branches.len(),
                conflicts.len()
            ));
            return Ok(true);
        }
    }

    // Halt policy refuses up front, before locking or creating anything, if
    // any promoted branch conflicts — the original all-or-nothing behavior.
    // Eject policy makes its decision live, branch by branch, inside
    // rebuild_environment itself, using the same pinned SHAs the build uses
    // (a single source of truth, rather than a second, separately-timed
    // check that could in principle disagree with the real merge).
    //
    // Skipped when replaying: a recorded resolution may compose a conflict
    // the read-only preflight (which knows nothing about resolutions) would
    // flag, so the decision is deferred into rebuild_environment_opts, which
    // tries replay first and only then halts on a still-unresolved conflict.
    if on_conflict == OnConflict::Halt && !args.replay_resolutions {
        let conflicts = preflight_compatibility_report(context, &base_branch, &promoted_branches)?;
        if !conflicts.is_empty() {
            return Err(anyhow::anyhow!(format_compatibility_report_for_rebuild(
                &args.env_name,
                &conflicts
            )));
        }
    }

    // Step 3: Execute rebuild
    let replay = args.replay_resolutions;
    let outcome = if args.force {
        context.log_info(&format!(
            "Force rebuilding locked environment '{}'...",
            args.env_name
        ));
        crate::utils::prelude::rebuild_environment_opts(context, &args.env_name, replay)?
    } else {
        with_locked_env(context, &args.env_name, || {
            crate::utils::prelude::rebuild_environment_opts(context, &args.env_name, replay)
        })?
    };

    if args.pr_comments {
        crate::utils::pr_status::report_held_status(
            context,
            &args.env_name,
            &promoted_branches,
            &outcome.held,
        );
    }

    if !outcome.replayed.is_empty() {
        context.log_info(&format!(
            "♻️ Composed {} branch{} from recorded resolutions: {}",
            outcome.replayed.len(),
            if outcome.replayed.len() == 1 {
                ""
            } else {
                "es"
            },
            outcome.replayed.join(", ")
        ));
    }

    if outcome.held.is_empty() {
        context.log_success(&format!(
            "Environment '{}' rebuilt successfully!",
            args.env_name
        ));
        Ok(false)
    } else {
        context.log_warning(&format_held_report(&args.env_name, &outcome.held));
        context.log_success(&format!(
            "Environment '{}' rebuilt with {} branch{} held.",
            args.env_name,
            outcome.held.len(),
            if outcome.held.len() == 1 { "" } else { "es" }
        ));
        Ok(true)
    }
}

/// Format the branches excluded from a build under `OnConflict::Eject` — a
/// warning, not a failure: the rebuild itself still succeeded with the rest.
fn format_held_report(env_name: &str, held: &[CompatibilityConflict]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "⛔ '{}': {} branch{} held (excluded from this build)\n\n",
        env_name,
        held.len(),
        if held.len() == 1 { "" } else { "es" }
    ));

    for c in held {
        out.push_str(&format!(
            "  {} conflicts with {}\n",
            c.branch, c.conflicts_with
        ));
        for f in &c.conflicted_files {
            out.push_str(&format!("    {}\n", f));
        }
        out.push('\n');
    }

    out.push_str("Fix each, then rerun to bring it back in:\n");
    for c in held {
        out.push_str(&format!(
            "  git checkout {} && git rebase {}\n",
            c.branch, c.conflicts_with
        ));
    }

    out
}

fn format_compatibility_report_for_rebuild(
    env_name: &str,
    conflicts: &[CompatibilityConflict],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "✗ Cannot rebuild '{}' — compatibility check failed\n\n",
        env_name
    ));

    for c in conflicts {
        out.push_str(&format!(
            "  {} conflicts with {}\n",
            c.branch, c.conflicts_with
        ));
        for f in &c.conflicted_files {
            out.push_str(&format!("    {}\n", f));
        }
        out.push('\n');
    }

    if let [only] = conflicts {
        out.push_str(&format!("Fix {} first:\n", only.branch));
        out.push_str(&format!(
            "  git checkout {} && git rebase {}\n",
            only.branch, only.conflicts_with
        ));
    } else {
        out.push_str(&format!(
            "Fix each of the {} branches above, then retry:\n",
            conflicts.len()
        ));
        for c in conflicts {
            out.push_str(&format!(
                "  git checkout {} && git rebase {}\n",
                c.branch, c.conflicts_with
            ));
        }
    }

    out
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
