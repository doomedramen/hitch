use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
use crate::utils::conflict_report::format_conflict_report;
use crate::utils::progress::StepLogger;
use anyhow::{Context, Result};

/// Reusable pre-check function for all commands
///
/// According to the specification:
/// - Ensure the current directory is a Git repository
/// - Ensure the working tree is clean (no unstaged or uncommitted changes)
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
            .add_and_commit(&["hitch.json"], "Update hitch configuration")?;

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

    // Lock the environment
    lock_environment(context, env_name)?;

    // Execute the closure
    let result = closure();

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
            context.log_verbose(&format!("Cleaning up backup branch '{}'", backup_branch));
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
        context.log_verbose(&format!("Cleaning up temporary branch '{}'", temp_branch));
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
    // and return the user to their original branch
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

            // Check if branch exists (now guaranteed by synchronize_branches, but keep for safety)
            if !context.git().branch_exists(branch)? {
                return Err(anyhow::anyhow!(
                    "Branch '{}' does not exist locally after synchronization",
                    branch
                ));
            }

            // Check for merge conflicts before attempting squash merge
            context.log_verbose(&format!(
                "Checking for merge conflicts in branch '{}'...",
                branch
            ));
            let conflict_result = context.git().check_merge_conflicts_comprehensive(branch)?;
            if conflict_result.has_conflicts {
                context.log_verbose(&format!("Merge conflicts detected in branch '{}'", branch));
                // Generate detailed conflict report
                let error_msg = format_conflict_report(
                    branch,
                    &conflict_result.target_branch,
                    base_branch,
                    env_name,
                    &conflict_result.conflicted_files,
                    conflict_result.merge_base.as_ref(),
                );

                return Err(anyhow::anyhow!("{}", error_msg));
            } else {
                context.log_verbose(&format!("No conflicts detected in branch '{}'", branch));
            }

            // Perform squash merge
            let merge_message = format!("hitch: squash merge '{}' into environment", branch);
            context.log_verbose(&format!(
                "Attempting to squash merge '{}' into temp branch...",
                branch
            ));
            context.git().squash_merge(branch, &merge_message)?;
            context.log_verbose(&format!(
                "✓ Successfully squash merged '{}' into temp branch",
                branch
            ));
        }

        Ok(())
    })();

    // Always try to return to original branch, even if an error occurred
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
