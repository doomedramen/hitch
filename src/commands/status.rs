use crate::commands::global_context::GlobalContext;
use crate::core::state::{
    build_state_snapshot, ActualComposition, ApprovalPolicy, DeclaredBranch, DesiredComposition,
    EnvironmentHealth, EnvironmentState, RepositoryStateSnapshot,
};
use crate::types::{Environment, HitchConfig};
use crate::utils::prelude::access_metadata_read_only;
use crate::utils::setup;
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Args;
use colored::*;

#[derive(Args)]
pub struct StatusCommand {
    /// Print detailed step-by-step logs
    #[arg(long)]
    pub verbose: bool,

    /// Show changes compared to the last commit
    #[arg(long)]
    pub diff: bool,
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

    // One snapshot for the whole command. `config` above is still needed for
    // the protection/protection-adjacent sections and for the environment
    // list, but every staleness verdict below now comes from here, so the
    // per-branch glyphs and the per-environment verdict cannot disagree with
    // each other or with `hitch details`.
    let snapshot = build_state_snapshot(&context)?;
    context.log_verbose(&format!(
        "Built state snapshot: {} environment(s), {} feature(s)",
        snapshot.environments.len(),
        snapshot.features.len(),
    ));

    // Display status
    display_status(&context, &config, &snapshot)?;

    // Show diff if requested
    if args.diff {
        display_diff(&context)?;
    }

    context.log_verbose("Status command completed successfully");
    Ok(())
}

/// Display formatted status information
fn display_status(
    context: &GlobalContext,
    config: &HitchConfig,
    snapshot: &RepositoryStateSnapshot,
) -> Result<()> {
    // Display overall summary
    display_overall_summary(context, config, snapshot)?;

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
        display_environment_status(context, env_name, env, config, snapshot)?;
    }

    // Display summary at the end
    display_status_summary(snapshot)?;

    display_protection_status(context, config);

    Ok(())
}

