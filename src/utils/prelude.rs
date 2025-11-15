use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;
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
    context.log_verbose("Reading hitch.json from hitch-metadata branch...");
    let config_json = context
        .git()
        .read_file_from_branch("hitch-metadata", "hitch.json")
        .context("Failed to read hitch.json from hitch-metadata branch")?;

    // Parse configuration
    let config: HitchConfig =
        serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

    context.log_verbose("✓ hitch.json loaded successfully (read-only)");

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
    let current_branch = context.git().get_current_branch();
    let already_on_metadata_branch =
        current_branch.is_ok() && current_branch.unwrap() == "hitch-metadata";

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
        let config_json = context
            .git()
            .read_file_from_branch("hitch-metadata", "hitch.json")
            .unwrap_or_else(|e| {
                // If file doesn't exist, create default config
                context.log_info(&format!(
                    "hitch.json not found or unreadable ({}), creating default configuration",
                    e
                ));
                let default_config = HitchConfig::new();
                serde_json::to_string_pretty(&default_config).unwrap()
            });

        let mut config: HitchConfig =
            serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

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
    let user_original_branch = context.git().get_current_branch()?;
    context.log_verbose(&format!(
        "Recorded user's original branch: {}",
        user_original_branch
    ));
    let mut cleanup_needed = false;

    // Get environment configuration
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config
        .environments
        .get(env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", env_name))?;

    // Step 2: Prepare temp branch
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let temp_branch = format!("hitch-tmp-{}-{}", environment.base, timestamp);

    let result = (|| -> Result<()> {
        context.log_info(&format!(
            "Creating temporary branch '{}' from '{}'",
            temp_branch, environment.base
        ));
        create_temp_branch_for_rebuild(context, &temp_branch, &environment.base)?;
        cleanup_needed = true;

        // Step 3: Merge branches into temp branch
        if !environment.branches.is_empty() {
            context.log_info("Merging promoted branches into temporary branch...");
            perform_squash_merges_for_rebuild(context, &temp_branch, &environment.branches)?;
        } else {
            context.log_info("No branches promoted to this environment, using base branch only");
        }

        // Step 4: Replace environment branch with temp branch
        context.log_info(&format!(
            "Replacing '{}' branch with rebuilt content",
            env_name
        ));
        let backup_branch =
            safe_replace_environment_branch_for_rebuild(context, env_name, &temp_branch)?;

        // Update rebuiltAt timestamp on success
        update_rebuilt_timestamp_for_rebuild(context, env_name)?;

        // Cleanup: delete backup branch (force delete since it's a backup we don't need)
        // Only try to delete if the backup branch actually exists
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

        // Cleanup: delete temp branch (try regular delete first, then force if needed)
        context.log_verbose(&format!("Cleaning up temporary branch '{}'", temp_branch));
        if let Err(_e) = context.git().delete_branch(&temp_branch, false) {
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

        cleanup_needed = false;
        Ok(())
    })();

    // Ensure cleanup happens even if rebuild fails
    if cleanup_needed {
        context.log_warning("Rebuild failed, performing cleanup...");

        // Return to user's original branch if we're not already there
        let current_branch = match context.git().get_current_branch() {
            Ok(branch) => branch,
            Err(_) => {
                context.log_error("Failed to get current branch during cleanup");
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
                context.log_error(&format!(
                    "Failed to return to user's original branch '{}': {}",
                    user_original_branch, e
                ));
            }
        }

        // Clean up temp branch if it exists
        if context.git().branch_exists(&temp_branch)? {
            context.log_info(&format!("Cleaning up failed temp branch '{}'", temp_branch));
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
) -> Result<()> {
    // Record original branch
    let original_branch = context.git().get_current_branch()?;

    // Switch to temp branch
    context.git().checkout_branch(temp_branch)?;

    for branch in branches {
        context.log_verbose(&format!("Processing branch '{}'", branch));

        // Check if branch exists
        if !context.git().branch_exists_anywhere(branch)? {
            return Err(anyhow::anyhow!("Branch '{}' does not exist", branch));
        }

        // Check for merge conflicts before attempting squash merge
        if context.git().check_merge_conflicts(branch)? {
            return Err(anyhow::anyhow!(
                "Merge conflict detected when merging branch '{}'. \
                 Please resolve conflicts before rebuilding.",
                branch
            ));
        }

        // Perform squash merge
        let merge_message = format!("hitch: squash merge '{}' into environment", branch);
        context.git().squash_merge(branch, &merge_message)?;
        context.log_verbose(&format!("✓ Squash merged '{}' into temp branch", branch));
    }

    // Return to original branch
    context.git().checkout_branch(&original_branch)?;

    Ok(())
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
