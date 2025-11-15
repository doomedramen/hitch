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

pub fn run(
    args: StatusCommand,
    context: &GlobalContext,
) -> Result<()> {
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
    let locked_envs = config.environments.values().filter(|e| e.is_locked()).count();
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
    println!("📊 {} environments: {} total, {} locked, {} need rebuild, {} never rebuilt",
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
                    suggestions.push(format!("• Rebuild {}: 'hitch rebuild {}'", env_name.bright_green(), env_name));
                }
                RebuildStatus::NeverRebuilt => {
                    suggestions.push(format!("• Initial rebuild {}: 'hitch rebuild {}'", env_name.bright_green(), env_name));
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
fn display_environment_status(context: &GlobalContext, env_name: &str, env: &Environment) -> Result<()> {
    // Environment header with visual separator and more info
    let status_indicator = if env.is_locked() {
        "🔒".bright_yellow()
    } else {
        "🔓".bright_green()
    };

    println!("┌─ {} {} {}",
        env_name.bright_green().bold(),
        status_indicator,
        format!("base: {}", env.base.bright_blue())
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
        println!("│  {}", "⚠️ Environment is locked - no changes allowed".bright_red());
    } else {
        println!("│  {}", "🔓 Environment is unlocked".bright_green());
    }

    // Branches section with more details
    println!("├─ Branches ({} promoted):", env.branches.len());
    if env.branches.is_empty() {
        println!("│  {}", "• No branches promoted".dimmed());
        println!("│  {}", format!("  Use 'hitch promote <branch> {}' to add branches", env_name).dimmed());
    } else {
        for (i, branch) in env.branches.iter().enumerate() {
            // Check if branch exists locally or remotely
            let branch_status = if let Ok(exists) = context.git().branch_exists_anywhere(branch) {
                if exists {
                    "✅".bright_green().to_string()
                } else {
                    "❌".bright_red().to_string()
                }
            } else {
                "❓".bright_yellow().to_string()
            };

            let extra_info = if branch.contains(env_name) {
                format!("(matches env name)").dimmed()
            } else {
                "".to_string().normal()
            };

            println!("│  {} {} {}",
                branch_status,
                format!("{}. {}", i + 1, branch).bright_white(),
                extra_info
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
        },
        None => "• Never".bright_red().to_string(),
    };
    println!("│  {}", rebuild_info);

    // Status section with enhanced details
    println!("└─ Status:");
    let rebuild_status = determine_rebuild_status(context, env)?;
    match rebuild_status {
        RebuildStatus::UpToDate => {
            println!("   {}", "✅ Up to date".bright_green());
        }
        RebuildStatus::NeedsRebuild(reason) => {
            println!("   {} {}", "⚠️".bright_yellow(), reason.bright_yellow());
            println!("   {}", format!("💡 Run 'hitch rebuild {}' to update", env_name).dimmed());
        }
        RebuildStatus::NeverRebuilt => {
            println!("   {} {}", "⚠️".bright_red(), "Never rebuilt".bright_red());
            println!("   {}", format!("💡 Run 'hitch rebuild {}' to initialize", env_name).dimmed());
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
