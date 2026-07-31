use crate::commands::global_context::GlobalContext;
use crate::types::{HitchConfig, OnConflict};
use crate::utils::conflict_report::format_conflict_report;
use crate::utils::git_operations::GitOperations;
use crate::utils::progress::StepLogger;
use anyhow::{Context, Result};

/// Ensure the `hitch-metadata` branch exists locally, bootstrapping it from
/// `origin/hitch-metadata` if a teammate already ran `hitch init` and pushed.
///
/// `hitch-metadata` is a separate branch, so a plain `git pull` on whatever
/// branch a teammate is actually working on never creates a local tracking
/// branch for it — only the person who ran `init` (or previously bootstrapped
/// it themselves) has it locally. Without this, every other teammate hits a
/// false "not initialized" error despite the metadata sitting right there on
/// origin. This never touches an *existing* local branch (staleness is
/// handled separately, deliberately, by `check_metadata_health`'s
/// behind-remote check, which errors rather than silently advancing it).
///
/// Returns whether the branch exists locally once this returns (already
/// present, or just bootstrapped) — `false` only when it truly doesn't exist
/// anywhere (local or origin), meaning Hitch has never been initialized here.
pub fn ensure_hitch_metadata_branch(context: &GlobalContext) -> Result<bool> {
    if context.git().branch_exists("hitch-metadata")? {
        return Ok(true);
    }

    let _ = context.git().fetch_branch("hitch-metadata");
    if context.git().branch_exists_anywhere("hitch-metadata")? {
        context
            .git()
            .create_local_branch_from_remote("hitch-metadata")?;
        return Ok(true);
    }

    Ok(false)
}

/// Check that the hitch-metadata branch is in a healthy state
///
/// This function verifies that the hitch-metadata branch:
/// 1. Exists locally (bootstrapping it from origin first if needed)
/// 2. Is not behind the remote (i.e., has all remote changes pulled)
/// 3. Is not in a merge conflict state
/// 4. Has a clean working tree (if currently on hitch-metadata)
///
/// This prevents issues where someone has edited hitch.json on the remote
/// and the local copy is out of sync, which could cause conflicts or data loss.
///
/// # Returns
/// - `Ok(())`: hitch-metadata is healthy
/// - `Err`: hitch-metadata is unhealthy with details about the problem
pub fn check_metadata_health(context: &GlobalContext) -> Result<()> {
    context.log_verbose("Checking hitch-metadata branch health...");

    // 1. Check that hitch-metadata branch exists locally (or on origin)
    if !ensure_hitch_metadata_branch(context)? {
        return Err(anyhow::anyhow!(
            "hitch-metadata branch does not exist locally or on origin. Run 'hitch init' to initialize Hitch."
        ));
    }
    context.log_verbose("✓ hitch-metadata branch exists locally");

    // 2. Check that local is not behind remote (has un-pulled changes)
    // We fetch first to ensure we have the latest remote state
    let _ = context.git().fetch_branch("hitch-metadata");
    if context.git().is_branch_behind_remote("hitch-metadata")? {
        return Err(anyhow::anyhow!(
            "hitch-metadata branch is behind remote. There are changes on origin/hitch-metadata \
             that have not been pulled.\n\
             \n\
             This usually means someone else has modified the Hitch configuration. \
             Please pull the latest changes before continuing:\n\
             \n\
               git checkout hitch-metadata\n\
               git pull origin hitch-metadata\n\
               git checkout -\n\
             \n\
             Then retry your command."
        ));
    }
    context.log_verbose("✓ hitch-metadata is up to date with remote");

    // 3. Check for merge conflicts
    // Switch to hitch-metadata temporarily to check its state
    let current_branch = context.git().get_current_branch()?;
    let on_metadata_branch = current_branch == "hitch-metadata";

    if !on_metadata_branch {
        // We need to check hitch-metadata's state, but we can do this without
        // actually switching by checking if there are any ongoing operations
        // and by checking the branch's state via git commands

        // Check for merge state files that might indicate a failed operation on hitch-metadata
        let git_dir = context.git().get_git_dir();
        let git_path = std::path::Path::new(&git_dir);

        // Check for various merge/rebase state indicators
        let merge_head = git_path.join("MERGE_HEAD");
        let rebase_apply = git_path.join("rebase-apply");
        let rebase_merge = git_path.join("rebase-merge");
        let cherry_pick = git_path.join("CHERRY_PICK_HEAD");
        let revert = git_path.join("REVERT_HEAD");

        if merge_head.exists()
            || rebase_apply.exists()
            || rebase_merge.exists()
            || cherry_pick.exists()
            || revert.exists()
        {
            return Err(anyhow::anyhow!(
                "Git repository is in an unresolved merge/rebase/cherry-pick/revert state.\n\
                 \n\
                 Please resolve the current Git operation before using Hitch commands.\n\
                 \n\
                 If you want to abort the current operation:\n\
                   - Merge: git merge --abort\n\
                   - Rebase: git rebase --abort\n\
                   - Cherry-pick: git cherry-pick --abort\n\
                   - Revert: git revert --abort"
            ));
        }
    } else {
        // We're on hitch-metadata, check for conflicts directly
        if context.git().has_merge_conflicts()? {
            return Err(anyhow::anyhow!(
                "hitch-metadata branch has unresolved merge conflicts.\n\
                 \n\
                 Please resolve the conflicts before using Hitch commands.\n\
                 \n\
                 After resolving conflicts:\n\
                   git add .\n\
                   git commit\n\
                 \n\
                 Or to abort the merge:\n\
                   git merge --abort"
            ));
        }
    }
    context.log_verbose("✓ No merge conflicts detected");

    // 4. If on hitch-metadata, check working tree is clean
    if on_metadata_branch && !context.git().is_working_directory_clean()? {
        return Err(anyhow::anyhow!(
            "hitch-metadata branch has uncommitted changes.\n\
             \n\
             Please commit or stash your changes before using Hitch commands:\n\
               git status\n\
               git add .\n\
               git commit -m \"Your message\"\n\
             \n\
             Or stash:\n\
               git stash"
        ));
    }

    context.log_verbose("✓ hitch-metadata health check passed");
    Ok(())
}

/// Reusable pre-check function for all commands
///
/// Verifies the current directory is a Git repository and the working tree is
/// clean.  Commands that want to tolerate a dirty tree should call
/// `with_auto_stash` instead (which auto-stashes before running and pops after).
pub fn pre_check(context: &GlobalContext) -> Result<()> {
    context.log_verbose("Running pre-check validation...");

    // Check if we're in a git repository
    context.log_verbose("Checking if current directory is a Git repository...");
    if let Err(e) = context.git().get_current_branch() {
        return Err(anyhow::anyhow!(
            "Not in a Git repository. Please run this command from within a Git repository. Error: {}",
            e
        ));
    }
    context.log_verbose("✓ Git repository validation passed");

    // Check if working tree is clean
    context.log_verbose("Checking if working tree is clean...");
    let is_clean = context.git().is_working_directory_clean()?;
    if !is_clean {
        return Err(anyhow::anyhow!(
            "Working tree is not clean. Please commit or stash your changes before running this command."
        ));
    }
    context.log_verbose("✓ Working tree is clean");

    context.log_verbose("Pre-check validation completed successfully");
    Ok(())
}

/// Reusable pre-check that only validates the Git repository (not working-tree
/// cleanliness).  Use this when the caller manages stashing manually.
pub fn pre_check_repo_only(context: &GlobalContext) -> Result<()> {
    context.log_verbose("Running pre-check validation (repo only)...");
    if let Err(e) = context.git().get_current_branch() {
        return Err(anyhow::anyhow!(
            "Not in a Git repository. Please run this command from within a Git repository. Error: {}",
            e
        ));
    }
    context.log_verbose("✓ Git repository validation passed");
    Ok(())
}

