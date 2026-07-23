use crate::commands::global_context::GlobalContext;
use crate::types::Environment;
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::{
    access_metadata_read_only, preflight_compatibility_report, publish_environment_build,
    CompatibilityConflict,
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
        resolve_mode_a(context, &args.env_name, &branch, &environment.base)
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

// ── Mode A: guided rebase onto base ────────────────────────────────

fn resolve_mode_a(context: &GlobalContext, env_name: &str, branch: &str, base: &str) -> Result<()> {
    context.log_info(&format!(
        "'{}' conflicts with base '{}' in '{}' — the durable fix is rebasing it.",
        branch, base, env_name
    ));

    let original_branch = context.git().get_current_branch()?;
    let prior_remote_sha = context
        .git()
        .rev_parse_opt(&format!("refs/remotes/origin/{}", branch))?;

    if original_branch != branch {
        context.git().checkout_branch(branch)?;
    }

    context.log_info(&format!("Rebasing '{}' onto '{}'...", branch, base));
    let clean = context.git().rebase_onto(base)?;

    if !clean {
        context.log_warning(&format!(
            "Rebase paused with conflicts. Resolve them with plain Git, then:\n\n\
               git add <files>\n\
               git rebase --continue\n\n\
             Or give up: git rebase --abort\n\n\
             Once the rebase finishes cleanly, push it and run 'hitch rebuild {}' — this \
             replaces the recurring fix instead of caching around it.",
            env_name
        ));
        return Ok(());
    }

    context.log_success(&format!("✓ '{}' rebased cleanly onto '{}'", branch, base));

    if context.should_push() {
        context.log_warning(&format!(
            "Ready to force push rebased '{}' to origin.",
            branch
        ));
        if context.confirm("Push it now?")? {
            match context
                .git()
                .force_push_with_lease(branch, prior_remote_sha.as_deref())
            {
                Ok(()) => context.log_success(&format!("✓ Pushed '{}'", branch)),
                Err(e) => {
                    context.log_error(&format!("Failed to push '{}': {}", branch, e));
                    context.log_error(&format!(
                        "Someone may have pushed to '{}' in the meantime. Fetch, and push \
                         manually once you've confirmed it's safe: git push origin {} --force",
                        branch, branch
                    ));
                }
            }
        } else {
            context.log_info(&format!(
                "Not pushed. Push manually when ready: git push origin {} --force",
                branch
            ));
        }
    }

    // Return the user to wherever they started, unless they asked to work
    // on the feature branch directly (already there).
    if original_branch != branch && context.git().branch_exists(&original_branch)? {
        context.git().checkout_branch(&original_branch)?;
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

fn continue_session(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    worktree_path: &std::path::Path,
    environment: &Environment,
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
    context.log_warning(&format!(
        "This resolution is not saved anywhere — the next plain 'hitch rebuild {}' will hit \
         the same conflict and hold '{}' again unless the fix is carried back into '{}' (or \
         '{}') itself.",
        env_name, branch, branch, environment.base
    ));

    Ok(())
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
    let _ = Command::new("git")
        .args(["mergetool"])
        .current_dir(worktree_path)
        .status();
}
