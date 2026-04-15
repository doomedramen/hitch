use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
use crate::utils::conflict_report::format_conflict_report;
use crate::utils::progress::StepLogger;
use crate::utils::resolve_state::{write_resolve_state, ResolveState};
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
/// If a conflict-resolution state is left in `.git/` after `f` returns (e.g.
/// the rebuild paused mid-way), the stash is NOT popped — the user will need
/// to pop it manually (or it will be popped by `hitch resolve --continue/--abort`
/// once the conflict is resolved).
pub fn with_auto_stash<F, R>(context: &GlobalContext, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    let is_clean = context.git().is_working_directory_clean()?;

    let stashed = if !is_clean {
        context.log_info("Auto-stashing local changes before operation...");
        let created = context
            .git()
            .stash_push("hitch: auto-stash before rebuild")?;
        if created {
            context.log_verbose("✓ Changes stashed");
        }
        created
    } else {
        false
    };

    let result = f();

    if stashed {
        let resolve_in_progress =
            crate::utils::resolve_state::resolve_state_exists(&context.git().get_git_dir());
        if resolve_in_progress {
            context.log_info(
                "Note: your stashed changes are saved. They will be restored after you run \
                 'hitch resolve --continue' or 'hitch resolve --abort'.",
            );
        } else {
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
    }

    result
}

/// Switch to a branch, execute a closure, and always return to the original branch
///
/// According to the specification:
/// - Record current branch
/// - Checkout target branch
/// - Execute the provided closure while on the target branch
/// - Always attempt to switch back to the original branch after the closure runs, even if it fails
pub fn switch_to<F, R>(context: &GlobalContext, target_branch: &str, closure: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    context.log_verbose(&format!("Switching to branch '{}'...", target_branch));

    // Record current branch
    let original_branch = context.git().get_current_branch()?;
    context.log_verbose(&format!("Current branch recorded: {}", original_branch));

    // CRITICAL: Clean up any uncommitted files before checkout
    if let Err(e) = context.git().abort_merge_and_clean() {
        context.log_warning(&format!(
            "Failed to reset working directory before checkout: {}",
            e
        ));
    }

    // Checkout target branch
    context.git().checkout_branch(target_branch)?;
    context.log_verbose(&format!("Switched to branch: {}", target_branch));

    // Execute the closure
    let result = closure();

    // Always attempt to switch back to original branch
    context.log_verbose(&format!(
        "Switching back to original branch: {}",
        original_branch
    ));

    // CRITICAL: Clean up any uncommitted files before returning
    if let Err(e) = context.git().abort_merge_and_clean() {
        context.log_warning(&format!(
            "Failed to reset working directory before returning: {}",
            e
        ));
    }

    // Handle detached HEAD case specially
    let checkout_result = if original_branch.starts_with("detached-HEAD-") {
        // Extract commit hash from detached-HEAD-abcdef1 format
        let commit_hash = &original_branch[13..]; // Remove "detached-HEAD-" prefix
        context.git().checkout_branch(commit_hash)
    } else {
        context.git().checkout_branch(&original_branch)
    };

    if let Err(e) = checkout_result {
        context.log_error(&format!(
            "Failed to return to original branch '{}': {}",
            original_branch, e
        ));
        context.log_warning("You may need to manually switch back to your original branch");
    } else {
        context.log_verbose(&format!(
            "Successfully returned to original branch: {}",
            original_branch
        ));
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

    // Read hitch.json using git show (no branch switching needed)
    // Try remote first (for fresh clones), then fallback to local
    context.log_verbose("Reading hitch.json from hitch-metadata branch...");
    let config_json = match context
        .git()
        .read_file_from_branch("origin/hitch-metadata", "hitch.json")
    {
        Ok(content) => {
            context.log_verbose("✓ Read from origin/hitch-metadata");
            content
        }
        Err(_) => {
            context
                .log_verbose("origin/hitch-metadata not available, trying local hitch-metadata...");
            context.git()
                .read_file_from_branch("hitch-metadata", "hitch.json")
                .context("Failed to read hitch.json from either origin/hitch-metadata or local hitch-metadata branch")?
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
/// - Temporarily switch to the hitch-metadata branch using switch-to
/// - Load and parse hitch.json
/// - Execute closure with mutable metadata object (for modification)
/// - Commit and optionally push changes (warn if push fails or skip with --no-push)
/// - Always switch back to the original branch afterward
pub fn modify_metadata<F>(context: &GlobalContext, closure: F) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    context.log_verbose("Accessing hitch metadata...");

    // Check if we're already on hitch-metadata branch (for init case)
    let already_on_metadata_branch = context
        .git()
        .get_current_branch()
        .map(|branch| branch == "hitch-metadata")
        .unwrap_or(false);

    if !already_on_metadata_branch {
        // Check hitch-metadata health before modifying
        // Skip this check during init (when already_on_metadata_branch is true)
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

    let modification_closure = || {
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

        context.log_verbose("✓ hitch.json loaded successfully");

        // Execute closure (for modification)
        context.log_verbose("Modifying metadata...");
        closure(&mut config)?;

        // Write updated config
        context.log_verbose("Writing updated hitch.json...");
        context
            .git()
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;

        // Commit changes
        context.log_verbose("Committing metadata changes...");
        context
            .git()
            .add_and_commit(&["hitch.json", ".gitignore"], "Update hitch configuration")?;

        // Optionally push
        if context.should_push() {
            context.log_verbose("Pushing metadata to remote...");
            if let Err(e) = context.git().push_branch("hitch-metadata") {
                context.log_warning(&format!("Failed to push metadata to remote: {}", e));
            } else {
                context.log_verbose("✓ Metadata pushed to remote");
            }
        } else {
            context.log_verbose("Skipping push due to --no-push flag");
        }

        Ok(())
    };

    if already_on_metadata_branch {
        context.log_verbose("Already on hitch-metadata branch, proceeding with metadata access...");
        modification_closure()
    } else {
        switch_to(context, "hitch-metadata", modification_closure)
    }
}

/// Modify metadata with rollback tracking
///
/// This function extends modify_metadata by capturing the pre-operation state
/// for potential rollback purposes. It works the same as modify_metadata
/// but also captures rollback information.
pub fn modify_metadata_with_rollback<F>(
    context: &GlobalContext,
    rollback_info: &mut crate::types::RollbackInfo,
    closure: F,
) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    context.log_verbose("Accessing hitch metadata with rollback tracking...");

    // Capture current commit SHA before making any changes
    let commit_before = crate::utils::rollback::capture_current_commit_sha(context)?;
    rollback_info.metadata_commit_before = Some(commit_before);

    // Execute the normal modify_metadata function
    let result = modify_metadata(context, closure);

    match &result {
        Ok(()) => {
            context.log_verbose("✓ Metadata modified successfully, rollback info captured");
            Ok(())
        }
        Err(e) => {
            context.log_verbose(&format!("Metadata modification failed: {}", e));
            // Clear rollback info since modification didn't succeed
            rollback_info.metadata_commit_before = None;
            result
        }
    }
}

/// Execute operations within a locked environment context
///
/// According to the specification:
/// - Calls lock(env_name) → executes closure → calls unlock(env_name) even if closure fails
/// - Ensures environment is safely locked during modifications
/// - Automatically handles warnings if push fails
///
/// EXCEPTION: When a rebuild conflict is in progress (resolve state file exists) we
/// deliberately skip the automatic unlock.  Unlocking requires switching to the
/// `hitch-metadata` branch, which would reset the working tree and destroy the conflict
/// markers the user needs to see.  The environment is unlocked later by
/// `hitch resolve --continue` or `hitch resolve --abort`.
pub fn with_locked_env<F, R>(context: &GlobalContext, env_name: &str, closure: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    context.log_verbose(&format!("Locking environment '{}'...", env_name));

    // Lock the environment
    lock_environment(context, env_name)?;

    // Execute the closure
    let result = closure();

    // Skip automatic unlock when a conflict resolution is in progress.
    // The resolve commands own the unlock at that point.
    let resolve_in_progress =
        crate::utils::resolve_state::resolve_state_exists(&context.git().get_git_dir());
    if resolve_in_progress {
        context.log_verbose(
            "Resolve state detected — skipping automatic unlock. \
             Environment will be unlocked when you run 'hitch resolve --continue' or '--abort'.",
        );
        return result;
    }

    // Always unlock the environment, even if closure failed
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
    let mut logger = StepLogger::new(
        format!("Rebuilding environment '{}'", env_name),
        total_steps,
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
    //
    // EXCEPTION: When a merge conflict is in progress (resolve state file exists),
    // we deliberately skip cleanup so the user can run `hitch resolve --continue`
    // or `hitch resolve --abort` to resume/abandon the rebuild.
    let resolve_state_present =
        crate::utils::resolve_state::resolve_state_exists(&context.git().get_git_dir());
    if cleanup_needed && !resolve_state_present {
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
            let checkout_result = if user_original_branch.starts_with("detached-HEAD-") {
                // Extract commit hash from detached-HEAD-abcdef1 format
                let commit_hash = &user_original_branch[13..]; // Remove "detached-HEAD-" prefix
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
        let checkout_result = if user_original_branch.starts_with("detached-HEAD-") {
            // Extract commit hash from detached-HEAD-abcdef1 format
            let commit_hash = &user_original_branch[13..]; // Remove "detached-HEAD-" prefix
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
/// When a squash merge conflicts this function:
/// 1. Leaves the working tree with conflict markers on `temp_branch`
/// 2. Writes `.git/hitch-resolve-state.json` so `hitch resolve` can resume
/// 3. Returns the user to their original branch
/// 4. Returns a human-readable error telling them to run `hitch resolve`
///
/// The caller (`rebuild_environment`) detects the resolve-state file and
/// skips the normal "abort + delete temp branch" cleanup so the conflict
/// remains accessible.
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

        let mut merged_so_far: Vec<String> = Vec::new();

        for (idx, branch) in branches.iter().enumerate() {
            context.log_verbose(&format!("Processing branch '{}'", branch));

            if !context.git().branch_exists(branch)? {
                return Err(anyhow::anyhow!(
                    "Branch '{}' does not exist locally after synchronization",
                    branch
                ));
            }

            // Dry-run check first so we can generate the conflict report without
            // actually leaving the repo in a half-merged state.
            context.log_verbose(&format!(
                "Checking for merge conflicts in branch '{}'...",
                branch
            ));
            let conflict_result = context.git().check_merge_conflicts_comprehensive(branch)?;

            if conflict_result.has_conflicts {
                context.log_verbose(&format!("Merge conflicts detected in branch '{}'", branch));

                // Now do the ACTUAL squash merge (without aborting) so the user
                // can see conflict markers in their working tree.
                let _ = context
                    .git()
                    .run_git_command(&["merge", "--squash", branch]);

                // Persist resolve state to .git/ so `hitch resolve` can resume
                let git_dir = context.git().get_git_dir();
                let remaining: Vec<String> = branches[idx..].to_vec();
                let state = ResolveState {
                    env_name: env_name.to_string(),
                    temp_branch: temp_branch.to_string(),
                    original_branch: original_branch.clone(),
                    base_branch: base_branch.to_string(),
                    merged_so_far: merged_so_far.clone(),
                    remaining_branches: remaining,
                    conflict_branch: branch.clone(),
                    reuse_resolutions: false,
                    rerere_restore: false,
                    rerere_original: None,
                };
                if let Err(e) = write_resolve_state(&git_dir, &state) {
                    context.log_warning(&format!("Failed to save resolve state: {}", e));
                }

                let conflict_report = format_conflict_report(
                    branch,
                    &conflict_result.target_branch,
                    base_branch,
                    env_name,
                    &conflict_result.conflicted_files,
                    conflict_result.merge_base.as_ref(),
                );

                return Err(anyhow::anyhow!(
                    "{}\n\nThe conflicted files are now open in your working tree on branch '{}'.\n\
                     Resolve the conflicts, then run:\n\
                     \n  hitch resolve --continue\n\
                     \nOr to abandon the rebuild:\n\
                     \n  hitch resolve --abort",
                    conflict_report,
                    temp_branch
                ));
            }

            context.log_verbose(&format!("No conflicts detected in branch '{}'", branch));

            let merge_message = format!("Hitch: merge {} into {}", branch, env_name);
            context.log_verbose(&format!(
                "Attempting to squash merge '{}' into temp branch...",
                branch
            ));
            context.git().squash_merge(branch, &merge_message)?;
            context.log_verbose(&format!(
                "✓ Successfully squash merged '{}' into temp branch",
                branch
            ));
            merged_so_far.push(branch.clone());
        }

        Ok(())
    })();

    // Always return to original branch (but DON'T abort the merge if we saved
    // resolve state — the caller checks for the state file).
    let resolve_state_present =
        crate::utils::resolve_state::resolve_state_exists(&context.git().get_git_dir());
    if !resolve_state_present {
        // No pending resolve: safe to clean up any dangling merge state
        let _ = context.git().abort_merge_and_clean();
        if let Err(e) = context.git().checkout_branch(&original_branch) {
            context.log_error(&format!(
                "Failed to return to original branch '{}' after merge operations: {}",
                original_branch, e
            ));
        }
    }
    // If resolve state IS present we intentionally leave the user on temp_branch
    // so they can see and edit the conflict markers in their working tree.

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

    // Step 4c: Handle remote branch replacement - always prompt when push is enabled
    if context.should_push() {
        // Interactive confirmation for force push

        println!(); // Add newline for clean separation
        context.log_warning(&format!(
            "Ready to force push the rebuilt '{}' branch to 'origin/{}'.",
            env_name, env_name
        ));
        context.log_warning(&format!(
            "This will OVERWRITE the remote '{}' branch with the new rebuilt version.",
            env_name
        ));
        context.log_warning("This action cannot be undone.");

        // Ask for user confirmation
        use std::io::{self, Write};
        print!("Do you want to proceed? [y/N]: ");
        io::stdout()
            .flush()
            .context("Failed to flush stdout for user prompt")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read user input")?;
        let input = input.trim().to_lowercase();

        if input == "y" || input == "yes" {
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

/// Create an approval request for a promote/demote operation
pub fn create_approval_request_for_operation(
    context: &GlobalContext,
    env_name: &str,
    branch_name: &str,
    operation: crate::types::Operation,
) -> Result<String> {
    context.log_verbose(&format!(
        "Creating approval request for {} operation: {} -> {}",
        operation, branch_name, env_name
    ));

    let mut request_id = String::new();
    modify_metadata(context, |config| {
        request_id = crate::utils::approvals::create_approval_request(
            context,
            config,
            env_name,
            branch_name,
            operation,
        )?;
        Ok(())
    })?;

    Ok(request_id)
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

    // Ensure all relevant branches are available locally before we start
    let mut all_branches = existing_branches.to_vec();
    all_branches.push(new_branch.to_string());
    context.git().synchronize_branches(&all_branches)?;

    let original_branch = context.git().get_current_branch()?;
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string();

    let mut conflict_pairs: Vec<(String, Vec<crate::utils::conflict_report::ConflictedFile>)> =
        Vec::new();

    for existing in existing_branches {
        let preflight_branch = format!("hitch-preflight-{}", timestamp);

        // Always clean up the preflight branch, even if an error occurs
        let pair_result: Result<Option<Vec<crate::utils::conflict_report::ConflictedFile>>> =
            (|| {
                // Create a fresh branch from base
                context
                    .git()
                    .create_branch_from(&preflight_branch, base_branch)?;

                // Squash-merge the existing promoted branch onto it (the "background" state)
                context.git().checkout_branch(&preflight_branch)?;
                let merge_msg = format!(
                    "hitch-preflight: squash merge '{}' for conflict check",
                    existing
                );
                context.git().squash_merge(existing, &merge_msg)?;

                // Now dry-run merge new_branch to see if it conflicts
                let result = context
                    .git()
                    .check_merge_conflicts_comprehensive(new_branch)?;
                if result.has_conflicts {
                    Ok(Some(result.conflicted_files))
                } else {
                    Ok(None)
                }
            })();

        // Return to original branch and clean up the preflight branch unconditionally
        let _ = context.git().abort_merge_and_clean();
        if let Err(e) = context.git().checkout_branch(&original_branch) {
            context.log_warning(&format!(
                "Failed to return to branch '{}' during preflight cleanup: {}",
                original_branch, e
            ));
        }
        if context
            .git()
            .branch_exists(&preflight_branch)
            .unwrap_or(false)
        {
            if let Err(e) = context.git().delete_branch(&preflight_branch, true) {
                context.log_warning(&format!(
                    "Failed to clean up preflight branch '{}': {}",
                    preflight_branch, e
                ));
            }
        }

        // Propagate hard errors (e.g. branch doesn't exist); collect conflict results
        match pair_result? {
            Some(conflicts) => conflict_pairs.push((existing.clone(), conflicts)),
            None => {
                context.log_verbose(&format!(
                    "  ✓ No conflict between '{}' and '{}'",
                    new_branch, existing
                ));
            }
        }
    }

    if conflict_pairs.is_empty() {
        return Ok(());
    }

    // Build a combined error message listing every conflicting pair
    let mut msg = format!(
        "Cannot promote '{}' to environment '{}': conflicts detected with already-promoted branches.\n\n",
        new_branch, env_name
    );
    for (sibling, files) in &conflict_pairs {
        msg.push_str(&format!(
            "  Conflicts with '{}' ({} file{}):\n",
            sibling,
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ));
        for f in files {
            msg.push_str(&format!("    - {} ({})\n", f.path, f.conflict_type));
        }
        msg.push('\n');
    }
    msg.push_str("Resolve by rebasing or merging the conflicting branches before promoting.\n");
    msg.push_str(
        "You can also use 'hitch resolve' after a failed rebuild to fix conflicts interactively.",
    );

    Err(anyhow::anyhow!("{}", msg))
}

// =============================================================================
// Resolve: continue / abort a paused rebuild
// =============================================================================

/// Complete a rebuild that was interrupted by a merge conflict.
///
/// Called by `hitch resolve --continue` after the user has resolved the conflict
/// markers left on `temp_branch` by the failed rebuild.  It:
/// 1. Commits the staged resolution on `temp_branch`.
/// 2. Squash-merges the remaining promoted branches (everything after the conflict branch).
/// 3. Replaces the real environment branch and updates the rebuilt timestamp.
/// 4. Cleans up `temp_branch` and removes `.git/hitch-resolve-state.json`.
/// 5. Checks out `original_branch`.
pub fn continue_rebuild_after_resolve(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<()> {
    let temp_branch = &state.temp_branch;
    let original_branch = &state.original_branch;
    let env_name = &state.env_name;

    // Must be on the temp branch for the commit to land in the right place
    let current = context.git().get_current_branch()?;
    if current != *temp_branch {
        return Err(anyhow::anyhow!(
            "Expected to be on branch '{}' but currently on '{}'. \
             Please switch to '{}' before running 'hitch resolve --continue'.",
            temp_branch,
            current,
            temp_branch
        ));
    }

    // Check for conflict markers in the working tree (unstaged)
    let wt_grep = context.git().run_git_command(&["grep", "-l", "^<<<<<<<"])?;
    if wt_grep.status.success() {
        let files = String::from_utf8_lossy(&wt_grep.stdout).trim().to_string();
        if !files.is_empty() {
            return Err(anyhow::anyhow!(
                "There are still conflict markers in your working tree.\n\
                 Please resolve all conflicts and stage the files before running 'hitch resolve --continue'.\n\
                 Files with conflict markers:\n  {}",
                files.lines().collect::<Vec<_>>().join("\n  ")
            ));
        }
    }

    // Check for conflict markers in staged files
    let staged_grep = context
        .git()
        .run_git_command(&["grep", "-l", "^<<<<<<<", "--cached"])?;
    if staged_grep.status.success() {
        let files = String::from_utf8_lossy(&staged_grep.stdout)
            .trim()
            .to_string();
        if !files.is_empty() {
            return Err(anyhow::anyhow!(
                "There are still conflict markers in staged files.\n\
                 Please resolve all conflicts and re-stage the files before running 'hitch resolve --continue'.\n\
                 Files with conflict markers:\n  {}",
                files.lines().collect::<Vec<_>>().join("\n  ")
            ));
        }
    }

    // Stage all changes (the resolved conflict files)
    let add_output = context.git().run_git_command(&["add", "-A"])?;
    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to stage resolved files: {}",
            stderr
        ));
    }

    // Commit the resolution
    let commit_msg = format!(
        "Hitch: merge {} into {} (conflicts resolved)",
        state.conflict_branch, env_name
    );
    let commit_output = context
        .git()
        .run_git_command(&["commit", "-m", &commit_msg])?;
    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        let stdout = String::from_utf8_lossy(&commit_output.stdout);
        let combined = format!("{}{}", stdout, stderr);
        // "nothing to commit" is acceptable — the resolution may have been staged already
        if !combined.contains("nothing to commit") && !combined.contains("nothing added to commit")
        {
            return Err(anyhow::anyhow!(
                "Failed to commit resolved conflicts: {}",
                combined
            ));
        }
    }

    context.log_success(&format!(
        "✓ Committed resolution for '{}'",
        state.conflict_branch
    ));

    // Continue squash-merging the remaining branches (skip the conflict_branch itself)
    let remaining = state
        .remaining_branches
        .iter()
        .skip(1) // first element is the conflict_branch we just resolved
        .cloned()
        .collect::<Vec<_>>();

    if !remaining.is_empty() {
        context.log_info(&format!(
            "Continuing rebuild: {} branch(es) remaining...",
            remaining.len()
        ));
        context.git().synchronize_branches(&remaining)?;

        for branch in &remaining {
            context.log_verbose(&format!("Squash-merging '{}'...", branch));

            // Dry-run check first
            let conflict_result = context.git().check_merge_conflicts_comprehensive(branch)?;
            if conflict_result.has_conflicts {
                // Write updated resolve state for the new conflict
                let git_dir = context.git().get_git_dir();
                let new_idx = state
                    .remaining_branches
                    .iter()
                    .position(|b| b == branch)
                    .unwrap_or(0);
                let new_remaining = state.remaining_branches[new_idx..].to_vec();
                let mut merged_so_far = state.merged_so_far.clone();
                // Add branches that we just successfully merged
                for b in &remaining {
                    if b == branch {
                        break;
                    }
                    merged_so_far.push(b.clone());
                }
                let new_state = crate::utils::resolve_state::ResolveState {
                    env_name: env_name.clone(),
                    temp_branch: temp_branch.clone(),
                    original_branch: original_branch.clone(),
                    base_branch: state.base_branch.clone(),
                    merged_so_far,
                    remaining_branches: new_remaining,
                    conflict_branch: branch.clone(),
                    reuse_resolutions: state.reuse_resolutions,
                    rerere_restore: state.rerere_restore,
                    rerere_original: state.rerere_original.clone(),
                };
                // Do the actual squash merge (leaving markers) then save state
                let _ = context
                    .git()
                    .run_git_command(&["merge", "--squash", branch]);
                if let Err(e) =
                    crate::utils::resolve_state::write_resolve_state(&git_dir, &new_state)
                {
                    context.log_warning(&format!("Failed to save resolve state: {}", e));
                }
                let conflict_report = format_conflict_report(
                    branch,
                    temp_branch,
                    &state.base_branch,
                    env_name,
                    &conflict_result.conflicted_files,
                    conflict_result.merge_base.as_ref(),
                );
                return Err(anyhow::anyhow!(
                    "{}\n\nThe conflicted files are now open in your working tree on branch '{}'.\n\
                     Resolve the conflicts, then run:\n\
                     \n  hitch resolve --continue\n\
                     \nOr to abandon the rebuild:\n\
                     \n  hitch resolve --abort",
                    conflict_report,
                    temp_branch
                ));
            }

            let merge_msg = format!("Hitch: merge {} into {}", branch, env_name);
            context.git().squash_merge(branch, &merge_msg)?;
            context.log_verbose(&format!("✓ Squash-merged '{}'", branch));
        }
    }

    // All branches merged — replace the real environment branch
    context.log_info(&format!("Finalising rebuild of '{}'...", env_name));
    let backup_branch =
        safe_replace_environment_branch_for_rebuild(context, env_name, temp_branch)?;

    update_rebuilt_timestamp_for_rebuild(context, env_name)?;

    // Cleanup: backup branch
    if context.git().branch_exists(&backup_branch)? {
        if let Err(e) = context.git().delete_branch(&backup_branch, true) {
            context.log_warning(&format!(
                "Failed to delete backup branch '{}': {}",
                backup_branch, e
            ));
        }
    }

    // Cleanup: temp branch
    if context.git().branch_exists(temp_branch)? {
        let _ = context
            .git()
            .delete_branch(temp_branch, false)
            .or_else(|_| context.git().delete_branch(temp_branch, true));
    }

    // Remove resolve state file
    let git_dir = context.git().get_git_dir();
    if let Err(e) = crate::utils::resolve_state::remove_resolve_state(&git_dir) {
        context.log_warning(&format!("Failed to remove resolve state file: {}", e));
    }

    // Return to original branch
    if let Err(e) = context.git().checkout_branch(original_branch) {
        context.log_warning(&format!(
            "Failed to return to '{}': {}. You may need to check it out manually.",
            original_branch, e
        ));
    } else {
        context.log_verbose(&format!("✓ Returned to '{}'", original_branch));
    }

    // Unlock the environment now that the rebuild is complete
    if let Err(e) = unlock_environment(context, env_name) {
        context.log_warning(&format!(
            "Failed to unlock environment '{}': {}. You may need to run 'hitch unlock {}' manually.",
            env_name, e, env_name
        ));
    }

    // Pop auto-stash if one was saved before the original rebuild command
    pop_auto_stash_if_present(context);

    Ok(())
}

/// Abort a rebuild that was interrupted by a merge conflict.
///
/// Called by `hitch resolve --abort`.  It resets the working tree, deletes the
/// temporary rebuild branch, removes the resolve state file, and checks out
/// the user's original branch.
pub fn abort_rebuild_resolve(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<()> {
    let temp_branch = &state.temp_branch;
    let original_branch = &state.original_branch;

    // Reset working tree (clears conflict markers / staged changes)
    let _ = context.git().run_git_command(&["merge", "--abort"]);
    let _ = context.git().run_git_command(&["reset", "--hard"]);
    let _ = context.git().run_git_command(&["clean", "-fd"]);

    // Checkout original branch (may already be there if resolve state was just created)
    let current = context.git().get_current_branch().unwrap_or_default();
    if current != *original_branch {
        if let Err(e) = context.git().checkout_branch(original_branch) {
            context.log_warning(&format!("Failed to return to '{}': {}", original_branch, e));
        }
    }

    // Delete temp branch
    if context.git().branch_exists(temp_branch).unwrap_or(false) {
        // Reset + clean again now that we're on a different branch
        let _ = context.git().run_git_command(&["reset", "--hard"]);
        if let Err(e) = context.git().delete_branch(temp_branch, true) {
            context.log_warning(&format!(
                "Failed to delete temp branch '{}': {}",
                temp_branch, e
            ));
        } else {
            context.log_verbose(&format!("✓ Deleted temp branch '{}'", temp_branch));
        }
    }

    // Remove resolve state file
    let git_dir = context.git().get_git_dir();
    if let Err(e) = crate::utils::resolve_state::remove_resolve_state(&git_dir) {
        context.log_warning(&format!("Failed to remove resolve state file: {}", e));
    }

    // Unlock the environment now that the rebuild is aborted
    let env_name = &state.env_name;
    if let Err(e) = unlock_environment(context, env_name) {
        context.log_warning(&format!(
            "Failed to unlock environment '{}': {}. You may need to run 'hitch unlock {}' manually.",
            env_name, e, env_name
        ));
    }

    // Pop auto-stash if one was saved before the original rebuild command
    pop_auto_stash_if_present(context);

    Ok(())
}

/// Pop the top git stash if its message indicates it was created by hitch's
/// auto-stash mechanism.  Called after `hitch resolve --continue/--abort` to
/// restore working-tree changes that were stashed before the interrupted rebuild.
fn pop_auto_stash_if_present(context: &GlobalContext) {
    const AUTO_STASH_PREFIX: &str = "On ";
    const AUTO_STASH_MSG: &str = "hitch: auto-stash before rebuild";
    if let Some(msg) = context.git().stash_top_message() {
        // git stash list --format=%s produces "On <branch>: <message>"
        if msg.contains(AUTO_STASH_MSG)
            || msg == AUTO_STASH_MSG
            || msg.starts_with(AUTO_STASH_PREFIX) && msg.contains(AUTO_STASH_MSG)
        {
            context.log_info("Restoring stashed changes from before the rebuild...");
            if let Err(e) = context.git().stash_pop() {
                context.log_warning(&format!(
                    "Failed to restore stashed changes: {}. Run 'git stash pop' manually.",
                    e
                ));
            } else {
                context.log_verbose("✓ Stashed changes restored");
            }
        }
    }
}
