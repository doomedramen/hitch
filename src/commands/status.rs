use crate::commands::global_context::GlobalContext;
use crate::types::{Environment, HitchConfig};
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Args;
use colored::*;

#[derive(Args)]
pub struct StatusCommand {
    /// Print detailed step-by-step logs
    #[arg(long)]
    pub verbose: bool,
}

pub fn run(args: StatusCommand, context: &GlobalContext) -> Result<()> {
    // Create a new context with the verbose flag
    let mut context = context.clone();
    context.verbose = args.verbose;

    context.log_verbose("Starting status command...");

    // Use read-only metadata access - works with unclean git states and doesn't create unnecessary commits
    context.log_verbose("Using read-only metadata access");
    let config = access_metadata_read_only(&context, |config: &HitchConfig| {
        Ok(config.clone()) // Return a copy for use in display
    })?;

    context.log_verbose("Successfully retrieved metadata using read-only access");

    // Display status
    display_status(&context, &config)?;

    context.log_verbose("Status command completed successfully");
    Ok(())
}

/// Display formatted status information
fn display_status(context: &GlobalContext, config: &HitchConfig) -> Result<()> {
    // Display overall summary
    display_overall_summary(context, config)?;

    if config.environments.is_empty() {
        context.log_info("No environments configured.");
        context.log_info("Use 'hitch add <environment>' to create your first environment.");
        return Ok(());
    }

    // Sort environments by name for consistent output
    let mut env_names: Vec<_> = config.environments.keys().collect();
    env_names.sort();

    for env_name in env_names {
        let env = &config.environments[env_name];
        display_environment_status(context, env_name, env)?;
    }

    // Display summary at the end
    display_status_summary(context, config)?;
    Ok(())
}

/// Display overall project summary
fn display_overall_summary(context: &GlobalContext, config: &HitchConfig) -> Result<()> {
    println!("{}", "🚀 Hitch Environment Status".bright_green().bold());
    println!("{}", "─".repeat(50).dimmed());

    // Count different states
    let total_envs = config.environments.len();
    let locked_envs = config
        .environments
        .values()
        .filter(|e| e.is_locked())
        .count();
    let mut needs_rebuild = 0;
    let mut never_rebuilt = 0;

    for env in config.environments.values() {
        if let Ok(status) = determine_rebuild_status(context, env) {
            match status {
                RebuildStatus::UpToDate => {}
                RebuildStatus::NeedsRebuild(_) => needs_rebuild += 1,
                RebuildStatus::NeverRebuilt => never_rebuilt += 1,
            }
        }
    }

    // Display summary line
    println!(
        "📊 {} environments: {} total, {} locked, {} need rebuild, {} never rebuilt",
        total_envs,
        total_envs.to_string().bright_cyan(),
        locked_envs.to_string().bright_yellow(),
        needs_rebuild.to_string().bright_yellow(),
        never_rebuilt.to_string().bright_red()
    );
    println!();

    // Show current git branch info
    if let Ok(current_branch) = context.git().get_current_branch() {
        if !current_branch.starts_with("detached-HEAD") {
            println!("📍 Current branch: {}", current_branch.bright_blue());
        } else {
            println!("📍 Current state: {}", "detached HEAD".bright_red());
        }
    }

    println!();
    Ok(())
}

