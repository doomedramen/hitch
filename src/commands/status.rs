use crate::commands::global_context::GlobalContext;
use crate::types::{Environment, HitchConfig};
use anyhow::{Context, Result};
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
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a new context with the verbose flag
    let mut context = context.clone();
    context.verbose = args.verbose;

    context.log_verbose("Starting status command...");

    // For status command, always use git show approach to avoid modifying metadata
    // This ensures status works even with unclean working directories and doesn't create unnecessary commits
    context.log_verbose("Using git show approach for read-only status access");
    let (config, access_method) = get_metadata_via_git_show(&context)?;

    context.log_verbose(&format!("Successfully retrieved metadata using: {}", access_method));

    // Display status
    display_status(&context, &config)?;

    context.log_verbose("Status command completed successfully");
    Ok(())
}


/// Get metadata using git show (works with unclean working tree)
fn get_metadata_via_git_show(context: &GlobalContext) -> Result<(HitchConfig, String)> {
    context.log_verbose("Attempting to read hitch.json from hitch-metadata branch...");

    let config_json = context
        .git()
        .read_file_from_branch("hitch-metadata", "hitch.json")
        .context("Failed to read hitch.json from hitch-metadata branch")?;

    let config: HitchConfig = serde_json::from_str(&config_json)
        .context("Failed to parse hitch.json from hitch-metadata branch")?;

    Ok((config, "git_show".to_string()))
}

/// Display formatted status information
fn display_status(context: &GlobalContext, config: &HitchConfig) -> Result<()> {
    if config.environments.is_empty() {
        context.log_info("No environments configured.");
        return Ok(());
    }

    context.log_info(&format!("Environments ({} total):", config.environments.len()));
    println!();

    // Sort environments by name for consistent output
    let mut env_names: Vec<_> = config.environments.keys().collect();
    env_names.sort();

    for env_name in env_names {
        let env = &config.environments[env_name];
        display_environment_status(context, env_name, env)?;
    }

    Ok(())
}

/// Display status for a single environment
fn display_environment_status(context: &GlobalContext, env_name: &str, env: &Environment) -> Result<()> {
    // Environment header with visual separator
    println!("┌─ {} {}", env_name.bright_green().bold(), format!("({})", env.base.bright_blue()));

    // Lock status indicator
    if env.is_locked() {
        let lock_info = format!(
            "🔒 Locked by {} at {}",
            env.locked_by.as_ref().unwrap_or(&"unknown".to_string()),
            format_timestamp(env.locked_at)
        );
        println!("│  {}", lock_info.yellow());
    }

    // Branches section
    println!("├─ Branches:");
    if env.branches.is_empty() {
        println!("│  {}", "• No branches promoted".dimmed());
    } else {
        for branch in &env.branches {
            println!("│  {}", format!("• {}", branch).bright_white());
        }
    }

    // Rebuilt information
    println!("├─ Rebuilt:");
    let rebuild_info = match env.rebuilt_at {
        Some(timestamp) => format!("• {}", format_timestamp(Some(timestamp))),
        None => "• Never".bright_red().to_string(),
    };
    println!("│  {}", rebuild_info);

    // Status section
    println!("└─ Status:");
    let rebuild_status = determine_rebuild_status(context, env)?;
    match rebuild_status {
        RebuildStatus::UpToDate => {
            println!("   {}", "✅ Up to date".bright_green());
        }
        RebuildStatus::NeedsRebuild(reason) => {
            println!("   {} {}", "⚠️".bright_yellow(), reason.bright_yellow());
        }
        RebuildStatus::NeverRebuilt => {
            println!("   {} {}", "⚠️".bright_red(), "Never rebuilt".bright_red());
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
