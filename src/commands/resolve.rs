use crate::commands::global_context::GlobalContext;
use crate::types::Environment;
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::{
    access_metadata_read_only, force_push_with_deploy_key_if_configured,
    preflight_compatibility_report, publish_environment_build, CompatibilityConflict,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ResolveCommand {
    /// The environment whose held branch to resolve
    #[arg()]
    pub env_name: String,

    /// Which promoted branch to resolve (required if more than one is
    /// currently held)
    #[arg(long)]
    pub branch: Option<String>,

    /// Continue an in-progress peer-conflict resolve session, after editing
    /// the conflicted files in the printed worktree and `git add`-ing them
    #[arg(long = "continue")]
    pub continue_: bool,

    /// Abort an in-progress peer-conflict resolve session, discarding it
    #[arg(long)]
    pub abort: bool,

    /// Print the in-progress resolve session's worktree path (for opening
    /// it in an editor/IDE) instead of doing anything else
    #[arg(long)]
    pub path: bool,

    /// Run `git mergetool` in the worktree when starting a peer-conflict
    /// resolve session, instead of leaving plain conflict markers
    #[arg(long)]
    pub tool: bool,

    /// On `--continue`, record this peer-conflict resolution as a
    /// content-addressed ref so a byte-identical conflict recurring on a
    /// later rebuild can be replayed instead of re-resolved (see
    /// `hitch resolutions` and `hitch rebuild --replay-resolutions`).
    /// Off by default: the durable fix is still carrying the change back
    /// into a real branch.
    #[arg(long)]
    pub record: bool,

    /// Imply `--record` and also push the recorded resolution to origin so
    /// teammates and CI can replay it. Separate from `--record` so sharing
    /// team-wide is always a deliberate, explicit act — it never happens as
    /// a side effect and never inherits the global `--yes`.
    #[arg(long)]
    pub share: bool,
}

/// Guided conflict resolution for a single held branch. Two modes,
/// auto-detected from what the branch actually conflicts with (see
/// docs/merge-conflict-handling-plan.md, phase 4):
///
/// - **Mode A** (conflicts with the environment's base): the durable fix is
///   rebasing the feature branch — hitch just kicks that off and hands
///   control to plain Git once it starts.
/// - **Mode B** (conflicts with a peer branch promoted ahead of it): neither
///   branch can own the fix alone, so hitch builds the composition up to
///   that point in a disposable worktree (never the user's own checkout),
///   lets the conflict happen for real, and — once resolved — publishes it
///   as the environment's new content via the same path `hitch rebuild`
///   uses. This is a one-time inclusion, not a persisted fix: nothing is
///   cached, so the same conflict resurfaces on the next ordinary rebuild
///   unless the change is carried back into a feature branch.
pub fn run(args: ResolveCommand, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    let config = access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let environment = config
        .environments
        .get(&args.env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", args.env_name))?
        .clone();

    let branch = resolve_target_branch(context, &args, &environment)?;
    let worktree_path = resolve_worktree_path(context, &args.env_name, &branch)?;

    if args.path {
        if !worktree_path.exists() {
            return Err(anyhow::anyhow!(
                "No resolve session in progress for '{}' in '{}'.",
                branch,
                args.env_name
            ));
        }
        println!("{}", worktree_path.display());
        return Ok(());
    }

    if args.abort {
        return abort_session(context, &worktree_path, &branch);
    }

    if args.continue_ {
        return continue_session(
            context,
            &args.env_name,
            &branch,
            &worktree_path,
            &environment,
            args.record || args.share,
            args.share,
        );
    }

    if worktree_path.exists() {
        return Err(anyhow::anyhow!(
            "A resolve session for '{}' in '{}' is already in progress.\n\
             Path: {}\n\
             Continue it: hitch resolve {} --branch {} --continue\n\
             Or abandon it: hitch resolve {} --branch {} --abort",
            branch,
            args.env_name,
            worktree_path.display(),
            args.env_name,
            branch,
            args.env_name,
            branch
        ));
    }

    let conflicts =
        preflight_compatibility_report(context, &environment.base, &environment.branches)?;
    let conflict = conflicts
        .into_iter()
        .find(|c| c.branch == branch)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "'{}' does not currently conflict in '{}' — nothing to resolve. Run 'hitch \
                 rebuild {}'.",
                branch,
                args.env_name,
                args.env_name
            )
        })?;

    if conflict.conflicts_with == environment.base {
        resolve_mode_a(
            context,
            &args.env_name,
            &branch,
            &environment.base,
            &worktree_path,
            args.tool,
        )
    } else {
        start_mode_b_session(
            context,
            &args.env_name,
            &branch,
            &conflict,
            &environment,
            &worktree_path,
            args.tool,
        )
    }
}