/// Display status summary at the end
fn display_status_summary(context: &GlobalContext, config: &HitchConfig) -> Result<()> {
    println!("{}", "─".repeat(50).dimmed());

    // Quick action suggestions
    let mut suggestions = Vec::new();

    // Check for environments that need rebuilding
    for (env_name, env) in &config.environments {
        if let Ok(status) = determine_rebuild_status(context, env) {
            match status {
                RebuildStatus::NeedsRebuild(_) => {
                    suggestions.push(format!(
                        "• Rebuild {}: 'hitch rebuild {}'",
                        env_name.bright_green(),
                        env_name
                    ));
                }
                RebuildStatus::NeverRebuilt => {
                    suggestions.push(format!(
                        "• Initial rebuild {}: 'hitch rebuild {}'",
                        env_name.bright_green(),
                        env_name
                    ));
                }
                RebuildStatus::UpToDate => {}
            }
        }
    }

    if !suggestions.is_empty() {
        println!("{}", "💡 Suggested actions:".bright_yellow());
        for suggestion in suggestions {
            println!("  {}", suggestion);
        }
        println!();
    }

    // Quick help
    println!("{}", "🔧 Quick commands:".bright_blue());
    println!("  • List branches: 'git branch -a'");
    println!("  • Promote branch: 'hitch promote <branch> <environment>'");
    println!("  • Rebuild env: 'hitch rebuild <environment>'");
    println!("  • Lock env: 'hitch lock <environment>'");
    println!();

    Ok(())
}

