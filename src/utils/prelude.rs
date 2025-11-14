use anyhow::{Result, Context};
use colored::*;
use crate::commands::global_context::GlobalContext;
use crate::types::HitchConfig;

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
pub fn switch_to<F, R>(
    context: &GlobalContext,
    target_branch: &str,
    closure: F,
) -> Result<R>
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
    context.log_verbose(&format!("Switching back to original branch: {}", original_branch));
    if let Err(e) = context.git().checkout_branch(&original_branch) {
        context.log_error(&format!("Failed to return to original branch '{}': {}", original_branch, e));
        context.log_warning("You may need to manually switch back to your original branch");
    } else {
        context.log_verbose(&format!("Successfully returned to original branch: {}", original_branch));
    }

    result
}

/// Access metadata from hitch-metadata branch with optional modification
///
/// According to the specification:
/// - Fetch latest hitch-metadata from remote: git fetch origin hitch-metadata
/// - Temporarily switch to the hitch-metadata branch using switch-to
/// - Load and parse hitch.json
/// - If a closure is provided, execute it with the metadata object (for modification)
/// - Any changes should be committed and optionally pushed (warn if push fails or skip with --no-push)
/// - Return metadata (updated if modified, or original if read-only)
/// - Always switch back to the original branch afterward
pub fn access_metadata<F>(context: &GlobalContext, closure: F) -> Result<()>
where
    F: FnOnce(&mut HitchConfig) -> Result<()>,
{
    context.log_verbose("Accessing hitch metadata...");

    // Fetch latest hitch-metadata from remote
    context.log_verbose("Fetching latest hitch-metadata from remote...");
    if let Err(e) = context.git().fetch_branch("hitch-metadata") {
        context.log_warning(&format!("Failed to fetch hitch-metadata from remote: {}", e));
        context.log_warning("Continuing with local metadata...");
    }

    switch_to(context, "hitch-metadata", || {
        // Load and parse hitch.json
        context.log_verbose("Loading hitch.json...");
        let config_json = context.git().read_file_from_branch("hitch-metadata", "hitch.json")
            .unwrap_or_else(|e| {
                // If file doesn't exist, create default config
                context.log_info(&format!("hitch.json not found or unreadable ({}), creating default configuration", e));
                let default_config = HitchConfig::new();
                serde_json::to_string_pretty(&default_config).unwrap()
            });

        let mut config: HitchConfig = serde_json::from_str(&config_json)
            .context("Failed to parse hitch.json")?;

        context.log_verbose("✓ hitch.json loaded successfully");

        // Execute closure (for modification)
        context.log_verbose("Modifying metadata...");
        closure(&mut config)?;

        // Write updated config
        context.log_verbose("Writing updated hitch.json...");
        context.git().write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;

        // Commit changes
        context.log_verbose("Committing metadata changes...");
        context.git().add_and_commit(&["hitch.json"], "Update hitch configuration")?;

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
    })
}

/// Execute operations within a locked environment context
///
/// According to the specification:
/// - Calls lock(env_name) → executes closure → calls unlock(env_name) even if closure fails
/// - Ensures environment is safely locked during modifications
/// - Automatically handles warnings if push fails
pub fn with_locked_env<F, R>(
    context: &GlobalContext,
    env_name: &str,
    closure: F,
) -> Result<R>
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
        context.log_warning(&format!("Failed to unlock environment '{}': {}", env_name, e));
        context.log_warning("You may need to manually unlock the environment");
    } else {
        context.log_verbose(&format!("✓ Environment '{}' unlocked successfully", env_name));
    }

    result
}

/// Lock an environment
fn lock_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    access_metadata(context, |config: &mut HitchConfig| {
        let environment = config.get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        if environment.is_locked() {
            return Err(anyhow::anyhow!(
                "Environment '{}' is already locked by {}",
                env_name,
                environment.locked_by.as_ref().unwrap_or(&"unknown".to_string())
            ));
        }

        let user_email = context.git().get_user_email()?;
        environment.lock(user_email.clone());

        context.log_info(&format!("Environment '{}' locked by {}", env_name, user_email));
        Ok(())
    })
}

/// Unlock an environment
fn unlock_environment(context: &GlobalContext, env_name: &str) -> Result<()> {
    access_metadata(context, |config: &mut HitchConfig| {
        let environment = config.get_environment_mut(env_name)
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