/// Run `f` with any uncommitted working-tree changes automatically stashed
/// beforehand and restored afterward.
///
/// If the working tree is already clean this is a no-op wrapper.
pub fn with_auto_stash<F, R>(context: &GlobalContext, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    let is_clean = context.git().is_working_directory_clean()?;

    let stashed = if !is_clean {
        context.log_info("Auto-stashing local changes before operation...");
        let created = context
            .git()
            .stash_push("hitch: auto-stash before operation")?;
        if created {
            context.log_verbose("✓ Changes stashed");
        }
        created
    } else {
        false
    };

    let result = f();

    if stashed {
        context.log_info("Restoring stashed changes...");
        if let Err(e) = context.git().stash_pop() {
            context.log_warning(&format!(
                "Failed to restore stashed changes: {}. Run 'git stash pop' to restore manually.",
                e
            ));
        } else {
            context.log_verbose("✓ Stashed changes restored");
        }
    }

    result
}

/// Read-only access to hitch metadata without branch switching
///
/// According to the specification (status command note):
/// - Make sure hitch-metadata is up to date (fetch)
/// - Use git show hitch-metadata:hitch.json to read metadata
/// - Works even with unclean git states
/// - No branch switching required
/// - Cannot modify metadata
pub fn access_metadata_read_only<F, R>(context: &GlobalContext, closure: F) -> Result<R>
where
    F: FnOnce(&HitchConfig) -> Result<R>,
{
    context.log_verbose("Reading hitch metadata (read-only)...");

    // Check hitch-metadata health before accessing
    check_metadata_health(context)?;

    // Make sure hitch-metadata is up to date
    context.log_verbose("Ensuring hitch-metadata is up to date...");
    if let Err(e) = context.git().fetch_branch("hitch-metadata") {
        context.log_warning(&format!(
            "Failed to fetch hitch-metadata from remote: {}",
            e
        ));
        context.log_warning("Continuing with local metadata...");
    }

    // Read hitch.json using git show (no branch switching needed).
    //
    // Read from the LOCAL hitch-metadata branch first so this is consistent with
    // `modify_metadata` (which always reads/writes local). `check_metadata_health`
    // guarantees the local branch exists and is not behind the remote, so local is
    // the source of truth. Reading `origin/` first would surface stale data after a
    // `--no-push` mutation (local ahead of origin). `origin/` remains a fallback for
    // edge cases where the local read fails.
    context.log_verbose("Reading hitch.json from hitch-metadata branch...");
    let config_json = match context
        .git()
        .read_file_from_branch("hitch-metadata", "hitch.json")
    {
        Ok(content) => {
            context.log_verbose("✓ Read from local hitch-metadata");
            content
        }
        Err(_) => {
            context
                .log_verbose("local hitch-metadata not readable, trying origin/hitch-metadata...");
            context.git()
                .read_file_from_branch("origin/hitch-metadata", "hitch.json")
                .context("Failed to read hitch.json from either local hitch-metadata or origin/hitch-metadata branch")?
        }
    };

    // Parse and validate configuration. `hitch-metadata` is writable by anyone
    // with push access, so this is a trust boundary — see `config_validation`.
    let config: HitchConfig =
        crate::utils::config_validation::parse_untrusted_config(&config_json)?;

    context.log_verbose("✓ hitch.json loaded and validated successfully (read-only)");

    // Execute user closure with read-only access
    closure(&config)
}

/// Modify hitch metadata with automatic branch management
///
/// According to the specification:
/// - Fetch latest hitch-metadata from remote: git fetch origin hitch-metadata
/// - Stage changes onto hitch-metadata via a scratch git index (see
///   `GitOperations::begin_branch_write`) — the user's working directory,
///   real index, and current branch are never touched
/// - Load and parse hitch.json
/// - Execute closure with mutable metadata object (for modification)
/// - Commit and optionally push changes (warn if push fails or skip with --no-push)
pub fn modify_metadata<F>(context: &GlobalContext, closure: F) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    modify_metadata_impl(context, closure, false)
}

/// Like [`modify_metadata`] but skips the pre-flight health check and remote
/// fetch.
///
/// Recovery/rollback paths use this: they must be able to write even when the
/// repository is in the degraded state (behind remote, mid-abort) that triggered
/// the rollback — otherwise `check_metadata_health` would refuse on the very
/// condition the rollback is trying to repair.
pub fn modify_metadata_unchecked<F>(context: &GlobalContext, closure: F) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    modify_metadata_impl(context, closure, true)
}

fn modify_metadata_impl<F>(context: &GlobalContext, closure: F, skip_preflight: bool) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    context.log_verbose("Accessing hitch metadata...");

    // The branch not existing locally OR on origin is the init bootstrap
    // case: there is nothing to health-check or fetch, and the write below
    // will create it as a root commit. If it exists on origin but not
    // locally (a teammate ran `init` and pushed, we just haven't bootstrapped
    // our local branch yet), this pulls it down first — otherwise this write
    // would wrongly take the bootstrap path and create a divergent root
    // commit, orphaning the team's real metadata history.
    let branch_exists = ensure_hitch_metadata_branch(context)?;

    if branch_exists && !skip_preflight {
        // Check hitch-metadata health before modifying (skipped on
        // recovery/rollback paths via `skip_preflight`).
        check_metadata_health(context)?;

        // Fetch latest hitch-metadata from remote
        context.log_verbose("Fetching latest hitch-metadata from remote...");
        if let Err(e) = context.git().fetch_branch("hitch-metadata") {
            context.log_warning(&format!(
                "Failed to fetch hitch-metadata from remote: {}",
                e
            ));
            context.log_warning("Continuing with local metadata...");
        }
    }

    // Stage every write below into a scratch index tied to hitch-metadata.
    // Nothing here touches the user's working directory, real index, or
    // current branch, so there is no checkout to protect and no auto-stash
    // needed — regardless of what branch the user happens to be on.
    context.git().begin_branch_write("hitch-metadata")?;

    let result = (|| {
        // Load and parse hitch.json
        context.log_verbose("Loading hitch.json...");
        let config_json = match context
            .git()
            .read_file_from_branch("hitch-metadata", "hitch.json")
        {
            Ok(content) => content,
            Err(e) => {
                // If file doesn't exist, create default config
                context.log_info(&format!(
                    "hitch.json not found or unreadable ({}), creating default configuration",
                    e
                ));
                let default_config = HitchConfig::new();
                serde_json::to_string_pretty(&default_config)
                    .context("Failed to serialize default hitch configuration")?
            }
        };

        let mut config: HitchConfig =
            crate::utils::config_validation::parse_untrusted_config(&config_json)?;

        // Refuse to rewrite a config newer than we understand (unless this is a
        // recovery/rollback write, which restores our own captured snapshot).
        if !skip_preflight {
            if let Err(e) = config.check_write_compatibility() {
                return Err(anyhow::anyhow!("{}", e));
            }
        }

        context.log_verbose("✓ hitch.json loaded successfully");

        // Execute closure (for modification)
        context.log_verbose("Modifying metadata...");
        closure(&mut config)?;

        // Write updated config
        context.log_verbose("Writing updated hitch.json...");
        context
            .git()
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;

        // Commit changes (any other file staged into this transaction, e.g.
        // init's .gitignore, is carried over from the branch's existing tree
        // or was staged earlier via the same `write_file` call).
        context.log_verbose("Committing metadata changes...");
        context
            .git()
            .commit_branch_write("Update hitch configuration")?;

        // Optionally push
        if context.should_push() {
            context.log_verbose("Pushing metadata to remote...");
            if let Err(e) = context.git().push_branch("hitch-metadata") {
                let msg = e.to_string();
                // A non-fast-forward rejection means origin/hitch-metadata moved
                // after our health check (someone else changed the config). Surface
                // this loudly: the change is committed LOCALLY only and the remote
                // is now out of sync — silently swallowing it (as before) leaves the
                // team on divergent config.
                if msg.contains("rejected")
                    || msg.contains("non-fast-forward")
                    || msg.contains("fetch first")
                    || msg.contains("Updates were rejected")
                {
                    context.log_warning(
                        "Could not push hitch-metadata: origin has changes yours do not. \
                         Your update is committed LOCALLY only and the remote is now out of sync.",
                    );
                    context.log_warning(
                        "Reconcile before others rely on the remote:\n  \
                         git checkout hitch-metadata && git pull --rebase origin hitch-metadata && git checkout -\n  \
                         then re-run, or push manually with: git push origin hitch-metadata",
                    );
                } else {
                    context.log_warning(&format!("Failed to push metadata to remote: {}", e));
                }
            } else {
                context.log_verbose("✓ Metadata pushed to remote");
            }
        } else {
            context.log_verbose("Skipping push due to --no-push flag");
        }

        Ok(())
    })();

    if result.is_err() {
        // Never leave a half-finished transaction behind — it would block
        // the next `begin_branch_write` call in this process.
        context.git().abort_branch_write();
    }

    result
}