/// Display status for a single environment
fn display_environment_status(
    context: &GlobalContext,
    env_name: &str,
    env: &Environment,
) -> Result<()> {
    // Environment header with visual separator and more info
    let status_indicator = if env.is_locked() {
        "🔒".bright_yellow()
    } else {
        "🔓".bright_green()
    };

    println!(
        "┌─ {} {} base: {}",
        env_name.bright_green().bold(),
        status_indicator,
        env.base.bright_blue()
    );

    // Lock status indicator with more details
    if env.is_locked() {
        let lock_info = format!(
            "🔒 Locked by {} at {}",
            env.locked_by.as_ref().unwrap_or(&"unknown".to_string()),
            format_timestamp(env.locked_at)
        );
        println!("│  {}", lock_info.yellow());

        // Add lock warning
        println!(
            "│  {}",
            "⚠️ Environment is locked - no changes allowed".bright_red()
        );
    } else {
        println!("│  {}", "🔓 Environment is unlocked".bright_green());
    }

    // Branches section with more details
    println!("├─ Branches ({} promoted):", env.branches.len());
    if env.branches.is_empty() {
        println!("│  {}", "• No branches promoted".dimmed());
        println!(
            "│  {}",
            format!(
                "  Use 'hitch promote <branch> {}' to add branches",
                env_name
            )
            .dimmed()
        );
    } else {
        for (i, branch) in env.branches.iter().enumerate() {
            // Check if branch exists locally or remotely and if it's already in source
            let branch_status = if let Ok(exists) = context.git().branch_exists_anywhere(branch) {
                if !exists {
                    "❌ ".bright_red().to_string()
                } else {
                    // Use the same heuristic as cleanup detection to handle squash merges
                    let is_in_source =
                        if let Ok(branch_sha) = context.git().get_branch_commit_sha(branch) {
                            if let Ok(target_sha) = context.git().get_branch_commit_sha(&env.base) {
                                is_branch_content_in_target(
                                    context,
                                    branch,
                                    &env.base,
                                    &branch_sha,
                                    &target_sha,
                                )
                                .unwrap_or(false)
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                    if is_in_source {
                        "⚠️ ".bright_yellow().to_string()
                    } else {
                        "✅ ".bright_green().to_string()
                    }
                }
            } else {
                "❓ ".bright_yellow().to_string()
            };

            let extra_info = if branch.contains(env_name) {
                "(matches env name)".to_string().dimmed()
            } else {
                "".to_string().normal()
            };

            // Add warning for branches already in source (using same heuristic as status)
            let source_warning = if let Ok(branch_sha) = context.git().get_branch_commit_sha(branch)
            {
                if let Ok(target_sha) = context.git().get_branch_commit_sha(&env.base) {
                    if is_branch_content_in_target(
                        context,
                        branch,
                        &env.base,
                        &branch_sha,
                        &target_sha,
                    )
                    .unwrap_or(false)
                    {
                        format!(" (already in {})", env.base.bright_blue())
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            };

            println!(
                "│  {} {} {}{}",
                branch_status,
                format!("{}. {}", i + 1, branch).bright_white(),
                extra_info,
                source_warning.normal()
            );
        }
    }

    // Rebuilt information with relative time
    println!("├─ Rebuilt:");
    let rebuild_info = match env.rebuilt_at {
        Some(timestamp) => {
            let formatted = format_timestamp(Some(timestamp));
            let relative = format_relative_time(timestamp);
            format!("• {} ({})", formatted.bright_white(), relative.dimmed())
        }
        None => "• Never".bright_red().to_string(),
    };
    println!("│  {}", rebuild_info);

    // Release information with relative time
    println!("├─ Released:");
    let release_info = match env.released_at {
        Some(timestamp) => {
            let formatted = format_timestamp(Some(timestamp));
            let relative = format_relative_time(timestamp);
            format!("• {} ({})", formatted.bright_white(), relative.dimmed())
        }
        None => "• Never".bright_yellow().to_string(),
    };
    println!("│  {}", release_info);

    // Check for branches that need cleanup after release
    check_and_display_cleanup_needs(context, env_name, env)?;

    // Status section with enhanced details
    println!("└─ Status:");
    let rebuild_status = determine_rebuild_status(context, env)?;
    match rebuild_status {
        RebuildStatus::UpToDate => {
            println!("   {}", "✅ Up to date".bright_green());
        }
        RebuildStatus::NeedsRebuild(reason) => {
            println!("   {} {}", "⚠️".bright_yellow(), reason.bright_yellow());
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to update", env_name).dimmed()
            );
        }
        RebuildStatus::NeverRebuilt => {
            println!("   {} {}", "⚠️".bright_red(), "Never rebuilt".bright_red());
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to initialize", env_name).dimmed()
            );
        }
    }

    // Add spacing between environments
    println!();
    Ok(())
}

/// Rebuild status for an environment
enum RebuildStatus {
    UpToDate,
    NeedsRebuild(String),
    NeverRebuilt,
}

/// Determine if an environment needs rebuilding
fn determine_rebuild_status(context: &GlobalContext, env: &Environment) -> Result<RebuildStatus> {
    // If never rebuilt, it needs rebuilding
    if env.rebuilt_at.is_none() {
        return Ok(RebuildStatus::NeverRebuilt);
    }

    let rebuilt_at = env.rebuilt_at.unwrap();
    let mut newer_branches = Vec::new();

    // Check base branch
    if let Ok(base_exists) = context.git().branch_exists_anywhere(&env.base) {
        if base_exists {
            if let Ok(base_sha) = context.git().get_branch_commit_sha(&env.base) {
                if let Ok(base_timestamp) = context.git().get_commit_timestamp(&base_sha) {
                    if base_timestamp > rebuilt_at {
                        newer_branches.push(env.base.clone());
                    }
                }
            }
        }
    }

    // Check promoted branches
    for branch in &env.branches {
        if let Ok(branch_exists) = context.git().branch_exists_anywhere(branch) {
            if branch_exists {
                if let Ok(branch_sha) = context.git().get_branch_commit_sha(branch) {
                    if let Ok(branch_timestamp) = context.git().get_commit_timestamp(&branch_sha) {
                        if branch_timestamp > rebuilt_at {
                            newer_branches.push(branch.clone());
                        }
                    }
                }
            }
        }
    }

    if newer_branches.is_empty() {
        Ok(RebuildStatus::UpToDate)
    } else {
        let reason = format!(
            "Rebuild needed ({} has newer commits)",
            newer_branches.join(", ")
        );
        Ok(RebuildStatus::NeedsRebuild(reason))
    }
}

/// Format a timestamp for display
fn format_timestamp(timestamp: Option<DateTime<Utc>>) -> String {
    match timestamp {
        Some(dt) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
        None => "Never".to_string(),
    }
}

/// Format a relative time (e.g., "2 hours ago", "3 days ago")
fn format_relative_time(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(timestamp);

    if duration.num_days() > 0 {
        let days = duration.num_days();
        return if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        };
    }

    if duration.num_hours() > 0 {
        let hours = duration.num_hours();
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        };
    }

    if duration.num_minutes() > 0 {
        let minutes = duration.num_minutes();
        return if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", minutes)
        };
    }

    "Just now".to_string()
}

