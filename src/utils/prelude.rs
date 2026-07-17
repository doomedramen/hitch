use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
use crate::utils::conflict_report::format_conflict_report;
use crate::utils::progress::StepLogger;
use anyhow::{Context, Result};

/// Check that the hitch-metadata branch is in a healthy state
///
/// This function verifies that the hitch-metadata branch:
/// 1. Exists locally
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

    // 1. Check that hitch-metadata branch exists locally
    if !context.git().branch_exists("hitch-metadata")? {
        return Err(anyhow::anyhow!(
            "hitch-metadata branch does not exist locally. Run 'hitch init' to initialize Hitch."
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

    // Parse configuration
    let config: HitchConfig =
        serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

    // Validate configuration
    if let Err(validation_error) = config.validate() {
        return Err(anyhow::anyhow!(
            "Configuration validation failed: {}",
            validation_error
        ));
    }

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

    // The branch not existing yet is the init bootstrap case: there is
    // nothing to health-check or fetch, and the write below will create it
    // as a root commit.
    let branch_exists = context.git().branch_exists("hitch-metadata")?;

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
            serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

        // Validate configuration
        if let Err(validation_error) = config.validate() {
            return Err(anyhow::anyhow!(
                "Configuration validation failed: {}",
                validation_error
            ));
        }

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

/// Rebuild an environment by merging its promoted branches into a new environment branch
///
/// This is the core reusable rebuild function that can be called by promote, demote, or rebuild commands
///
/// According to SPEC.md:
/// - Step 1: Lock the environment (handled by caller using with_locked_env)
/// - Step 2: Prepare temp branch
/// - Step 3: Merge branches into temp branch
/// - Step 4: Merge temp branch into real environment branch
/// - Step 5: Update rebuiltAt timestamp (handled automatically on success)
/// - Automatic rollback on any failure
pub fn rebuild_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    context.log_verbose(&format!(
        "Starting rebuild process for environment '{}'",
        env_name
    ));

    // Acquire per-environment rebuild lock to prevent concurrent rebuilds.
    // The lock is released automatically when `_rebuild_lock` goes out of scope.
    let git_dir = std::path::PathBuf::from(context.git().get_git_dir());
    let _rebuild_lock = crate::utils::rebuild_lock::RebuildLock::acquire(&git_dir, env_name)?;

    // Record original branch for cleanup - this is the branch the user was on when we started
    // We always return users to their original branch, even if the rebuild fails
    let user_original_branch = context.git().get_current_branch()?;
    context.log_verbose(&format!(
        "Recorded user's original branch: {}",
        user_original_branch
    ));
    let mut cleanup_needed = false;

    // Get environment configuration to understand how to rebuild
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config
        .environments
        .get(env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", env_name))?;

    // Set up step logging
    let base_steps = 4; // Initialize, create temp, merge/replace, cleanup
    let merge_steps = if environment.branches.is_empty() {
        1
    } else {
        environment.branches.len()
    };
    let total_steps = base_steps + merge_steps;
    let mut logger = StepLogger::new_with_output(
        format!("Rebuilding environment '{}'", env_name),
        total_steps,
        context.output.clone(),
    );

    // Step 1: Initialize
    logger.step("Initializing rebuild".to_string());

    // Step 2: Prepare temp branch with timestamp to avoid conflicts
    // The temp branch allows us to build the new environment state without affecting the real branch
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let temp_branch = format!("hitch-tmp-{}-{}", environment.base, timestamp);

    // Use a closure to ensure proper cleanup even if an error occurs
    let result = (|| -> Result<()> {
        logger.step(format!(
            "Creating temporary branch from '{}'",
            environment.base
        ));
        create_temp_branch_for_rebuild(context, &temp_branch, &environment.base)?;
        cleanup_needed = true; // Mark that we have something to clean up

        // Step 3: Merge all promoted branches into temp branch using squash merges
        // Squash merges combine all changes without creating merge commits, keeping history clean
        if !environment.branches.is_empty() {
            logger.step(format!(
                "Merging {} promoted branches",
                environment.branches.len()
            ));
            context.log_verbose(&format!("Branches to merge: {:?}", environment.branches));
            perform_squash_merges_for_rebuild(
                context,
                &temp_branch,
                &environment.branches,
                &environment.base,
                env_name,
            )?;
        } else {
            logger.step("No promoted branches to merge".to_string());
        }

        // Step 4: Replace the real environment branch with our rebuilt temp branch
        // This creates a backup first, then atomically replaces the branch
        logger.step(format!(
            "Replacing '{}' branch with rebuilt content",
            env_name
        ));
        let backup_branch =
            safe_replace_environment_branch_for_rebuild(context, env_name, &temp_branch)?;

        // Update rebuiltAt timestamp on success
        update_rebuilt_timestamp_for_rebuild(context, env_name)?;

        // Cleanup Step 1: Delete backup branch (it was created as a safety net during replacement)
        // We use force delete since the backup might have been merged or have other references
        let mut cleanup_errors = Vec::new();
        if context.git().branch_exists(&backup_branch)? {
            logger.step(format!("Cleaning up backup branch '{}'", backup_branch));
            if let Err(e) = context.git().delete_branch(&backup_branch, true) {
                let error_msg =
                    format!("Failed to delete backup branch '{}': {}", backup_branch, e);
                context.log_warning(&error_msg);
                cleanup_errors.push(error_msg);
            }
        } else {
            context.log_verbose(&format!(
                "No backup branch '{}' to clean up (first rebuild)",
                backup_branch
            ));
        }

        // Cleanup Step 2: Delete the temporary branch we used for rebuilding
        // Try regular delete first (if branch is fully merged), then force delete if needed
        logger.step(format!("Cleaning up temporary branch '{}'", temp_branch));
        if let Err(_e) = context.git().delete_branch(&temp_branch, false) {
            // Regular delete failed, likely because branch wasn't merged
            // Try force delete to remove it anyway
            context.log_verbose(&format!(
                "Regular delete failed, trying force delete for '{}'",
                temp_branch
            ));
            if let Err(e2) = context.git().delete_branch(&temp_branch, true) {
                let error_msg = format!("Failed to delete temp branch '{}': {}", temp_branch, e2);
                context.log_warning(&error_msg);
                cleanup_errors.push(error_msg);
            }
        }

        // Report overall success/failure based on cleanup results
        if cleanup_errors.is_empty() {
            context.log_verbose(&format!(
                "✓ Successfully cleaned up all temporary branches for environment '{}'",
                env_name
            ));
        } else {
            context.log_warning(&format!(
                "Cleanup completed with {} error(s) for environment '{}'",
                cleanup_errors.len(),
                env_name
            ));
            for error in &cleanup_errors {
                context.log_warning(&format!("  {}", error));
            }
        }

        cleanup_needed = false; // Mark that cleanup is complete
        Ok(())
    })();

    // CRITICAL: Ensure cleanup happens even if rebuild fails
    // This is the error recovery path - we must always clean up temp branches
    // and return the user to their original branch.
    if cleanup_needed {
        context.log_warning("Rebuild failed, performing cleanup...");

        // Determine current branch to decide if we need to switch back
        let current_branch = match context.git().get_current_branch() {
            Ok(branch) => branch,
            Err(_) => {
                context.log_error("Failed to get current branch during cleanup");
                // Assume we need to switch back if we can't determine current branch
                user_original_branch.clone()
            }
        };

        // If we're not on the user's original branch, we need to switch back
        if current_branch != user_original_branch {
            context.log_info(&format!(
                "Returning to user's original branch '{}'",
                user_original_branch
            ));

            // CRITICAL: Abort any ongoing merge before checking out
            // Git will refuse checkout if there are unresolved merge conflicts
            if let Err(e) = context.git().abort_merge_and_clean() {
                context.log_warning(&format!(
                    "Failed to reset working directory before checkout: {}",
                    e
                ));
            }

            // Handle detached HEAD case specially
            let checkout_result =
                if let Some(commit_hash) = user_original_branch.strip_prefix("detached-HEAD-") {
                    // Extract commit hash from detached-HEAD-abcdef1 format
                    context.git().checkout_branch(commit_hash)
                } else {
                    context.git().checkout_branch(&user_original_branch)
                };

            if let Err(e) = checkout_result {
                context.log_error(&format!(
                    "Failed to return to user's original branch '{}': {}",
                    user_original_branch, e
                ));
            }
        }

        // Clean up temp branch if it exists
        if context.git().branch_exists(&temp_branch)? {
            context.log_info(&format!("Cleaning up failed temp branch '{}'", temp_branch));

            // First, abort any ongoing merge and reset working directory to clean state
            if let Err(e) = context.git().abort_merge_and_clean() {
                context.log_warning(&format!(
                    "Failed to reset working directory during cleanup: {}",
                    e
                ));
            }

            // Now try to delete the temp branch
            if let Err(e) = context.git().delete_branch(&temp_branch, true) {
                context.log_warning(&format!(
                    "Failed to delete temp branch '{}': {}",
                    temp_branch, e
                ));
            }
        }
    }

    // Propagate the original result
    result?;

    // Ensure we're back on the user's original branch after successful rebuild
    let current_branch = match context.git().get_current_branch() {
        Ok(branch) => branch,
        Err(_) => {
            context.log_warning("Failed to get current branch after rebuild");
            user_original_branch.clone()
        }
    };

    if current_branch != user_original_branch {
        context.log_info(&format!(
            "Returning to user's original branch '{}'",
            user_original_branch
        ));

        // Handle detached HEAD case specially
        let checkout_result =
            if let Some(commit_hash) = user_original_branch.strip_prefix("detached-HEAD-") {
                // Extract commit hash from detached-HEAD-abcdef1 format
                context.git().checkout_branch(commit_hash)
            } else {
                context.git().checkout_branch(&user_original_branch)
            };

        if let Err(e) = checkout_result {
            context.log_warning(&format!(
                "Failed to return to user's original branch '{}': {}",
                user_original_branch, e
            ));
        } else {
            context.log_verbose(&format!(
                "✓ Returned to user's original branch '{}'",
                user_original_branch
            ));
        }
    }

    logger.complete();

    context.log_verbose(&format!(
        "✓ Rebuild process completed for environment '{}'",
        env_name
    ));
    Ok(())
}

/// Create a temporary branch from the base branch for rebuilding
fn create_temp_branch_for_rebuild(
    context: &GlobalContext,
    temp_branch: &str,
    base_branch: &str,
) -> Result<()> {
    // Ensure base branch exists
    if !context.git().branch_exists_anywhere(base_branch)? {
        return Err(anyhow::anyhow!(
            "Base branch '{}' does not exist",
            base_branch
        ));
    }

    // Synchronize base branch - ensure it's available locally
    context
        .git()
        .synchronize_branches(&[base_branch.to_string()])?;
    context.log_verbose(&format!("✓ Synchronized base branch '{}'", base_branch));

    // Create temp branch from base branch
    context.git().create_branch_from(temp_branch, base_branch)?;
    context.log_verbose(&format!(
        "✓ Created temporary branch '{}' from '{}'",
        temp_branch, base_branch
    ));

    Ok(())
}

/// Perform squash merges of promoted branches into temp branch for rebuilding
///
/// When a squash merge would conflict, this function returns a detailed conflict report
/// without leaving any mid-operation resolve state behind.
fn perform_squash_merges_for_rebuild(
    context: &GlobalContext,
    temp_branch: &str,
    branches: &[String],
    base_branch: &str,
    env_name: &str,
) -> Result<()> {
    // Record original branch
    let original_branch = context.git().get_current_branch()?;

    // Synchronize branches before starting merge operations
    context.log_info("Synchronizing remote branches...");
    context.git().synchronize_branches(branches)?;
    context.log_info("✓ Branch synchronization complete");

    // Use a closure to ensure we always return to original branch
    let result = (|| -> Result<()> {
        // Switch to temp branch
        context.git().checkout_branch(temp_branch)?;

        // Clean working directory to avoid issues with untracked files
        if !context.git().is_working_directory_clean()? {
            context.log_verbose("Cleaning working directory before merge operations");
            context
                .git()
                .clean_working_directory("Clean up before rebuild operations")?;
        }

        for branch in branches {
            context.log_verbose(&format!("Processing branch '{}'", branch));

            if !context.git().branch_exists(branch)? {
                return Err(anyhow::anyhow!(
                    "Branch '{}' does not exist locally after synchronization",
                    branch
                ));
            }

            // Perform the squash merge directly. Previously this did a separate
            // "dry-run" `merge --no-ff` conflict check and then a second real
            // `merge --squash` — merging every branch twice. Instead we do the one
            // real merge and, if it conflicts, build the report from the state it
            // left behind (the trailing abort_merge_and_clean below then clears it).
            let merge_message = format!("Hitch: merge {} into {}", branch, env_name);
            context.log_verbose(&format!(
                "Attempting to squash merge '{}' into temp branch...",
                branch
            ));

            match context.git().squash_merge(branch, &merge_message) {
                Ok(()) => {
                    context.log_verbose(&format!(
                        "✓ Successfully squash merged '{}' into temp branch",
                        branch
                    ));
                }
                Err(e) => {
                    // Distinguish a genuine merge conflict (report it clearly) from
                    // any other failure (propagate as-is).
                    if context.git().has_merge_conflicts().unwrap_or(false) {
                        context.log_verbose(&format!(
                            "Merge conflicts detected in branch '{}'",
                            branch
                        ));

                        let conflict_result = context.git().current_conflict_result(branch)?;
                        let conflict_report = format_conflict_report(
                            branch,
                            &conflict_result.target_branch,
                            base_branch,
                            env_name,
                            &conflict_result.conflicted_files,
                            conflict_result.merge_base.as_ref(),
                        );

                        return Err(anyhow::anyhow!("{}", conflict_report));
                    }

                    return Err(e);
                }
            }
        }

        Ok(())
    })();

    // Always return to original branch and clean up any dangling merge state.
    let _ = context.git().abort_merge_and_clean();
    if let Err(e) = context.git().checkout_branch(&original_branch) {
        context.log_error(&format!(
            "Failed to return to original branch '{}' after merge operations: {}",
            original_branch, e
        ));
    }

    result
}

/// Safely replace environment branch with temp branch for rebuilding
fn safe_replace_environment_branch_for_rebuild(
    context: &GlobalContext,
    env_name: &str,
    temp_branch: &str,
) -> Result<String> {
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let backup_branch = format!("hitch-backup-{}-{}", env_name, timestamp);

    // Record original branch
    let original_branch = context.git().get_current_branch()?;

    // Step 4a: Rename current environment branch to backup
    if context.git().branch_exists(env_name)? {
        context.log_verbose(&format!(
            "Backing up '{}' branch to '{}'",
            env_name, backup_branch
        ));
        context.git().rename_branch(env_name, &backup_branch)?;
    }

    // Step 4b: Create new environment branch from temp branch
    context.log_verbose(&format!(
        "Creating new '{}' branch from temp branch",
        env_name
    ));
    context.git().create_branch_from(env_name, temp_branch)?;

    // Step 4c: Handle remote branch replacement - prompt/confirm when push is enabled
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

        if context.confirm.confirm_force_push_rebuild(env_name)? {
            context.log_info(&format!(
                "Force pushing rebuilt '{}' branch to replace remote",
                env_name
            ));
            if let Err(e) = context.git().force_push_branch(env_name) {
                context.log_error(&format!(
                    "Failed to force push rebuilt '{}' branch: {}",
                    env_name, e
                ));
                context.log_error(&format!(
                    "You may need to manually run: git push origin {} --force",
                    env_name
                ));
            } else {
                context.log_success(&format!(
                    "✓ Force pushed rebuilt '{}' branch to remote",
                    env_name
                ));
            }
        } else {
            context.log_info(&format!(
                "Skipping remote replacement for '{}' branch.",
                env_name
            ));
            context.log_info(&format!(
                "The local '{}' branch has been rebuilt. To push manually, run: git push origin {} --force",
                env_name, env_name
            ));
        }
    } else {
        context.log_verbose(&format!(
            "Skipping remote operations for '{}' branch due to --no-push flag",
            env_name
        ));
    }

    // Return to original branch
    context.git().checkout_branch(&original_branch)?;

    context.log_verbose(&format!("✓ Successfully replaced '{}' branch", env_name));
    Ok(backup_branch)
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
        let res = context.git().merge_tree_write_tree_name_only(
            &base_commit,
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