/// Execute operations within a locked environment context
///
/// According to the specification:
/// - Calls lock(env_name) → executes closure → calls unlock(env_name) even if closure fails
/// - Ensures environment is safely locked during modifications
/// - Automatically handles warnings if push fails
pub fn with_locked_env<F, R>(context: &GlobalContext, env_name: &str, closure: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    context.log_verbose(&format!("Locking environment '{}'...", env_name));

    // Only manage the lock if we actually acquire it. If the environment is already
    // locked — by another operation or a re-entrant call — leave the lock untouched so
    // we never release a lock we didn't take (which would let a concurrent operation
    // proceed, or unlock an env mid-way through an outer operation that still needs it).
    let already_locked = access_metadata_read_only(context, |config| {
        Ok(config
            .environments
            .get(env_name)
            .map(|e| e.is_locked())
            .unwrap_or(false))
    })?;

    if !already_locked {
        lock_environment(context, env_name)?;
    }

    // Execute the closure
    let result = closure();

    // Unlock only if we were the ones who locked it, even if the closure failed.
    if !already_locked {
        context.log_verbose(&format!("Unlocking environment '{}'...", env_name));
        if let Err(e) = unlock_environment(context, env_name) {
            context.log_warning(&format!(
                "Failed to unlock environment '{}': {}",
                env_name, e
            ));
            context.log_warning("You may need to manually unlock the environment");
        } else {
            context.log_verbose(&format!(
                "✓ Environment '{}' unlocked successfully",
                env_name
            ));
        }
    }

    result
}

/// Lock an environment
pub fn lock_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    modify_metadata(context, |config: &mut HitchConfig| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        if environment.is_locked() {
            context.log_info(&format!("Environment '{}' is already locked", env_name));
            return Ok(());
        }

        // Get current user email
        let user_email = context.git().get_user_email()?;
        environment.lock(user_email.clone());

        context.log_info(&format!(
            "Environment '{}' locked by '{}'",
            env_name, user_email
        ));
        Ok(())
    })
}

/// Unlock an environment
pub fn unlock_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    modify_metadata(context, |config: &mut HitchConfig| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        if !environment.is_locked() {
            context.log_info(&format!("Environment '{}' is already unlocked", env_name));
            return Ok(());
        }

        environment.unlock();

        context.log_info(&format!("Environment '{}' unlocked", env_name));
        Ok(())
    })
}

/// What a rebuild actually did, beyond publishing the environment branch.
pub struct RebuildOutcome {
    /// Branches that were excluded from this build because they conflicted
    /// (only ever non-empty under `OnConflict::Eject`).
    pub held: Vec<CompatibilityConflict>,
    /// Branches that conflicted but were composed anyway from a recorded
    /// resolution (only ever non-empty under `--replay-resolutions`).
    pub replayed: Vec<String>,
}

/// Rebuild an environment by composing its promoted branches into a new
/// environment branch, in an isolated worktree.
///
/// This is the core reusable rebuild function that can be called by promote,
/// demote, or rebuild commands.
///
/// Design (see docs/merge-conflict-handling-plan.md, phase 1):
/// - Composition happens in a disposable linked worktree, never in the
///   user's own checkout — the user's working tree is not touched.
/// - Base and every promoted branch are resolved to concrete SHAs once, right
///   after synchronizing; the worktree is built from, and every merge
///   consumes, those pinned SHAs. This closes the old TOCTOU window where a
///   passing preflight could still conflict during the real merge because
///   refs moved in between.
/// - Publishing the rebuilt branch is a single compare-and-swap `update-ref`,
///   preceded by writing a timestamped backup ref. There is no
///   rename-to-backup-then-recreate window; a crash either leaves the old
///   branch untouched or the new one fully published, never in between.
/// - Automatic rollback on any failure (the worktree is torn down; the real
///   environment branch is never mutated unless the whole build succeeded).
/// - A branch that conflicts is handled per the environment's `on_conflict`
///   policy (phase 3): `Eject` (the default) excludes it from the
///   composition and keeps going — matching the eject-and-continue policy
///   every merge queue surveyed converges on — while `Halt` aborts the whole
///   rebuild on the first conflict, as phase 1 always did.
pub fn rebuild_environment(context: &GlobalContext, env_name: &str) -> Result<RebuildOutcome> {
    rebuild_environment_opts(context, env_name, false)
}