/// Check and display cleanup needs for promoted branches that have been released
fn check_and_display_cleanup_needs(
    context: &GlobalContext,
    env_name: &str,
    env: &Environment,
) -> Result<()> {
    let mut branches_in_source = Vec::new();

    // Always check if environment has promoted branches (regardless of release status)
    if !env.branches.is_empty() {
        // Check if the base branch exists for comparison
        if context.git().branch_exists_anywhere(&env.base)? {
            for branch in &env.branches {
                // Check if the branch's commits exist in the base branch (indicating it was released)
                // Use a different approach that works with squash merges
                if context.git().branch_exists_anywhere(branch)? {
                    // Get the list of commits that exist in both branches
                    match context.git().get_branch_commit_sha(branch) {
                        Ok(branch_sha) => {
                            // Check if this specific commit or equivalent changes exist in target
                            // For squash merges, we need to check if the branch changes are in the target
                            if let Ok(target_sha) = context.git().get_branch_commit_sha(&env.base) {
                                // Check if the diff between the branch and its merge base is empty in target
                                // This indicates the branch changes were incorporated (even via squash)
                                if is_branch_content_in_target(
                                    context,
                                    branch,
                                    &env.base,
                                    &branch_sha,
                                    &target_sha,
                                )? {
                                    branches_in_source.push(branch.clone());
                                }
                            }
                        }
                        Err(_) => {
                            // Branch doesn't exist, skip it
                        }
                    }
                }
            }
        }
    }

    // Display branches that exist in source branch
    if !branches_in_source.is_empty() {
        println!(
            "│  {} {}",
            "⚠️ ".bright_yellow(),
            "Branches already in source:".bright_yellow()
        );

        // Split the message to avoid long lines
        println!(
            "│    {} {} {}",
            "The following branches exist in".bright_white(),
            env.base.bright_blue(),
            "and can be demoted:".bright_white()
        );

        // List branches on separate lines if there are multiple
        if branches_in_source.len() > 1 {
            for branch in &branches_in_source {
                println!("│      • {}", branch.bright_cyan());
            }
        } else {
            println!("│      • {}", branches_in_source[0].bright_cyan());
        }

        // Build demote commands for user convenience
        let demote_commands: Vec<String> = branches_in_source
            .iter()
            .map(|b| format!("hitch demote {} {}", b, env_name))
            .collect();

        if demote_commands.len() == 1 {
            println!("│    {}", format!("Run: {}", demote_commands[0]).dimmed());
        } else {
            println!("│    {}", "Run:".dimmed());
            for cmd in demote_commands {
                println!("│      {}", cmd.dimmed());
            }
        }
    }

    Ok(())
}

/// Check if the content of a branch has been incorporated into target branch (handles squash merges)
fn is_branch_content_in_target(
    context: &GlobalContext,
    source_branch: &str,
    target_branch: &str,
    _source_sha: &str,
    _target_sha: &str,
) -> Result<bool> {
    // For environments that have been released, check if the branch content is in target
    // This is a heuristic that works for squash merges

    // Get the config to check release timestamps
    let config = crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;

    // Find the environment that contains this branch
    for env in config.environments.values() {
        if env.base == target_branch && env.branches.contains(&source_branch.to_string()) {
            // Check if this environment has been released
            if let Some(_released_at) = env.released_at {
                // If the environment was released, assume the content was incorporated
                // This is a reasonable heuristic for our use case
                return Ok(true);
            }
        }
    }

    // Fallback to the original check for regular merges
    context
        .git()
        .is_branch_merged_into(source_branch, target_branch)
}