/// Display overall project summary
fn display_overall_summary(
    context: &GlobalContext,
    config: &HitchConfig,
    snapshot: &RepositoryStateSnapshot,
) -> Result<()> {
    println!("{}", "🚀 Hitch Environment Status".bright_green().bold());
    println!("{}", "─".repeat(50).dimmed());

    // Count different states
    let total_envs = config.environments.len();
    let locked_envs = config
        .environments
        .values()
        .filter(|e| e.is_locked())
        .count();
    let needs_rebuild = snapshot
        .environments
        .iter()
        .filter(|e| e.health.is_actionable())
        .count();
    let never_rebuilt = snapshot
        .environments
        .iter()
        .filter(|e| matches!(e.health, EnvironmentHealth::NeverBuilt))
        .count();

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
fn display_status_summary(snapshot: &RepositoryStateSnapshot) -> Result<()> {
    println!("{}", "─".repeat(50).dimmed());

    // Quick action suggestions
    let mut suggestions = Vec::new();

    // Check for environments that need rebuilding. `LegacyUnknown` gets no
    // suggestion: there is nothing obviously wrong to fix, and nudging a
    // rebuild on every `hitch release`-published environment would be noise.
    for state in &snapshot.environments {
        let env_name = &state.name;
        match &state.health {
            EnvironmentHealth::NeedsRebuild { .. } | EnvironmentHealth::MissingBranch => {
                suggestions.push(format!(
                    "• Rebuild {}: 'hitch rebuild {}'",
                    env_name.bright_green(),
                    env_name
                ));
            }
            EnvironmentHealth::NeverBuilt => {
                suggestions.push(format!(
                    "• Initial rebuild {}: 'hitch rebuild {}'",
                    env_name.bright_green(),
                    env_name
                ));
            }
            EnvironmentHealth::Realised
            | EnvironmentHealth::PartiallyRealised { .. }
            | EnvironmentHealth::LegacyUnknown => {}
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
    config: &HitchConfig,
    snapshot: &RepositoryStateSnapshot,
) -> Result<()> {
    // The environment's own snapshot entry. Falling back to a synthesised
    // LegacyUnknown state (rather than unwrapping) keeps status renderable if
    // the two ever drift — a display command that panics on an internal
    // mismatch is worse than one that admits it has nothing to show.
    let state = snapshot
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .cloned()
        .unwrap_or_else(|| unknown_environment_state(env_name, env));
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
        // Pre-compute environment release status for performance (using already-loaded config)
        let mut released_envs = std::collections::HashSet::new();

        for (cfg_env_name, cfg_env) in &config.environments {
            if cfg_env.released_at.is_some() {
                released_envs.insert((cfg_env_name.clone(), cfg_env.base.clone()));
            }
        }

        // Two different questions, deliberately kept apart. The build record
        // says what the *last* build did, which is a fact; this local-only
        // preflight says what the *next* build would do, which is a prediction.
        // The old code conflated them, printing "held on rebuild" for a
        // prediction — which reads as a claim about the branch's current state.
        let held_last_build = match &state.actual {
            ActualComposition::FromRecord(actual) => Some(actual.held.clone()),
            _ => None,
        };
        let would_be_held = crate::utils::prelude::preflight_compatibility_report_local(
            context,
            &env.base,
            &env.branches,
        );

        for (i, branch) in env.branches.iter().enumerate() {
            // Existence comes from the snapshot, which already resolved every
            // declared branch against `refs/heads/*` and then the cached
            // remote-tracking ref. The old `branch_exists_anywhere` shelled out
            // to `git ls-remote --heads origin` once per branch, so this loop
            // cost a network round trip per promoted branch. The whole command
            // is offline now.
            let branch_exists = state
                .desired
                .branches
                .iter()
                .any(|b| &b.name == branch && b.sha.is_some());

            let is_in_source = branch_exists
                && context
                    .git()
                    .is_branch_merged_into(branch, &env.base)
                    .unwrap_or(false);

            // Staleness now comes from the snapshot's SHA comparison, not from
            // comparing a commit timestamp against a wall-clock `rebuilt_at`.
            // Those disagree for every rebased or cherry-picked branch, and
            // the old form also could not see a `--no-rebuild` promotion at
            // all, because nothing about that moves a timestamp.
            let is_stale = match &state.health {
                EnvironmentHealth::NeedsRebuild { changed_inputs, .. } => {
                    changed_inputs.iter().any(|c| &c.branch == branch)
                }
                _ => false,
            };

            // The record's verdict, when it has one, is the fact; the preflight
            // only fills the gap when the record is silent about this branch.
            let held_in_last_build = held_last_build
                .as_ref()
                .and_then(|held| held.iter().find(|c| &c.branch == branch));
            let held_next_build = would_be_held.iter().find(|c| &c.branch == branch);

            let branch_status = if !branch_exists {
                "❌".bright_red().to_string()
            } else if held_in_last_build.is_some() || held_next_build.is_some() {
                "⛔".red().to_string()
            } else if is_in_source {
                "⚠️ ".bright_yellow().to_string()
            } else if is_stale {
                "🔄".bright_yellow().to_string()
            } else {
                "✅".bright_green().to_string()
            };

            let source_warning = if is_in_source {
                format!(" (already in {})", env.base.bright_blue())
            } else {
                "".to_string()
            };

            let stale_warning = if is_stale && !is_in_source {
                " (new commits since last rebuild)"
                    .bright_yellow()
                    .to_string()
            } else {
                "".to_string()
            };

            // "was held" and "would be held" are different claims and get
            // different words. Only the prediction is actionable now.
            let held_warning = if let Some(c) = held_in_last_build {
                format!(
                    " (held in the last build — conflicts with {})",
                    c.conflicts_with
                )
                .red()
                .to_string()
            } else if let Some(c) = held_next_build {
                format!(
                    " (would be held on the next rebuild — conflicts with {})",
                    c.conflicts_with
                )
                .red()
                .to_string()
            } else {
                "".to_string()
            };

            println!(
                "│  {} {}{}{}{}",
                branch_status,
                format!("{}. {}", i + 1, branch).bright_white(),
                source_warning.normal(),
                stale_warning,
                held_warning
            );
        }
    }

    // The base's staleness is the same question as any other input's, and is
    // answered by the same comparison.
    let base_is_stale = match &state.health {
        EnvironmentHealth::NeedsRebuild { changed_inputs, .. } => {
            changed_inputs.iter().any(|c| c.branch == env.base)
        }
        _ => false,
    };

    // Rebuilt information with relative time
    println!("├─ Rebuilt:");
    let rebuild_info = match env.rebuilt_at {
        Some(timestamp) => {
            let formatted = format_timestamp(Some(timestamp));
            let relative = format_relative_time(timestamp);
            let stale_note = if base_is_stale {
                format!(
                    " {} base branch '{}' has new commits",
                    "⚠️".bright_yellow(),
                    env.base.bright_blue()
                )
            } else {
                "".to_string()
            };
            format!(
                "• {} ({}){}",
                formatted.bright_white(),
                relative.dimmed(),
                stale_note
            )
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
    match &state.health {
        EnvironmentHealth::Realised => {
            println!("   {}", "✅ Up to date".bright_green());
        }
        EnvironmentHealth::PartiallyRealised { held } => {
            println!(
                "   {} {}",
                "⚠️ ".bright_yellow(),
                format!(
                    "Current, but {} branch(es) held on the last build: {}",
                    held.len(),
                    held.join(", ")
                )
                .bright_yellow()
            );
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to retry them", env_name).dimmed()
            );
        }
        EnvironmentHealth::NeedsRebuild {
            changed_inputs,
            added,
            removed,
        } => {
            for change in changed_inputs {
                let (from, to) = change.short();
                println!(
                    "   {} {}",
                    "⚠️ ".bright_yellow(),
                    format!("{from} → {to}  {}", change.branch).bright_yellow()
                );
            }
            for branch in added {
                println!(
                    "   {} {}",
                    "⚠️ ".bright_yellow(),
                    format!("{branch}  promoted since the last build").bright_yellow()
                );
            }
            for branch in removed {
                println!(
                    "   {} {}",
                    "⚠️ ".bright_yellow(),
                    format!("{branch}  demoted since the last build").bright_yellow()
                );
            }
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to update", env_name).dimmed()
            );
        }
        EnvironmentHealth::NeverBuilt => {
            println!("   {} {}", "⚠️ ".bright_red(), "Never rebuilt".bright_red());
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to initialize", env_name).dimmed()
            );
        }
        // `hitch release` and both of `hitch resolve`'s publish paths land a
        // branch without writing a record, on purpose — hitch has no truthful
        // input for one. So this is a normal state, and saying "up to date"
        // here would be a claim about a build hitch cannot describe.
        EnvironmentHealth::LegacyUnknown => {
            println!(
                "   {} {}",
                "❓ ".bright_yellow(),
                "Actual unknown — no build record for this environment".bright_yellow()
            );
            println!(
                "   {}",
                "  (it was last built by a hitch that does not record builds, \
                 or published by 'hitch release'/'hitch resolve')"
                    .dimmed()
            );
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to make it known", env_name).dimmed()
            );
        }
        EnvironmentHealth::MissingBranch => {
            println!(
                "   {} {}",
                "❌ ".bright_red(),
                format!("Environment branch '{}' does not exist", env_name).bright_red()
            );
            println!(
                "   {}",
                format!("💡 Run 'hitch rebuild {}' to create it", env_name).dimmed()
            );
        }
    }

    // Add spacing between environments
    println!();
    Ok(())
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

    // Check all environments for actual merge status
    if !env.branches.is_empty() && context.git().branch_exists_anywhere(&env.base)? {
        for branch in &env.branches {
            if context.git().branch_exists_anywhere(branch)?
                && context.git().is_branch_merged_into(branch, &env.base)?
            {
                branches_in_source.push(branch.clone());
            }
        }
    }

    // Display branches that exist in source branch
    if !branches_in_source.is_empty() {
        println!(
            "│  {} {}",
            "⚠️  ".bright_yellow(),
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

/// Display changes compared to the last commit
fn display_diff(context: &GlobalContext) -> Result<()> {
    use crate::utils::diff::{diff_configs, format_diff};

    let git = context.git();

    // Get the last committed configuration
    let old_config_json = match git.read_file_from_branch("hitch-metadata", "hitch.json") {
        Ok(content) => content,
        Err(_) => {
            context.log_info("No previous configuration found to compare against.");
            return Ok(());
        }
    };

    let old_config: HitchConfig = match serde_json::from_str(&old_config_json) {
        Ok(config) => config,
        Err(e) => {
            context.log_warning(&format!("Failed to parse previous configuration: {}", e));
            return Ok(());
        }
    };

    // Get current configuration
    let current_config_json = std::fs::read_to_string("hitch.json")?;
    let current_config: HitchConfig = serde_json::from_str(&current_config_json)?;

    // Generate and display diff
    let changes = diff_configs(&old_config, &current_config);
    let diff_output = format_diff(&changes);

    println!("{}", diff_output);

    // Show summary
    let summary = crate::utils::diff::create_summary(&changes);
    if !summary.is_empty() && summary != "No changes" {
        context.log_info(&format!("Summary: {}", summary));
    }

    Ok(())
}

fn display_protection_status(context: &GlobalContext, config: &HitchConfig) {
    let (owner, repo) = match crate::utils::gh::owner_repo_from_remote() {
        Ok(pair) => pair,
        Err(_) => return,
    };

    let cache = match setup::load_protection_cache(&owner, &repo) {
        Some(c) => c,
        None => return,
    };

    let env_names: Vec<String> = config.environments.keys().cloned().collect();
    let base_names: Vec<String> = config
        .environments
        .values()
        .map(|env| env.base.clone())
        .collect();

    let unprotected = setup::get_unprotected_branches(&owner, &repo, &env_names, &base_names);

    if !unprotected.is_empty() {
        println!();
        context.log_warning(&format!("Unprotected branches: {}", unprotected.join(", ")));
        context.log_info("Run 'hitch setup' to add them to the protection ruleset.");
    }

    let _ = cache;
}

/// A stand-in state for an environment the snapshot does not contain.
///
/// Only reachable if the snapshot and `hitch.json` disagree, which they should
/// not — but a display command that panics on an internal mismatch is strictly
/// worse than one that says it has nothing to show. Every field is the honest
/// unknown, and `LegacyUnknown` is a state the CLI already knows how to render.
fn unknown_environment_state(env_name: &str, env: &Environment) -> EnvironmentState {
    EnvironmentState {
        name: env_name.to_string(),
        base: env.base.clone(),
        desired: DesiredComposition {
            base: env.base.clone(),
            base_sha: None,
            branches: env
                .branches
                .iter()
                .map(|name| DeclaredBranch {
                    name: name.clone(),
                    sha: None,
                })
                .collect(),
        },
        actual: ActualComposition::LegacyUnknown,
        health: EnvironmentHealth::LegacyUnknown,
        locked: env.is_locked(),
        approval_policy: ApprovalPolicy {
            required: env.requires_approval,
            min_approvals: env.min_approvals,
            approvers: env.approvers.clone(),
        },
        locked_by: env.locked_by.clone(),
        locked_at: env.locked_at,
        rebuilt_at: env.rebuilt_at,
        released_at: env.released_at,
    }
}