/// Figure out which promoted branch to operate on: the explicit `--branch`,
/// or — if there's exactly one currently held — that one. Ambiguous or
/// empty cases ask the user to be explicit rather than guessing.
fn resolve_target_branch(
    context: &GlobalContext,
    args: &ResolveCommand,
    environment: &Environment,
) -> Result<String> {
    if let Some(ref b) = args.branch {
        return Ok(b.clone());
    }

    let conflicts =
        preflight_compatibility_report(context, &environment.base, &environment.branches)?;

    match conflicts.len() {
        0 => Err(anyhow::anyhow!(
            "No branches are currently held in '{}' — nothing to resolve.",
            args.env_name
        )),
        1 => Ok(conflicts[0].branch.clone()),
        _ => {
            let names: Vec<&str> = conflicts.iter().map(|c| c.branch.as_str()).collect();
            Err(anyhow::anyhow!(
                "'{}' has {} branches held: {}. Specify which one with --branch.",
                args.env_name,
                conflicts.len(),
                names.join(", ")
            ))
        }
    }
}

/// Deterministic (non-timestamped) worktree path for a resolve session, so
/// `--continue`/`--abort`/`--path` can find it again without any persisted
/// state — the worktree's existence on disk *is* the state.
fn resolve_worktree_path(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
) -> Result<std::path::PathBuf> {
    let repo_path = std::path::PathBuf::from(context.git().workdir());
    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let parent = repo_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Repository at '{}' has no parent directory to place a resolve worktree in",
            repo_path.display()
        )
    })?;
    Ok(parent.join(format!(
        ".hitch-resolve-{}-{}-{}",
        repo_name,
        env_name,
        sanitize_for_path(branch)
    )))
}

