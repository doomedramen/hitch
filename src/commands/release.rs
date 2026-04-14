use crate::commands::global_context::GlobalContext;
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

    /// Force release even if environment is locked
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: ReleaseCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Releasing environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name, args.force)?;

    // Step 2: Resolve target branch
    let target_branch = resolve_target_branch(context, &args.env_name, args.target_branch)?;

    // Step 3: User confirmation (skip with --force)
    if !args.force {
        confirm_release(context, &args.env_name, &target_branch)?;
    }

    // Step 4-7: Execute release with automatic locking and unlocking
    if args.force {
        context.log_info(&format!(
            "Force releasing locked environment '{}' to '{}'...",
            args.env_name, target_branch
        ));
        perform_release_forced(context, &args.env_name, &target_branch)?;
    } else {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            perform_release(context, &args.env_name, &target_branch)
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

/// Perform the release with normal locking
fn perform_release(context: &GlobalContext, env_name: &str, target_branch: &str) -> Result<()> {
    perform_release_core(context, env_name, target_branch)
}

/// Perform the release with forced mode (environment already locked)
fn perform_release_forced(
    context: &GlobalContext,
    env_name: &str,
    target_branch: &str,
) -> Result<()> {
    perform_release_core(context, env_name, target_branch)
}

/// Core release logic shared by normal and forced modes
fn perform_release_core(
    context: &GlobalContext,
    env_name: &str,
    target_branch: &str,
) -> Result<()> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    if environment.branches.is_empty() {
        context.log_info(&format!(
            "No branches promoted to environment '{}', nothing to release",
            env_name
        ));
        return Ok(());
    }

    context.log_info(&format!(
        "Releasing {} promoted branches from environment '{}' to '{}'",
        environment.branches.len(),
        env_name,
        target_branch
    ));

    // Record original branch
    let original_branch = context.git().get_current_branch()?;
    context.log_verbose(&format!("Current branch: '{}'", original_branch));

    // Synchronize all branches
    context.log_info("Synchronizing branches for release...");
    let mut all_branches = environment.branches.clone();
    all_branches.push(target_branch.to_string());
    context.git().synchronize_branches(&all_branches)?;

    // Switch to target branch
    context.log_info(&format!(
        "Switching to target branch '{}'...",
        target_branch
    ));
    context.git().checkout_branch(target_branch)?;

    // Perform merges with conflict checking
    for branch in &environment.branches {
        context.log_info(&format!("Merging '{}' into '{}'...", branch, target_branch));

        let (has_conflicts, conflicted_files) =
            context.git().check_merge_conflicts_detailed(branch)?;

        if has_conflicts {
            return Err(build_conflict_error(
                branch,
                conflicted_files,
                target_branch,
                env_name,
            ));
        }

        let merge_message = format!(
            "Hitch: release {} from {} to {}",
            branch, env_name, target_branch
        );
        context.git().squash_merge(branch, &merge_message)?;
        context.log_verbose(&format!(
            "✓ Squash merged '{}' into '{}'",
            branch, target_branch
        ));
    }

    // Commit the merged changes
    let commit_message = format!("Hitch: release {} to {}", env_name, target_branch);
    context.git().commit(&commit_message)?;
    context.log_info(&format!("✓ Committed release to '{}'", target_branch));

    // Create auto-tag for release tracking with descriptive name and ISO 8601 timestamp
    let now = chrono::Utc::now();
    let datetime_str = now.format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let target_branch_clean = target_branch.replace('/', "-");
    let tag_name = format!(
        "hitch-release-{}-to-{}-{}",
        env_name, target_branch_clean, datetime_str
    );
    let tag_message = format!(
        "Hitch release of environment '{}' to '{}' at {}",
        env_name,
        target_branch,
        now.format("%Y-%m-%d %H:%M:%S UTC")
    );

    context.git().create_tag(&tag_name, &tag_message)?;
    context.log_info(&format!("✓ Created release tag '{}'", tag_name));

    // Push changes and tag if enabled
    if context.should_push() {
        context.log_info("Pushing release to remote...");
        context.git().push_branch(target_branch)?;
        context.git().push_tag(&tag_name)?;
        context.log_verbose("✓ Pushed release and tag to remote");
    }

    // Return to original branch
    context.git().checkout_branch(&original_branch)?;
    context.log_verbose(&format!(
        "✓ Returned to original branch '{}'",
        original_branch
    ));

    // Update release metadata
    update_release_timestamp(context, env_name)?;

    Ok(())
}

/// Build detailed conflict error message
fn build_conflict_error(
    branch: &str,
    conflicted_files: Option<Vec<String>>,
    target_branch: &str,
    env_name: &str,
) -> anyhow::Error {
    let mut error_msg = format!(
        "Merge conflict detected when releasing branch '{}' to '{}'",
        branch, target_branch
    );

    if let Some(files) = conflicted_files {
        if !files.is_empty() {
            error_msg.push_str("\n\nConflicting files:");
            for file in files {
                error_msg.push_str(&format!("\n  • {}", file));
            }
        }
    }

    error_msg.push_str("\n\nTo resolve this:");
    error_msg.push_str(&format!(
        "\n1. Check out target branch: git checkout {}",
        target_branch
    ));
    error_msg.push_str(&format!(
        "\n2. Manually merge '{}': git merge {}",
        branch, branch
    ));
    error_msg.push_str("\n3. Resolve conflicts and commit");
    error_msg.push_str("\n4. Try release again with the environment instead:");
    error_msg.push_str(&format!(
        "\n   hitch release {} {}",
        env_name, target_branch
    ));

    anyhow::anyhow!("{}", error_msg)
}

/// Update the release timestamp in environment metadata
fn update_release_timestamp(context: &GlobalContext, env_name: &str) -> Result<()> {
    context.log_verbose("Updating release timestamp...");

    crate::utils::prelude::modify_metadata(context, |config| {
        let environment = config
            .get_environment_mut(env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

        environment.update_released_timestamp();
        context.log_verbose(&format!("✓ Updated release timestamp for '{}'", env_name));

        Ok(())
    })?;

    Ok(())
}

/// Confirm release operation with user
fn confirm_release(context: &GlobalContext, env_name: &str, target_branch: &str) -> Result<()> {
    use std::io::{self, Write};

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
    print!("\nDo you want to continue? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim().to_lowercase();
    if input != "y" && input != "yes" {
        context.log_info("Release cancelled by user.");
        std::process::exit(0);
    }

    context.log_info("User confirmed release - proceeding...");
    Ok(())
}