/// `rebuild_environment` with the phase-5 replay opt-in. `replay = true`
/// (only ever set by `hitch rebuild --replay-resolutions`) makes a
/// conflicting branch first try a recorded, content-addressed resolution
/// (see `crate::utils::resolutions`) before being held — turning a
/// previously hand-resolved peer conflict back into a clean compose without
/// re-resolving it. Every other caller (promote, demote, release, approve)
/// passes `false`, so a plain rebuild never consults resolutions.
pub fn rebuild_environment_opts(
    context: &GlobalContext,
    env_name: &str,
    replay: bool,
) -> Result<RebuildOutcome> {
    context.log_verbose(&format!(
        "Starting rebuild process for environment '{}'",
        env_name
    ));

    // Acquire per-environment rebuild lock to prevent concurrent rebuilds.
    // The lock is released automatically when `_rebuild_lock` goes out of scope.
    let git_dir = std::path::PathBuf::from(context.git().get_git_dir());
    let _rebuild_lock = crate::utils::rebuild_lock::RebuildLock::acquire(&git_dir, env_name)?;

    // Get environment configuration to understand how to rebuild
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config
        .environments
        .get(env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", env_name))?
        .clone();

    // Set up step logging: sync+pin, one step per branch to merge, publish.
    let merge_steps = environment.branches.len().max(1);
    let total_steps = 2 + merge_steps;
    let mut logger = StepLogger::new_with_output(
        format!("Rebuilding environment '{}'", env_name),
        total_steps,
        context.output.clone(),
    );

    // Step 1: synchronize base + every promoted branch once, then pin each to
    // the concrete SHA we will actually build from. Everything below — the
    // worktree's starting point, every squash merge — uses these pinned
    // values instead of the mutable branch names, so a ref moving mid-build
    // (another push, a concurrent rebuild) cannot change what we compose.
    logger.step("Synchronizing branches".to_string());
    if !context.git().branch_exists_anywhere(&environment.base)? {
        return Err(anyhow::anyhow!(
            "Base branch '{}' does not exist",
            environment.base
        ));
    }
    let mut all_branches = vec![environment.base.clone()];
    all_branches.extend(environment.branches.iter().cloned());
    context.git().synchronize_branches(&all_branches)?;

    let base_sha = context.git().get_branch_commit_sha(&environment.base)?;
    let mut pinned_branches = Vec::with_capacity(environment.branches.len());
    for branch in &environment.branches {
        if !context.git().branch_exists(branch)? {
            return Err(anyhow::anyhow!(
                "Branch '{}' does not exist locally after synchronization",
                branch
            ));
        }
        let sha = context.git().get_branch_commit_sha(branch)?;
        pinned_branches.push((branch.clone(), sha));
    }

    // Snapshot the remote environment SHA now, before building, so the
    // eventual push is leased against what we actually observed — not
    // whatever `origin/<env>` happens to be once the build finishes.
    let remote_env_sha_before = context
        .git()
        .rev_parse_opt(&format!("refs/remotes/origin/{}", env_name))?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();

    // Step 2..N: compose the promoted branches, in order, entirely in the
    // object database — `git merge-tree --write-tree` runs the same ORT merge
    // a real `git merge` does but writes only trees, so there is no worktree
    // to create, leak on a crash, or collide with the user's checkouts. A
    // branch that conflicts is either ejected — skipped, recorded as held,
    // with later branches checked against what actually accumulated without
    // it, so nine healthy branches are never blocked by one broken one — or
    // the whole rebuild halts, per the environment's `on_conflict` policy.
    // Ejecting is now simply "don't advance `composed`": there is no merge
    // state anywhere that would need aborting.
    let mut held: Vec<CompatibilityConflict> = Vec::new();
    let mut replayed: Vec<String> = Vec::new();
    let mut confirmed_replay_keys: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let build_result = (|| -> Result<String> {
        let git = context.git();
        let mut composed = base_sha.clone();
        let mut last_composed = environment.base.clone();

        for (branch, sha) in &pinned_branches {
            logger.step(format!("Merging '{}'", branch));

            let merge_message = format!("Hitch: merge {} into {}", branch, env_name);
            let outcome = git.merge_tree_compose(&composed, sha)?;

            if outcome.conflicted_stages.is_empty() {
                // A merge that changes nothing produces no commit, matching
                // what `git merge --squash` + `git commit` did when there was
                // nothing staged (a branch already contained in the base).
                let composed_tree = git.rev_parse(&format!("{}^{{tree}}", composed))?;
                if outcome.tree_oid != composed_tree {
                    composed = git.commit_tree(&outcome.tree_oid, &[&composed], &merge_message)?;
                }
                context.log_verbose(&format!("✓ Composed '{}' into '{}'", branch, env_name));
                last_composed = branch.clone();
                continue;
            }

            // Try a recorded resolution first (phase 5, opt-in). An exact
            // content-addressed match means byte-identical conflict inputs, so
            // replaying it reproduces the same fix a human already made —
            // turning the hold back into a clean compose. On any miss,
            // decline, or error, fall through to the normal halt/eject path.
            if replay {
                if let Some(resolved) = try_replay_resolution(
                    context,
                    &composed,
                    branch,
                    &merge_message,
                    &outcome,
                    &mut confirmed_replay_keys,
                )? {
                    composed = resolved;
                    replayed.push(branch.clone());
                    last_composed = branch.clone();
                    continue;
                }
            }

            let conflict_result =
                git.conflict_result_from_compose(branch, env_name, &composed, sha, &outcome)?;

            if environment.on_conflict == OnConflict::Halt {
                return Err(anyhow::anyhow!(
                    "{}",
                    format_conflict_report(
                        branch,
                        &conflict_result.target_branch,
                        &environment.base,
                        env_name,
                        &conflict_result.conflicted_files,
                        conflict_result.merge_base.as_ref(),
                    )
                ));
            }

            let conflicted_paths: Vec<String> = conflict_result
                .conflicted_files
                .iter()
                .map(|f| f.path.clone())
                .collect();
            context.log_warning(&format!(
                "⛔ Held '{}' — conflicts with '{}' ({} file{})",
                branch,
                last_composed,
                conflicted_paths.len(),
                if conflicted_paths.len() == 1 { "" } else { "s" }
            ));
            held.push(CompatibilityConflict {
                branch: branch.clone(),
                conflicts_with: last_composed.clone(),
                conflicted_files: conflicted_paths,
            });
        }

        if environment.branches.is_empty() {
            logger.step("No promoted branches to merge".to_string());
        }

        Ok(composed)
    })();

    let new_sha = build_result?;

    // The composed commit is only reachable from this process until the
    // publish CAS lands. Anchor it under a ref for that window so a
    // concurrent `git gc --prune=now` cannot collect it out from under us.
    let build_ref = format!("refs/hitch/build/{}/{}", env_name, timestamp);
    context.git().update_ref(&build_ref, &new_sha)?;
    let drop_build_ref = || {
        let _ = context.git().delete_ref(&build_ref);
    };

    // Publish, push, and record the timestamp — shared with `hitch resolve`'s
    // Mode B, which produces a build the same way but from a hand-resolved
    // worktree instead of a straight-through compose. The anchor ref is only
    // dropped *after* publish is attempted, so the new commit stays reachable
    // for the whole window until `refs/heads/<env>` takes over that job.
    logger.step(format!("Publishing '{}'", env_name));
    let publish_result = publish_environment_build(
        context,
        env_name,
        &new_sha,
        &timestamp,
        &remote_env_sha_before,
    );
    drop_build_ref();
    publish_result?;

    logger.complete();

    context.log_verbose(&format!(
        "✓ Rebuild process completed for environment '{}'",
        env_name
    ));
    Ok(RebuildOutcome { held, replayed })
}

/// If `hitch setup` has been run for this repository, use the deploy key to
/// force-push so the push bypasses the `hitch-protection` GitHub ruleset.
/// Falls back to a plain `force_push_with_lease` against `origin` when no
/// deploy key is configured.
pub(crate) fn force_push_with_deploy_key_if_configured(
    context: &GlobalContext,
    env_name: &str,
    remote_sha_before: &Option<String>,
) -> Result<()> {
    let result = match crate::utils::gh::owner_repo_from_remote() {
        Ok((owner, repo)) if crate::utils::setup::is_setup(&owner, &repo) => {
            let key_path = crate::utils::setup::key_path(&owner, &repo);
            let ssh_url = format!("git@github.com:{}/{}.git", owner, repo);
            context.git().force_push_with_ssh_identity(
                env_name,
                remote_sha_before.as_deref(),
                &key_path.to_string_lossy(),
                &ssh_url,
            )
        }
        _ => context
            .git()
            .force_push_with_lease(env_name, remote_sha_before.as_deref()),
    };

    if result.is_ok() {
        record_pushed_tip(context, env_name);
    }
    result
}

/// Point `refs/remotes/origin/<branch>` at whatever hitch just pushed.
///
/// The deploy-key pushes go to an explicit SSH URL rather than the `origin`
/// remote, so git does not update the remote-tracking ref for them. Left
/// alone, `git status` then reports a branch hitch has just synchronized as
/// ahead of — or diverged from — origin until the user's next fetch, which
/// reads exactly like the desync bug this is all trying to eliminate.
/// Best-effort: a failure here is cosmetic and must not fail a landed push.
fn record_pushed_tip(context: &GlobalContext, branch: &str) {
    if let Ok(Some(sha)) = context
        .git()
        .rev_parse_opt(&format!("refs/heads/{}", branch))
    {
        let _ = context.git().set_remote_tracking_ref(branch, &sha);
    }
}

/// If `hitch setup` has been run for this repository, use the deploy key to
/// push so the push bypasses the `hitch-protection` GitHub ruleset. Falls
/// back to a plain `push_branch` against `origin` when no deploy key is
/// configured.
pub(crate) fn push_branch_with_deploy_key_if_configured(
    context: &GlobalContext,
    branch: &str,
) -> Result<()> {
    let result = match crate::utils::gh::owner_repo_from_remote() {
        Ok((owner, repo)) if crate::utils::setup::is_setup(&owner, &repo) => {
            let key_path = crate::utils::setup::key_path(&owner, &repo);
            let ssh_url = format!("git@github.com:{}/{}.git", owner, repo);
            context
                .git()
                .push_with_ssh_identity(branch, &key_path.to_string_lossy(), &ssh_url)
        }
        _ => context.git().push_branch(branch),
    };

    if result.is_ok() {
        record_pushed_tip(context, branch);
    }
    result
}

/// A checkout that has some branch attached as HEAD, captured *before* that
/// branch's ref is moved. See `scan_checkouts_on_branch`.
pub(crate) struct CheckoutState {
    path: String,
    /// Whether the working tree was clean at scan time — i.e. before the ref
    /// moved. Asking afterwards is useless: every affected checkout reports
    /// dirty, because `git status` is then comparing an old working tree
    /// against the new tip.
    clean: bool,
}

/// Record which checkouts have `branch` attached, and whether each was clean,
/// *before* anything moves the ref.
///
/// This must be called ahead of the `update-ref` and its result handed to
/// `resync_checkouts`. The ordering is the whole point: once the ref has
/// moved, the difference between "the user has uncommitted work here" and
/// "this checkout is simply stale" is no longer observable.
pub(crate) fn scan_checkouts_on_branch(
    context: &GlobalContext,
    branch: &str,
) -> Result<Vec<CheckoutState>> {
    let mut states = Vec::new();

    for checkout in context.git().checkouts_on_branch(branch)? {
        let clean = match checkout_git(context, &checkout.path) {
            Some(git) => git.is_working_directory_clean().unwrap_or(false),
            None => false,
        };
        states.push(CheckoutState {
            path: checkout.path,
            clean,
        });
    }

    Ok(states)
}

/// The paths of a scan, for handing to a durable pending-resync record.
pub(crate) fn checkout_paths(scanned: &[CheckoutState]) -> Vec<String> {
    scanned.iter().map(|c| c.path.clone()).collect()
}

/// Open a `GitOperations` for `path`, reusing the context's own instance when
/// that path *is* the main checkout. `git worktree list` reports fully
/// resolved paths, so compare canonically rather than by string.
fn checkout_git(context: &GlobalContext, path: &str) -> Option<GitOperations> {
    if GitOperations::same_checkout_path(path, context.git().workdir()) {
        // Cheap to reopen and keeps the return type uniform; the main checkout
        // is a normal repository path like any other.
        GitOperations::new_at_path(context.git().workdir()).ok()
    } else {
        GitOperations::new_at_path(path).ok()
    }
}

/// Bring every checkout in `scanned` back in line with the ref hitch just
/// moved to `new_sha`.
///
/// `git update-ref` moves a branch without touching the index or working tree
/// of any checkout that has it attached as HEAD — git leaves those alone by
/// design. So a user sitting on `production` when `hitch release` publishes
/// sees the entire release diff appear as uncommitted *reverse* changes in
/// `git status`, with no indication of why. This is the repair, and it must be
/// called from every site that lands a new commit on a branch a human might be
/// standing on (`publish_environment_build` and `hitch release`).
///
/// Checkouts that were clean at scan time are hard-reset to `new_sha` (their
/// HEAD symref already resolves there; only index and working tree are stale).
/// Ones that were already dirty are deliberately **not** touched — the user's
/// uncommitted work outranks tidiness — and get a warning naming the path and
/// the exact recovery command. Detached-HEAD checkouts never appear here at
/// all: their HEAD names a commit, not the branch, so nothing moved underneath
/// them.
pub(crate) fn resync_checkouts(
    context: &GlobalContext,
    branch: &str,
    new_sha: &str,
    scanned: &[CheckoutState],
) {
    for checkout in scanned {
        if !checkout.clean {
            context.log_warning(&format!(
                "'{}' is checked out at '{}' with uncommitted changes, so it was NOT \
                 updated to the rebuilt branch. Your changes are safe, but that working \
                 tree no longer matches '{}'. To reconcile it:\n  \
                 cd {} && git stash && git reset --hard {} && git stash pop",
                branch, checkout.path, branch, checkout.path, branch
            ));
            continue;
        }

        let result = match checkout_git(context, &checkout.path) {
            Some(git) => git.reset_hard_to(new_sha),
            None => Err(anyhow::anyhow!("could not open the repository there")),
        };

        match result {
            Ok(()) => context.log_verbose(&format!(
                "✓ Updated working tree at '{}' to rebuilt '{}'",
                checkout.path, branch
            )),
            Err(e) => context.log_warning(&format!(
                "Failed to update the working tree at '{}' to the rebuilt '{}': {}. \
                 Reconcile it with:\n  cd {} && git reset --hard {}",
                checkout.path, branch, e, checkout.path, branch
            )),
        }
    }
}

/// Publish `new_sha` as environment `env_name`'s new content: back up the
/// current ref (if any) under a timestamped backup ref, then swap
/// `refs/heads/<env_name>` to `new_sha` with a single compare-and-swap —
/// the only instruction that mutates it, so nothing is observable as
/// changed until this call succeeds. Then, if pushing is enabled, offers a
/// confirmed `--force-with-lease` push against `remote_sha_before` (the
/// remote SHA observed before the caller started building, so a concurrent
/// push elsewhere is detected instead of silently clobbered), and finally
/// records the environment's `rebuilt_at` timestamp.
///
/// Shared by `rebuild_environment` and `hitch resolve`'s Mode B — both
/// produce a new environment-branch commit and need to land it the same
/// way; only how the commit was built differs.
pub(crate) fn publish_environment_build(
    context: &GlobalContext,
    env_name: &str,
    new_sha: &str,
    backup_timestamp: &str,
    remote_sha_before: &Option<String>,
) -> Result<()> {
    let env_ref = format!("refs/heads/{}", env_name);
    let old_env_sha = context.git().rev_parse_opt(&env_ref)?;

    // Must happen before the ref moves — see `scan_checkouts_on_branch`.
    let checkouts = scan_checkouts_on_branch(context, env_name)?;

    // Write the resync down before anything observable changes, so a crash
    // between the CAS and the resync is recoverable rather than a permanent
    // silent desync — see `crate::utils::pending_resync`.
    crate::utils::pending_resync::record(
        context,
        &crate::utils::pending_resync::PendingResync {
            branch: env_name.to_string(),
            from_sha: old_env_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
        },
    )?;

    if let Some(ref old_sha) = old_env_sha {
        let backup_ref = format!("refs/hitch/backup/{}/{}", env_name, backup_timestamp);
        context.git().update_ref(&backup_ref, old_sha)?;
        context.log_verbose(&format!(
            "✓ Backed up previous '{}' ({}) to '{}'",
            env_name, old_sha, backup_ref
        ));
    }

    if let Err(e) = context.git().update_ref_cas(
        &env_ref,
        new_sha,
        old_env_sha.as_deref(),
        &format!("hitch: rebuild {}", env_name),
    ) {
        return Err(anyhow::anyhow!(
            "Failed to publish '{}': {}. The build itself succeeded but could not be \
             published — this usually means another rebuild landed first. Fetch and re-run \
             'hitch rebuild {}'.",
            env_name,
            e,
            env_name
        ));
    }

    context.log_verbose(&format!("✓ Published '{}' ({})", env_name, new_sha));

    // The ref has moved; any checkout standing on it is now stale. Do this
    // before pushing so the local repository is coherent even if the push
    // fails or the user declines it.
    resync_checkouts(context, env_name, new_sha, &checkouts);
    crate::utils::pending_resync::clear(context, env_name);

    if context.should_push() {
        context.log_warning(&format!(
            "Ready to force push the rebuilt '{}' branch to 'origin/{}'.",
            env_name, env_name
        ));
        context.log_warning(&format!(
            "This will OVERWRITE the remote '{}' branch with the new rebuilt version.",
            env_name
        ));
        context.log_warning("This action cannot be undone.");

        if context.confirm("Do you want to proceed?")? {
            context.log_info(&format!(
                "Force pushing rebuilt '{}' branch to replace remote",
                env_name
            ));
            match force_push_with_deploy_key_if_configured(context, env_name, remote_sha_before) {
                Ok(()) => {
                    context.log_success(&format!(
                        "✓ Force pushed rebuilt '{}' branch to remote",
                        env_name
                    ));
                }
                Err(e) => {
                    context.log_error(&format!(
                        "Failed to force push rebuilt '{}' branch: {}",
                        env_name, e
                    ));
                    context.log_error(&format!(
                        "Someone may have pushed to '{}' while this rebuild ran, or the \
                         deploy key may be missing/outdated. Fetch and re-run 'hitch rebuild \
                         {}', or push once you've confirmed it's safe to overwrite: \
                         hitch push {} -f",
                        env_name, env_name, env_name
                    ));
                }
            }
        } else {
            context.log_info(&format!(
                "Skipping remote replacement for '{}' branch.",
                env_name
            ));
            context.log_info(&format!(
                "The local '{}' branch has been rebuilt. To push manually, run: hitch push {} -f",
                env_name, env_name
            ));
        }
    } else {
        context.log_verbose(&format!(
            "Skipping remote operations for '{}' branch due to --no-push flag",
            env_name
        ));
    }

    update_rebuilt_timestamp_for_rebuild(context, env_name)?;
    Ok(())
}

/// Attempt to compose a conflicting branch from a recorded resolution rather
/// than holding it (phase 5 replay). `outcome` holds the conflicted merge's
/// exact stage OIDs; this looks up a content-addressed resolution and — if one
/// exists and is authorized — splices the recorded resolved blobs into the
/// merge result tree and commits it onto `composed`. Returns the new composed
/// commit when the branch was composed this way, `None` on any miss/decline
/// (caller then holds/halts as usual). A hard error is only returned for an
/// unexpected git failure.
///
/// Nothing here touches a working tree or the real index: the splice goes
/// through a scratch index (`splice_blobs_into_tree`), so a failed replay
/// leaves no state to abort — the caller simply doesn't advance `composed`.
///
/// Safety (see docs/merge-conflict-handling-plan.md phase 5 hardening):
/// - The match is exact over `(path, base_oid, ours_oid, theirs_oid)`, so a
///   resolution only ever applies to byte-identical conflict inputs — any
///   change to any side is a miss, never a wrong replay.
/// - The whole feature is opt-in per invocation (`--replay-resolutions`),
///   a flag that cannot hide in `HITCH_YES`.
/// - Without `--yes`, each distinct resolution is confirmed once before it is
///   applied; under `--yes` (CI) the explicit flag is the authorization and
///   every application is logged loudly with its key and recorder.
fn try_replay_resolution(
    context: &GlobalContext,
    composed: &str,
    branch: &str,
    merge_message: &str,
    outcome: &crate::utils::git_operations::MergeTreeCompose,
    confirmed_keys: &mut std::collections::HashSet<String>,
) -> Result<Option<String>> {
    use crate::utils::resolutions;

    if outcome.conflicted_stages.is_empty() {
        return Ok(None);
    }
    let key = resolutions::resolution_key(context.git(), &outcome.conflicted_stages)?;
    let Some(res) = resolutions::load_resolution(context.git(), &key)? else {
        return Ok(None);
    };

    // A recorded resolution is content nobody on this machine reviewed,
    // spliced into a branch that deploys. When the repository has opted in,
    // it must carry a signature from a signer the repository trusts —
    // `recorded_by` alone is self-reported by whoever wrote the ref and
    // proves nothing.
    // Whether signing is even required is itself read from `hitch.json`, so a
    // failure to read metadata here (e.g. `check_metadata_health` reporting
    // hitch-metadata is behind remote) must not silently resolve to "not
    // required" — that would fail open on the one question that decides
    // whether the whole signature gate is active. Propagate instead: the
    // caller (`rebuild_environment_opts`) already aborts the whole rebuild on
    // any `Err` from this function (see the `?` at its call site), which
    // matches how a `Halt`-policy conflict already aborts the rebuild — this
    // is not a "hold this branch and keep going" situation, since we cannot
    // even determine whether replay is authorized.
    let require_signed =
        access_metadata_read_only(context, |config| Ok(config.require_signed_resolutions))?;
    if require_signed && !resolutions::verify_resolution_signature(context.git(), &res)? {
        context.log_warning(&format!(
            "Recorded resolution {} for '{}' is not signed by a trusted signer and this \
             repository requires signed resolutions — holding '{}' instead. To inspect it:\n  \
             hitch resolutions",
            &key[..12.min(key.len())],
            branch,
            branch
        ));
        return Ok(None);
    }

    // Authorize. The exact-OID match guarantees identical inputs, but a
    // recorded resolution is still someone else's content landing on a
    // deployable branch, so gate it: prompt once per key without --yes;
    // under --yes the explicit --replay-resolutions flag is the go-ahead and
    // we log loudly instead.
    if !confirmed_keys.contains(&key) {
        if context.assume_yes {
            context.log_warning(&format!(
                "♻️ Applying recorded resolution {} for '{}' (recorded by {} at {}) under \
                 --yes/--replay-resolutions.",
                &key[..12.min(key.len())],
                branch,
                res.meta.recorded_by,
                res.meta.recorded_at
            ));
        } else {
            let ok = context.confirm(&format!(
                "Apply recorded resolution {} for '{}' (by {} at {}) to '{}'?",
                &key[..12.min(key.len())],
                branch,
                res.meta.recorded_by,
                res.meta.recorded_at,
                res.meta.env
            ))?;
            if !ok {
                context.log_info(&format!(
                    "Declined resolution for '{}' — it will be held instead.",
                    branch
                ));
                return Ok(None);
            }
        }
        confirmed_keys.insert(key.clone());
    }

    // Apply: swap each conflicted path in the merge result tree for the
    // recorded resolved blob, then commit that tree. Every conflicted path
    // must be covered — a partial resolution would leave conflict markers in
    // a published tree, so it is treated as a miss and the branch is held.
    let apply = (|| -> Result<String> {
        let git = context.git();

        let mut entries = Vec::with_capacity(res.resolved.len());
        for (path, content) in &res.resolved {
            let blob = git.hash_object_bytes(content)?;
            entries.push((path.clone(), blob));
        }

        let unresolved: Vec<&str> = outcome
            .conflicted_stages
            .iter()
            .map(|(path, _, _, _)| path.as_str())
            .filter(|path| !res.resolved.iter().any(|(p, _)| p == path))
            .collect();
        if !unresolved.is_empty() {
            return Err(anyhow::anyhow!(
                "recorded resolution does not cover every conflicted path (missing: {})",
                unresolved.join(", ")
            ));
        }

        let tree = git.splice_blobs_into_tree(&outcome.tree_oid, &entries)?;
        git.commit_tree(&tree, &[composed], merge_message)
    })();

    match apply {
        Ok(new_composed) => {
            context.log_info(&format!(
                "♻️ Reused recorded resolution {} for '{}' (by {}).",
                &key[..12.min(key.len())],
                branch,
                res.meta.recorded_by
            ));
            Ok(Some(new_composed))
        }
        Err(e) => {
            context.log_verbose(&format!(
                "Could not apply recorded resolution for '{}': {} — holding instead.",
                branch, e
            ));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod try_replay_resolution_tests {
    use super::*;
    use crate::commands::global_context::GlobalContext;
    use crate::utils::git_operations::{MergeStages, MergeTreeCompose};
    use crate::utils::logging::Logger;
    use crate::utils::resolutions::{self, PendingConflict};
    use std::collections::HashSet;
    use std::sync::Arc;

    /// Test-only raw git invocation: this builds a throwaway repo to drive
    /// `try_replay_resolution` directly, not hitch's own automation surface,
    /// so it deliberately bypasses `GitOperations`/`run_git_command` (see the
    /// stdin-inheritance gotcha in AGENTS.md for why that convention exists
    /// elsewhere).
    #[allow(clippy::disallowed_methods)]
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Regression test for the bug this module's `require_signed` check used
    /// to have: reading `hitch.json` to decide whether signing is required
    /// used `.unwrap_or(false)`, so ANY failure to read metadata — including
    /// `check_metadata_health` reporting hitch-metadata is behind its
    /// remote-tracking ref, exactly the "someone pushed to it concurrently"
    /// condition that check exists to catch — silently resolved to "signing
    /// not required" instead of surfacing. That fails open on the one
    /// question that decides whether the whole signature gate is active.
    ///
    /// This constructs a repo where `refs/remotes/origin/hitch-metadata` is
    /// ahead of local `hitch-metadata` — no real "origin" remote or network
    /// fetch needed, since `is_branch_behind_remote` only ever compares those
    /// two local refs — and a recorded (unsigned) resolution matching a
    /// fabricated conflict, so `try_replay_resolution` gets past the
    /// `load_resolution` lookup and reaches the `require_signed` read. Before
    /// the fix this returned `Ok(None)` (silently held, signing bypassed);
    /// after the fix it must return `Err` mentioning the metadata problem.
    #[test]
    fn require_signed_check_fails_closed_on_metadata_health_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path();

        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.name", "Test User"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("README.md"), "hi\n")?;
        git(repo, &["add", "README.md"]);
        git(repo, &["commit", "-q", "-m", "init"]);

        // `hitch-metadata` branch, opted into signed resolutions.
        git(repo, &["checkout", "-q", "-b", "hitch-metadata"]);
        std::fs::write(
            repo.join("hitch.json"),
            r#"{"version":"1.0","environments":{},"require_signed_resolutions":true}"#,
        )?;
        git(repo, &["add", "hitch.json"]);
        git(repo, &["commit", "-q", "-m", "config"]);
        git(repo, &["checkout", "-q", "-"]);

        // Simulate "hitch-metadata moved on origin since we last synced":
        // a child commit of the local tip, referenced only via the local
        // remote-tracking ref — exactly what `check_metadata_health`'s
        // behind-remote check inspects, with no actual remote required.
        let metadata_sha = git(repo, &["rev-parse", "hitch-metadata"]);
        let tree = git(repo, &["rev-parse", &format!("{metadata_sha}^{{tree}}")]);
        let ahead_sha = git(
            repo,
            &[
                "commit-tree",
                &tree,
                "-p",
                &metadata_sha,
                "-m",
                "concurrent update",
            ],
        );
        git(
            repo,
            &[
                "update-ref",
                "refs/remotes/origin/hitch-metadata",
                &ahead_sha,
            ],
        );

        let logger = Arc::new(Logger::for_command("test", false));
        let context =
            GlobalContext::new_at_path(&repo.to_string_lossy(), false, true, true, logger)
                .expect("failed to build test GlobalContext");

        // A recorded (unsigned) resolution matching a fabricated conflict on
        // "shared.txt", so `try_replay_resolution` finds a hit and reaches
        // the `require_signed` check rather than returning early on a miss.
        let stages: Vec<MergeStages> = vec![(
            "shared.txt".to_string(),
            Some("base-oid".to_string()),
            Some("ours-oid".to_string()),
            Some("theirs-oid".to_string()),
        )];
        let resolved_dir = tempfile::tempdir()?;
        std::fs::write(resolved_dir.path().join("shared.txt"), "resolved\n")?;
        let pending = PendingConflict {
            env: "dev".to_string(),
            branch: "branch-b".to_string(),
            conflicts_with: "branch-a".to_string(),
            source_branch_head: metadata_sha.clone(),
            stages: stages.clone(),
        };
        resolutions::record_resolution(
            context.git(),
            &pending,
            resolved_dir.path(),
            "tester@example.com",
            "2026-01-01T00:00:00Z",
        )?;

        let outcome = MergeTreeCompose {
            tree_oid: tree,
            conflicted_stages: stages,
        };
        let mut confirmed = HashSet::new();

        let result = try_replay_resolution(
            &context,
            &metadata_sha,
            "branch-b",
            "merge message",
            &outcome,
            &mut confirmed,
        );

        let err = result.expect_err(
            "an unreadable/health-failing hitch-metadata during a replay attempt must surface \
             as an error, not silently disable the signing requirement",
        );
        assert!(
            err.to_string().contains("behind remote"),
            "expected the metadata health failure to surface, got: {}",
            err
        );

        Ok(())
    }
}

/// Update the rebuiltAt timestamp for an environment during rebuilding
fn update_rebuilt_timestamp_for_rebuild(context: &GlobalContext, env_name: &str) -> Result<()> {
    context.log_verbose("Updating rebuilt timestamp...");

    modify_metadata(context, |config| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        environment.update_rebuilt_timestamp();
        context.log_verbose(&format!("✓ Updated rebuilt timestamp for '{}'", env_name));

        Ok(())
    })?;

    Ok(())
}

// =============================================================================
// Approval Workflow Helper Functions
// =============================================================================

/// Create approval requests for a batch of branches atomically.
///
/// All requests are created inside a single metadata transaction. If creating a request
/// for any branch fails (e.g. one already has a pending request), the whole transaction
/// is rolled back and none are persisted — avoiding a partially-created set of approval
/// requests that would have to be cleaned up by hand. Returns the created request IDs in
/// input order.
pub fn create_approval_requests_for_operation(
    context: &GlobalContext,
    env_name: &str,
    branches: &[String],
    operation: crate::types::Operation,
) -> Result<Vec<String>> {
    let mut request_ids = Vec::new();
    modify_metadata(context, |config| {
        for branch in branches {
            let id = crate::utils::approvals::create_approval_request(
                context, config, env_name, branch, operation,
            )?;
            request_ids.push(id);
        }
        Ok(())
    })?;

    Ok(request_ids)
}

/// Get approval requests with optional filtering
pub fn get_approval_requests(
    context: &GlobalContext,
    environment: Option<&str>,
    status: Option<crate::types::ApprovalStatus>,
) -> Result<Vec<crate::types::ApprovalRequest>> {
    access_metadata_read_only(context, |config| {
        Ok(
            crate::utils::approvals::get_approval_requests(config, environment, status)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        )
    })
}

/// Get a specific approval request by ID
pub fn get_approval_request_by_id(
    context: &GlobalContext,
    request_id: &str,
) -> Result<crate::types::ApprovalRequest> {
    access_metadata_read_only(context, |config| {
        crate::utils::approvals::find_approval_request(config, request_id).cloned()
    })
}

/// Display approval request creation information
pub fn display_approval_request_created(context: &GlobalContext, request_id: &str) -> Result<()> {
    access_metadata_read_only(context, |config| {
        crate::utils::approvals::display_approval_request_info(context, config, request_id)
    })
}

/// Get environment configuration for approval checking
pub fn get_environment_config_for_approval(
    context: &GlobalContext,
    env_name: &str,
) -> Result<crate::types::Environment> {
    access_metadata_read_only(context, |config| {
        config
            .get_environment(env_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))
    })
}

// =============================================================================
// Compatibility Preflight (merge-tree)
// =============================================================================

/// Result describing the first blocking conflict in a sequential merge simulation.
#[derive(Debug, Clone)]
pub struct CompatibilityFailure {
    pub blocking_branch: String,
    pub base_branch: String,
    pub conflicted_files: Vec<String>,
}

/// Simulate sequential squash-merge compatibility using `git merge-tree --write-tree`.
///
/// This is a read-only preflight that:
/// - starts from `base_branch`'s tree
/// - applies each branch's tree in order via merge-tree
/// - returns the first branch that conflicts (with a list of conflicted files)
pub fn preflight_compatibility_merge_tree(
    context: &GlobalContext,
    base_branch: &str,
    branches_in_order: &[String],
) -> Result<Option<CompatibilityFailure>> {
    if branches_in_order.is_empty() {
        return Ok(None);
    }

    // Ensure all branches are available locally for rev-parse + merge-tree.
    let mut all = Vec::with_capacity(branches_in_order.len() + 1);
    all.push(base_branch.to_string());
    all.extend(branches_in_order.iter().cloned());
    context.git().synchronize_branches(&all)?;

    let base_commit = context.git().rev_parse(base_branch)?;
    let mut current_tree = context
        .git()
        .rev_parse(&format!("{}^{{tree}}", base_branch))?;

    for branch in branches_in_order {
        let their_tree = context.git().rev_parse(&format!("{}^{{tree}}", branch))?;
        // The true common ancestor of `base_branch` and `branch` — NOT
        // `base_branch`'s own current tip. Passing the tip as `--merge-base`
        // makes `git merge-tree` believe "our" side (the accumulated
        // composition) has zero changes since the merge-base, so it silently
        // fast-forwards to `branch`'s content instead of reporting a real
        // conflict — the exact scenario where `branch` conflicts with `base`
        // because base moved on independently after `branch` diverged.
        let merge_base = context
            .git()
            .get_merge_base(base_branch, branch)?
            .unwrap_or_else(|| base_commit.clone());
        let res = context.git().merge_tree_write_tree_name_only(
            &merge_base,
            &current_tree,
            &their_tree,
        )?;

        if !res.conflicted_files.is_empty() {
            return Ok(Some(CompatibilityFailure {
                blocking_branch: branch.clone(),
                base_branch: base_branch.to_string(),
                conflicted_files: res.conflicted_files,
            }));
        }

        current_tree = res.tree_oid;
    }

    Ok(None)
}

/// One branch's conflict, as found by `preflight_compatibility_report`.
#[derive(Debug, Clone)]
pub struct CompatibilityConflict {
    /// The branch that could not be folded into the composition.
    pub branch: String,
    /// What it conflicts with: the environment's base branch if this is the
    /// first branch (or every earlier branch was also skipped), or the last
    /// branch that composed successfully ahead of it otherwise. Distinguishing
    /// the two matters because the fix differs — rebase onto base, or resolve
    /// against the specific peer.
    pub conflicts_with: String,
    pub conflicted_files: Vec<String>,
}

/// Simulate composing `branches_in_order` onto `base_branch`, in the same
/// order and with the same read-only `git merge-tree` primitive as
/// `preflight_compatibility_merge_tree`, but reporting *every* branch that
/// cannot be folded in rather than stopping at the first one.
///
/// A conflicting branch is excluded from the running composition (not
/// merged, its tree left out) and later branches are checked against what
/// actually accumulated without it — so one preflight names every branch
/// that needs attention, instead of the caller re-running rebuild once per
/// conflict to discover the next one. This does not build or mutate
/// anything; it is the same kind of simulation as
/// `preflight_compatibility_merge_tree`, just exhaustive.
pub fn preflight_compatibility_report(
    context: &GlobalContext,
    base_branch: &str,
    branches_in_order: &[String],
) -> Result<Vec<CompatibilityConflict>> {
    let mut conflicts = Vec::new();
    if branches_in_order.is_empty() {
        return Ok(conflicts);
    }

    let mut all = Vec::with_capacity(branches_in_order.len() + 1);
    all.push(base_branch.to_string());
    all.extend(branches_in_order.iter().cloned());
    context.git().synchronize_branches(&all)?;

    let base_commit = context.git().rev_parse(base_branch)?;
    let mut current_tree = context
        .git()
        .rev_parse(&format!("{}^{{tree}}", base_branch))?;
    let mut last_composed = base_branch.to_string();

    for branch in branches_in_order {
        let their_tree = context.git().rev_parse(&format!("{}^{{tree}}", branch))?;
        // See the comment in `preflight_compatibility_merge_tree` — this
        // must be the true common ancestor of `base_branch` and `branch`,
        // not `base_branch`'s current tip, or a branch that conflicts with
        // base (because base moved on after it diverged) is missed entirely.
        let merge_base = context
            .git()
            .get_merge_base(base_branch, branch)?
            .unwrap_or_else(|| base_commit.clone());
        let res = context.git().merge_tree_write_tree_name_only(
            &merge_base,
            &current_tree,
            &their_tree,
        )?;

        if res.conflicted_files.is_empty() {
            current_tree = res.tree_oid;
            last_composed = branch.clone();
        } else {
            conflicts.push(CompatibilityConflict {
                branch: branch.clone(),
                conflicts_with: last_composed.clone(),
                conflicted_files: res.conflicted_files,
            });
        }
    }

    Ok(conflicts)
}

/// Local-only variant of `preflight_compatibility_report`, for callers like
/// `hitch status`/`hitch tree` that display per-branch state on every
/// invocation and must stay fast and offline — unlike `rebuild --dry-run` and
/// `hitch conflicts`, this never fetches. It works only from whatever is
/// already resolvable locally (existing local branches or remote-tracking
/// refs); a branch that isn't resolvable is silently skipped rather than
/// erroring, matching the `.unwrap_or(false)` best-effort style the rest of
/// `hitch status`'s per-branch checks already use — that branch's other
/// state (missing, stale, etc.) is what should explain it to the user, not
/// this function.
pub fn preflight_compatibility_report_local(
    context: &GlobalContext,
    base_branch: &str,
    branches_in_order: &[String],
) -> Vec<CompatibilityConflict> {
    let mut conflicts = Vec::new();
    if branches_in_order.is_empty() {
        return conflicts;
    }

    let Ok(base_commit) = context.git().rev_parse(base_branch) else {
        return conflicts;
    };
    let Ok(mut current_tree) = context
        .git()
        .rev_parse(&format!("{}^{{tree}}", base_branch))
    else {
        return conflicts;
    };
    let mut last_composed = base_branch.to_string();

    for branch in branches_in_order {
        let Ok(their_tree) = context.git().rev_parse(&format!("{}^{{tree}}", branch)) else {
            continue;
        };
        // See the comment in `preflight_compatibility_merge_tree` — must be
        // the true common ancestor, not `base_branch`'s current tip.
        let merge_base = context
            .git()
            .get_merge_base(base_branch, branch)
            .ok()
            .flatten()
            .unwrap_or_else(|| base_commit.clone());
        let Ok(res) =
            context
                .git()
                .merge_tree_write_tree_name_only(&merge_base, &current_tree, &their_tree)
        else {
            continue;
        };

        if res.conflicted_files.is_empty() {
            current_tree = res.tree_oid;
            last_composed = branch.clone();
        } else {
            conflicts.push(CompatibilityConflict {
                branch: branch.clone(),
                conflicts_with: last_composed.clone(),
                conflicted_files: res.conflicted_files,
            });
        }
    }

    conflicts
}

// =============================================================================
// Pre-promote Conflict Checking
// =============================================================================

/// Check whether a new branch conflicts with any branch already promoted to an environment
///
/// This runs before state is modified, so if a conflict is found no changes have been made.
/// It checks the new branch pairwise against every existing promoted branch by:
/// 1. Creating a throwaway branch from base_branch
/// 2. Squash-merging the existing promoted branch onto it
/// 3. Dry-run merging the new branch to detect conflicts
/// 4. Always cleaning up the throwaway branch
///
/// All conflicting pairs are collected and reported in a single error.
pub fn check_pre_promote_conflicts(
    context: &GlobalContext,
    new_branch: &str,
    existing_branches: &[String],
    base_branch: &str,
    env_name: &str,
) -> Result<()> {
    if existing_branches.is_empty() {
        return Ok(());
    }

    context.log_verbose(&format!(
        "Checking '{}' for conflicts with {} already-promoted branch(es) in '{}'...",
        new_branch,
        existing_branches.len(),
        env_name
    ));

    // Sequential compatibility simulation:
    // base + existing promoted branches (in order) + new branch (appended).
    let mut sequence = existing_branches.to_vec();
    sequence.push(new_branch.to_string());

    if let Some(failure) = preflight_compatibility_merge_tree(context, base_branch, &sequence)? {
        // If an existing branch blocks before we even reach `new_branch`, the environment is
        // already in a bad state (should be rare; typically prevented by earlier checks).
        if failure.blocking_branch != new_branch {
            return Err(anyhow::anyhow!(
                "Cannot promote '{}' to environment '{}': environment already contains incompatible promoted branches.\n\
                 First conflict occurs when merging '{}' onto '{}'.",
                new_branch,
                env_name,
                failure.blocking_branch,
                base_branch
            ));
        }

        let mut msg = format!(
            "Cannot promote '{}' to environment '{}': compatibility check failed.\n\n",
            new_branch, env_name
        );
        msg.push_str(&format!(
            "  {} conflicts with {}\n",
            new_branch, base_branch
        ));
        for f in &failure.conflicted_files {
            msg.push_str(&format!("    {}\n", f));
        }
        msg.push('\n');
        msg.push_str(&format!("Fix {} first:\n", new_branch));
        msg.push_str(&format!(
            "  git checkout {} && git rebase {}\n",
            new_branch, base_branch
        ));
        return Err(anyhow::anyhow!("{}", msg));
    }

    Ok(())
}