fn sanitize_for_path(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

// ── Session marker ─────────────────────────────────────────────────

/// What kind of resolve session a worktree holds, stashed in its private git
/// directory so `--continue` and `--abort` don't have to infer it from the
/// worktree's shape. Mode B sessions predate this and are recognised by its
/// absence, so an in-flight session from an older binary still continues.
#[derive(serde::Serialize, serde::Deserialize)]
struct RebaseSession {
    branch: String,
    base: String,
    /// The branch tip the rebase started from — the CAS old-value when the
    /// result is finally landed.
    from_sha: String,
}

fn rebase_session_path(worktree_git: &GitOperations) -> std::path::PathBuf {
    std::path::PathBuf::from(worktree_git.get_git_dir()).join("hitch-resolve-rebase.json")
}

fn read_rebase_session(worktree_git: &GitOperations) -> Option<RebaseSession> {
    let bytes = std::fs::read(rebase_session_path(worktree_git)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ── Mode A: guided rebase onto base ────────────────────────────────

/// Rebase the feature branch onto the environment's base — in a disposable
/// detached worktree, never the user's own checkout.
///
/// This used to `git checkout` the branch in the real repository, rebase it
/// there, and check the user back out afterwards. That put the user's working
/// tree in the middle of the operation: any failure between the two checkouts
/// stranded them on someone else's branch, and a conflicted rebase left them
/// parked mid-rebase on a branch they never asked to be on. Working detached
/// and landing the result with a CAS `update-ref` (plus the checkout resync
/// every other publish path uses) means the user's checkout is untouched
/// whatever happens — including when it is the branch being rebased.
fn resolve_mode_a(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    base: &str,
    worktree_path: &std::path::Path,
    tool: bool,
) -> Result<()> {
    context.log_info(&format!(
        "'{}' conflicts with base '{}' in '{}' — the durable fix is rebasing it.",
        branch, base, env_name
    ));

    context
        .git()
        .synchronize_branches(&[base.to_string(), branch.to_string()])?;

    let from_sha = context.git().get_branch_commit_sha(branch)?;
    let base_sha = context.git().get_branch_commit_sha(base)?;
    let worktree_path_str = worktree_path.to_string_lossy().to_string();

    context
        .git()
        .add_worktree_detached(&worktree_path_str, &from_sha)?;

    let cleanup = || {
        let _ = context.git().remove_worktree(&worktree_path_str, true);
    };

    let worktree_git = match GitOperations::new_at_path(&worktree_path_str) {
        Ok(g) => g,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };

    context.log_info(&format!("Rebasing '{}' onto '{}'...", branch, base));
    let clean = match worktree_git.rebase_onto(&base_sha) {
        Ok(clean) => clean,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };

    if !clean {
        let session = RebaseSession {
            branch: branch.to_string(),
            base: base.to_string(),
            from_sha,
        };
        if let Err(e) = std::fs::write(
            rebase_session_path(&worktree_git),
            serde_json::to_vec_pretty(&session)?,
        ) {
            cleanup();
            return Err(anyhow::anyhow!(
                "Could not record the resolve session state: {}",
                e
            ));
        }

        if tool {
            run_mergetool(&worktree_path_str);
        }

        context.log_warning(&format!(
            "Rebase paused with conflicts in {}\n\n\
             Resolve them there with plain Git (or --tool for git mergetool):\n\n\
               cd {}\n\
               git add <files>\n\
               git rebase --continue\n\n\
             Then land it:\n\n\
               hitch resolve {} --branch {} --continue\n\n\
             Or give up: hitch resolve {} --branch {} --abort\n\n\
             Your own checkout has not been touched.",
            worktree_path.display(),
            worktree_path.display(),
            env_name,
            branch,
            env_name,
            branch
        ));
        return Ok(());
    }

    let new_sha = match worktree_git.rev_parse("HEAD") {
        Ok(sha) => sha,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };

    let result = finish_mode_a(context, env_name, branch, base, &from_sha, &new_sha);
    cleanup();
    result
}

/// Land a completed rebase: move `refs/heads/<branch>` to the rebased tip with
/// a compare-and-swap, bring any checkout standing on it back in line, and
/// offer the force push. Shared by the clean-first-try path and `--continue`.
fn finish_mode_a(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    base: &str,
    from_sha: &str,
    new_sha: &str,
) -> Result<()> {
    if new_sha == from_sha {
        context.log_info(&format!(
            "'{}' is already on top of '{}' — nothing to land.",
            branch, base
        ));
        return Ok(());
    }

    let prior_remote_sha = context
        .git()
        .rev_parse_opt(&format!("refs/remotes/origin/{}", branch))?;

    // Same publish discipline as every other branch move: record the resync
    // first so an interruption is recoverable, CAS so a branch that moved
    // under us is detected rather than clobbered, then resync the checkouts.
    let checkouts = crate::utils::prelude::scan_checkouts_on_branch(context, branch)?;
    crate::utils::publish_journal::record(
        context,
        &crate::utils::publish_journal::PublishRecord {
            branch: branch.to_string(),
            from_sha: Some(from_sha.to_string()),
            to_sha: new_sha.to_string(),
            checkouts: crate::utils::prelude::checkout_paths(&checkouts),
        },
    )?;

    context
        .git()
        .update_ref_cas(
            &format!("refs/heads/{}", branch),
            new_sha,
            Some(from_sha),
            &format!("hitch: resolve — rebase {} onto {}", branch, base),
        )
        .map_err(|e| {
            crate::utils::publish_journal::clear(context, branch);
            anyhow::anyhow!(
                "'{}' rebased cleanly, but could not be updated: {}. It moved while the rebase \
                 was running — fetch and rerun 'hitch resolve {} --branch {}'.",
                branch,
                e,
                env_name,
                branch
            )
        })?;

    crate::utils::prelude::resync_checkouts(context, branch, new_sha, &checkouts);
    crate::utils::publish_journal::clear(context, branch);

    context.log_success(&format!("✓ '{}' rebased onto '{}'", branch, base));

    if context.should_push() {
        context.log_warning(&format!(
            "Ready to force push rebased '{}' to origin.",
            branch
        ));
        if context.confirm("Push it now?")? {
            match force_push_with_deploy_key_if_configured(context, branch, &prior_remote_sha) {
                Ok(()) => context.log_success(&format!("✓ Pushed '{}'", branch)),
                Err(e) => {
                    context.log_error(&format!("Failed to push '{}': {}", branch, e));
                    context.log_error(&format!(
                        "Someone may have pushed to '{}' in the meantime. Fetch, and push \
                         manually once you've confirmed it's safe: hitch push {} -f",
                        branch, branch
                    ));
                }
            }
        } else {
            context.log_info(&format!(
                "Not pushed. Push manually when ready: hitch push {} -f",
                branch
            ));
        }
    }

    context.log_info(&format!(
        "Run 'hitch rebuild {}' to pick this up.",
        env_name
    ));
    Ok(())
}

// ── Mode B: worktree resolve for a peer-vs-peer conflict ───────────

fn start_mode_b_session(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    conflict: &CompatibilityConflict,
    environment: &Environment,
    worktree_path: &std::path::Path,
    tool: bool,
) -> Result<()> {
    context.log_info(&format!(
        "'{}' conflicts with '{}' in '{}' — neither branch can own this fix alone.",
        branch, conflict.conflicts_with, env_name
    ));

    let idx = environment
        .branches
        .iter()
        .position(|b| b == branch)
        .ok_or_else(|| anyhow::anyhow!("'{}' is not promoted to '{}'", branch, env_name))?;
    let preceding = &environment.branches[..idx];

    let mut all = vec![environment.base.clone()];
    all.extend(preceding.iter().cloned());
    all.push(branch.to_string());
    context.git().synchronize_branches(&all)?;

    let base_sha = context.git().get_branch_commit_sha(&environment.base)?;
    let temp_branch = format!("hitch-resolve-{}", sanitize_for_path(branch));
    let worktree_path_str = worktree_path.to_string_lossy().to_string();

    context
        .git()
        .add_worktree(&worktree_path_str, &temp_branch, &base_sha)?;

    let cleanup = || {
        let _ = context.git().remove_worktree(&worktree_path_str, true);
        if context.git().branch_exists(&temp_branch).unwrap_or(false) {
            let _ = context.git().delete_branch(&temp_branch, true);
        }
    };

    let result = (|| -> Result<bool> {
        let worktree_git = GitOperations::new_at_path(&worktree_path_str)?;

        for peer in preceding {
            let sha = context.git().get_branch_commit_sha(peer)?;
            let message = format!("Hitch: merge {} into {}", peer, env_name);
            if let Err(e) = worktree_git.squash_merge(&sha, &message) {
                return Err(anyhow::anyhow!(
                    "Could not replay the composition up to '{}': '{}' no longer merges \
                     cleanly on its own ({}). The world has moved since the last preflight — \
                     run 'hitch rebuild {} --dry-run' to see current state.",
                    branch,
                    peer,
                    e,
                    env_name
                ));
            }
        }

        let branch_sha = context.git().get_branch_commit_sha(branch)?;
        let message = format!("Hitch: merge {} into {}", branch, env_name);
        match worktree_git.squash_merge(&branch_sha, &message) {
            Ok(()) => Ok(true), // composes now — nothing left to resolve
            Err(e) => {
                if worktree_git.has_merge_conflicts().unwrap_or(false) {
                    Ok(false) // expected: real conflict markers now in the worktree
                } else {
                    Err(e)
                }
            }
        }
    })();

    match result {
        Ok(true) => {
            cleanup();
            context.log_success(&format!(
                "'{}' composes cleanly now — nothing to resolve. Run 'hitch rebuild {}'.",
                branch, env_name
            ));
            Ok(())
        }
        Ok(false) => {
            // Capture the exact conflict stages *now*, while the worktree
            // index still carries them — once the user `git add`s their
            // resolution the stages are gone, but `--continue` needs them to
            // record the resolution against the right content-addressed key.
            // Stashed in the worktree's private git dir (outside the working
            // tree, auto-removed with the worktree).
            let worktree_git = GitOperations::new_at_path(&worktree_path_str)?;
            let branch_sha = context.git().get_branch_commit_sha(branch)?;
            match worktree_git.unmerged_stages() {
                Ok(stages) => {
                    let pending = crate::utils::resolutions::PendingConflict {
                        env: env_name.to_string(),
                        branch: branch.to_string(),
                        conflicts_with: conflict.conflicts_with.clone(),
                        source_branch_head: branch_sha,
                        stages,
                    };
                    if let Err(e) =
                        crate::utils::resolutions::write_pending(&worktree_git, &pending)
                    {
                        context.log_verbose(&format!(
                            "Could not stash pending-conflict state (resolution recording will \
                             be unavailable for this session): {}",
                            e
                        ));
                    }
                }
                Err(e) => context.log_verbose(&format!("Could not read conflict stages: {}", e)),
            }

            if tool {
                run_mergetool(&worktree_path_str);
            }
            context.log_warning(&format!(
                "Conflicts left in {}\n\n\
                 Resolve the files there (any editor, or --tool for git mergetool), then:\n\n\
                   hitch resolve {} --branch {} --continue\n\n\
                 Or give up: hitch resolve {} --branch {} --abort",
                worktree_path.display(),
                env_name,
                branch,
                env_name,
                branch
            ));
            Ok(())
        }
        Err(e) => {
            cleanup();
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn continue_session(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    worktree_path: &std::path::Path,
    environment: &Environment,
    record: bool,
    share: bool,
) -> Result<()> {
    if !worktree_path.exists() {
        return Err(anyhow::anyhow!(
            "No resolve session in progress for '{}' in '{}' (expected worktree at {}).\n\
             Run 'hitch resolve {} --branch {}' to start one.",
            branch,
            env_name,
            worktree_path.display(),
            env_name,
            branch
        ));
    }

    let worktree_path_str = worktree_path.to_string_lossy().to_string();
    let worktree_git = GitOperations::new_at_path(&worktree_path_str)?;
    let temp_branch = format!("hitch-resolve-{}", sanitize_for_path(branch));

    // A Mode A session is a paused rebase, not a paused merge — it lands the
    // rebased branch rather than composing and publishing the environment.
    if let Some(session) = read_rebase_session(&worktree_git) {
        return continue_rebase_session(context, env_name, worktree_path, &worktree_git, &session);
    }

    if worktree_git.has_merge_conflicts().unwrap_or(false) {
        return Err(anyhow::anyhow!(
            "'{}' still has unresolved files. Resolve them and 'git add' each, then rerun \
             'hitch resolve {} --branch {} --continue'.\nWorktree: {}",
            branch,
            env_name,
            branch,
            worktree_path.display()
        ));
    }

    if has_leftover_markers(&worktree_git) {
        return Err(anyhow::anyhow!(
            "'{}' has files with leftover conflict markers (<<<<<<< / >>>>>>>) even though \
             they're staged. Remove the markers, 'git add' again, then rerun 'hitch resolve {} \
             --branch {} --continue'.\nWorktree: {}",
            branch,
            env_name,
            branch,
            worktree_path.display()
        ));
    }

    let cleanup = || {
        let _ = context.git().remove_worktree(&worktree_path_str, true);
        if context.git().branch_exists(&temp_branch).unwrap_or(false) {
            let _ = context.git().delete_branch(&temp_branch, true);
        }
    };

    let build_result = (|| -> Result<String> {
        worktree_git.commit(&format!("Hitch: merge {} into {}", branch, env_name))?;

        let idx = environment
            .branches
            .iter()
            .position(|b| b == branch)
            .unwrap_or(environment.branches.len());
        let following = &environment.branches[idx + 1..];

        for peer in following {
            let sha = context.git().get_branch_commit_sha(peer)?;
            let message = format!("Hitch: merge {} into {}", peer, env_name);
            if let Err(e) = worktree_git.squash_merge(&sha, &message) {
                let _ = worktree_git.abort_merge_and_clean();
                return Err(anyhow::anyhow!(
                    "'{}' resolved, but composing the rest of '{}' now fails on '{}': {}\n\
                     Your resolution was not discarded — the worktree is still at {}. Fix '{}' \
                     first (rebase it onto '{}'), then rerun 'hitch resolve {} --branch {} \
                     --continue'.",
                    branch,
                    env_name,
                    peer,
                    e,
                    worktree_path.display(),
                    peer,
                    environment.base,
                    env_name,
                    branch
                ));
            }
        }

        worktree_git.rev_parse("HEAD")
    })();

    let new_sha = build_result?;

    // Record the resolution (if asked) BEFORE cleanup tears down the
    // worktree — recording reads both the captured conflict stages (in the
    // worktree's private git dir) and the resolved file contents (its
    // working tree), neither of which survives cleanup.
    let recorded = if record {
        maybe_record_resolution(context, &worktree_git, worktree_path, share)?
    } else {
        false
    };

    let remote_sha_before = context
        .git()
        .rev_parse_opt(&format!("refs/remotes/origin/{}", env_name))?;
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();

    let publish_result =
        publish_environment_build(context, env_name, &new_sha, &timestamp, &remote_sha_before);
    cleanup();
    publish_result?;

    context.log_success(&format!(
        "✓ Published '{}' with '{}' included.",
        env_name, branch
    ));
    if recorded {
        context.log_info(&format!(
            "Resolution recorded. A byte-identical conflict on a later rebuild can replay it \
             with 'hitch rebuild {} --replay-resolutions'{}. The durable fix is still carrying \
             the change back into '{}'.",
            env_name,
            if share { " (shared to origin)" } else { "" },
            branch
        ));
    } else {
        context.log_warning(&format!(
            "This resolution is not saved anywhere — the next plain 'hitch rebuild {}' will hit \
             the same conflict and hold '{}' again unless the fix is carried back into '{}' (or \
             '{}') itself. (Pass --record to save it.)",
            env_name, branch, branch, environment.base
        ));
    }

    Ok(())
}

/// Land a Mode A session whose rebase the user has finished by hand.
///
/// The rebase itself belongs to plain Git — hitch only checks that it actually
/// finished and then publishes the result the same way the clean path does.
fn continue_rebase_session(
    context: &GlobalContext,
    env_name: &str,
    worktree_path: &std::path::Path,
    worktree_git: &GitOperations,
    session: &RebaseSession,
) -> Result<()> {
    if worktree_git.rebase_in_progress() {
        return Err(anyhow::anyhow!(
            "The rebase of '{}' is still in progress. Finish it first:\n\n\
               cd {}\n\
               git add <files>\n\
               git rebase --continue\n\n\
             Then rerun 'hitch resolve {} --branch {} --continue'.",
            session.branch,
            worktree_path.display(),
            env_name,
            session.branch
        ));
    }

    if has_leftover_markers(worktree_git) {
        return Err(anyhow::anyhow!(
            "'{}' has files with leftover conflict markers (<<<<<<< / >>>>>>>). Remove them, \
             finish the rebase, then rerun 'hitch resolve {} --branch {} --continue'.\n\
             Worktree: {}",
            session.branch,
            env_name,
            session.branch,
            worktree_path.display()
        ));
    }

    let new_sha = worktree_git.rev_parse("HEAD")?;
    let result = finish_mode_a(
        context,
        env_name,
        &session.branch,
        &session.base,
        &session.from_sha,
        &new_sha,
    );

    // Only tear the session down once the result has actually landed —
    // otherwise a failed publish would discard the user's hand-resolved work.
    if result.is_ok() {
        let _ = context
            .git()
            .remove_worktree(&worktree_path.to_string_lossy(), true);
    }
    result
}

/// Record the just-resolved conflict as a content-addressed resolution ref,
/// reading the captured stages from the worktree's stashed pending state and
/// the resolved content from its working tree. Returns whether a resolution
/// was actually recorded. Best-effort: a recording failure is surfaced as a
/// warning, not a hard error — the resolution has already been composed and
/// is about to be published regardless.
fn maybe_record_resolution(
    context: &GlobalContext,
    worktree_git: &GitOperations,
    worktree_path: &std::path::Path,
    share: bool,
) -> Result<bool> {
    let pending = match crate::utils::resolutions::read_pending(worktree_git) {
        Ok(Some(p)) => p,
        Ok(None) => {
            context.log_warning(
                "Cannot record: this resolve session has no captured conflict state (it may \
                 predate --record support, or the conflict was never left for editing).",
            );
            return Ok(false);
        }
        Err(e) => {
            context.log_warning(&format!("Cannot record resolution: {}", e));
            return Ok(false);
        }
    };

    let recorded_by = context.git().get_user_email().unwrap_or_default();
    let recorded_at = chrono::Utc::now().to_rfc3339();

    match crate::utils::resolutions::record_resolution(
        context.git(),
        &pending,
        worktree_path,
        &recorded_by,
        &recorded_at,
    ) {
        Ok(crate::utils::resolutions::RecordOutcome::Recorded { key, files }) => {
            context.log_verbose(&format!("Recorded resolution {} ({} file(s))", key, files));
            if share {
                let refspec = format!(
                    "{}{}",
                    crate::utils::resolutions::RESOLUTIONS_REF_PREFIX,
                    key
                );
                if let Err(e) = context.git().push_refspec(&refspec) {
                    context.log_warning(&format!(
                        "Recorded locally, but failed to share to origin: {}\nPush it manually: \
                         git push origin {}",
                        e, refspec
                    ));
                }
            }
            Ok(true)
        }
        Ok(crate::utils::resolutions::RecordOutcome::Conflict {
            key,
            existing_commit,
        }) => {
            context.log_warning(&format!(
                "A different resolution for this exact conflict already exists ({}, commit {}). \
                 Not overwriting it. Inspect both with 'hitch resolutions show {}', and if yours \
                 should win, 'hitch resolutions forget {}' first.",
                key, existing_commit, key, key
            ));
            Ok(false)
        }
        Err(e) => {
            context.log_warning(&format!("Failed to record resolution: {}", e));
            Ok(false)
        }
    }
}

fn abort_session(
    context: &GlobalContext,
    worktree_path: &std::path::Path,
    branch: &str,
) -> Result<()> {
    if !worktree_path.exists() {
        context.log_info(&format!("No resolve session in progress for '{}'.", branch));
        return Ok(());
    }

    let worktree_path_str = worktree_path.to_string_lossy().to_string();
    let temp_branch = format!("hitch-resolve-{}", sanitize_for_path(branch));

    // A paused rebase holds its own state; abort it before the worktree goes
    // so git doesn't leave rebase metadata behind in the shared git dir.
    if let Ok(worktree_git) = GitOperations::new_at_path(&worktree_path_str) {
        if worktree_git.rebase_in_progress() {
            let _ = worktree_git.rebase_abort();
        }
    }

    let _ = context.git().remove_worktree(&worktree_path_str, true);
    if context.git().branch_exists(&temp_branch).unwrap_or(false) {
        let _ = context.git().delete_branch(&temp_branch, true);
    }

    context.log_success(&format!("✓ Discarded resolve session for '{}'", branch));
    Ok(())
}

fn has_leftover_markers(worktree_git: &GitOperations) -> bool {
    match worktree_git.run_git_command(&["grep", "-I", "-l", "-e", "^<<<<<<< ", "-e", "^>>>>>>> "])
    {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => false,
    }
}

fn run_mergetool(worktree_path: &str) {
    use std::process::Command;
    // The one deliberate exception to "never let git block on a real
    // terminal" (see AGENTS.md): `hitch resolve --tool` exists specifically
    // to hand the user an interactive `git mergetool` session.
    #[allow(clippy::disallowed_methods)]
    let _ = Command::new("git")
        .args(["mergetool"])
        .current_dir(worktree_path)
        .status();
}
