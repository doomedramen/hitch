use crate::commands::global_context::GlobalContext;
use crate::core::render::{
    emit_json, render_environment_summaries, render_matrix, render_matrix_at,
};
use crate::core::state::{
    build_state_snapshot, ActualComposition, ApprovalPolicy, DeclaredBranch, DesiredComposition,
    EnvironmentHealth, EnvironmentState, RepositoryStateSnapshot,
};
use crate::core::status::{build_matrix_model, build_status_model};
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

    /// Show the per-environment, per-branch view that predates the matrix.
    ///
    /// The value is optional: `--environments` alone shows every environment,
    /// `--environments dev` shows one. clap has no optional-value form for an
    /// `Option<T>`, so the flag is declared as taking 0-or-1 arguments and
    /// `default_missing_value` turns the bare form into `Some("")` — which
    /// `run` reads back as "no name given" rather than as an environment
    /// literally called `""`.
    #[arg(long, value_name = "NAME", num_args = 0..=1, default_missing_value = "")]
    pub environments: Option<String>,
}

pub fn run(args: StatusCommand, context: &GlobalContext) -> Result<()> {
    // Create a new context with the verbose flag
    let mut context = context.clone();
    context.verbose = context.verbose || args.verbose;

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

    // The matrix and the per-environment view are two renderings of *one*
    // snapshot and *one* model, so the choice between them is made here and
    // both read from the same values. `--json` follows the same rule the four
    // mutating commands do: stdout is a document and nothing else, so the
    // prose is not printed at all rather than printed and then made
    // unparseable.
    if context.json {
        emit_status_json(&snapshot)?;
    } else if let Some(only) = args.environments.as_deref() {
        display_status(
            &context,
            &config,
            &snapshot,
            // The bare `--environments` form. See the field's doc comment.
            (!only.is_empty()).then_some(only),
        )?;
    } else {
        display_matrix(&context, &config, &snapshot)?;
    }

    // Show diff if requested
    if args.diff {
        display_diff(&context)?;
    }

    context.log_verbose("Status command completed successfully");
    Ok(())
}

/// The `--json` document for `hitch status`.
///
/// `{"schema_version", "status"}` rather than P6's `{plan, receipt}`, and the
/// reason is in the P7 plan: a two-key envelope exists because a *mutation*
/// has two halves — what was true before and what is true after. A read-only
/// view has one, and forcing it into two keys would mean either a `null`
/// receipt (which says "nothing happened" — true, and useless) or a second
/// envelope shape anyway.
///
/// It carries the matrix model and the environment models rather than the
/// whole snapshot, because the snapshot is an internal shape with
/// `BTreeMap`-ordering already imposed and no stability promise, whereas
/// `build_status_model`'s output is what `hitch status` actually displays.
#[derive(serde::Serialize)]
struct StatusDocument {
    captured_at: DateTime<Utc>,
    current_branch: Option<String>,
    matrix: crate::core::status::MatrixModel,
    environments: Vec<crate::core::status::EnvironmentStatusModel>,
}

fn emit_status_json(snapshot: &RepositoryStateSnapshot) -> Result<()> {
    // `MatrixModel`/`EnvironmentStatusModel` are projections of the snapshot
    // and are not `Serialize` yet; the document builds them here so the JSON
    // and the prose are rendered from the same two values.
    #[derive(serde::Serialize)]
    struct Envelope {
        schema_version: u32,
        status: StatusDocument,
    }
    let envelope = Envelope {
        schema_version: crate::core::render::JSON_SCHEMA_VERSION,
        status: StatusDocument {
            captured_at: snapshot.captured_at,
            current_branch: snapshot.current_branch.clone(),
            matrix: crate::core::status::build_matrix_model(snapshot),
            environments: build_status_model(snapshot).environments,
        },
    };
    emit_json(&envelope)
}

/// The default view: the matrix, the per-environment summary lines, and the
/// pending work — in that order, so the grid is the first thing a reader sees
/// and the suggestions sit directly beneath the thing they are about.
fn display_matrix(
    context: &GlobalContext,
    config: &HitchConfig,
    snapshot: &RepositoryStateSnapshot,
) -> Result<()> {
    display_headline(context)?;

    if config.environments.is_empty() {
        context.log_info("No environments configured.");
        context.log_info("Use 'hitch add <environment>' to create your first environment.");
        return Ok(());
    }

    let matrix = build_matrix_model(snapshot);
    match column_budget() {
        // No width to report — a pipe, a CI log, a redirect. That is not "narrow",
        // it is unbounded, and inventing a budget from a guess would make a
        // caller decide something about the reader's terminal that it cannot see.
        None => println!("{}", render_matrix(&matrix)),
        Some(budget) => println!("{}", render_matrix_at(&matrix, budget)),
    }
    println!();

    if !matrix.summaries.is_empty() {
        println!(
            "{}",
            render_environment_summaries(&matrix.summaries).trim_end()
        );
        println!();
    }

    // Suggested actions, moved up from the bottom of the screen. The
    // suggestions themselves are unchanged — the same `match` on
    // `EnvironmentHealth`, the same commands, the same deliberate silence for
    // `LegacyUnknown` — so this is a move and not a rewrite.
    display_suggested_actions(snapshot);

    println!("{}", "🔧 Quick commands:".bright_blue());
    println!("  • Explain a branch: 'hitch why <branch> [environment]'");
    println!("  • List branches: 'git branch -a'");
    println!("  • Promote branch: 'hitch promote <branch> <environment>'");
    println!("  • Rebuild env: 'hitch rebuild <environment>'");
    println!("  • Lock env: 'hitch lock <environment>'");
    println!();

    display_protection_status(context, config);

    Ok(())
}

/// The reader's terminal width, if there is one to report.
///
/// `COLUMNS` only, deliberately. There is no `terminal_size` dependency here and
/// adding one to learn a number the shell already exports is a real cost — in
/// the dependency tree, in build time, and in a second way for the value to be
/// wrong. So:
///
/// - unset, or set to something that is not a number: unbounded. Every
///   non-interactive caller lands here, and every one of them wants the full
///   grid.
/// - set: used as the budget, so [`render_matrix_at`] can decline to print a
///   table that would wrap into nonsense.
///
/// A width of zero is treated as *no* width rather than as "zero columns",
/// because `COLUMNS=0` is what some shells export for "unknown" and a budget of
/// zero would print the fallback for a reader with a perfectly wide terminal.
fn column_budget() -> Option<usize> {
    match std::env::var("COLUMNS") {
        Ok(raw) => raw.trim().parse::<usize>().ok().filter(|w| *w > 0),
        Err(_) => None,
    }
}

/// The per-environment, per-branch view that predates the matrix.
///
/// Unchanged in wording on purpose. It is a genuinely different view — it
/// carries the per-branch *prediction* ("would be held on the next rebuild"),
/// the approval policy and the cleanup notes, none of which belong in a grid —
/// so it survives as `--environments` rather than being reworded into the new
/// shape. What a reader wants to know about one environment is now
/// `hitch why <environment>`.
fn display_status(
    context: &GlobalContext,
    config: &HitchConfig,
    snapshot: &RepositoryStateSnapshot,
    only_environment: Option<&str>,
) -> Result<()> {
    // Display overall summary
    display_headline(context)?;

    if config.environments.is_empty() {
        context.log_info("No environments configured.");
        context.log_info("Use 'hitch add <environment>' to create your first environment.");
        return Ok(());
    }

    if let Some(name) = only_environment {
        // A typo'd environment name is worth an error rather than an empty
        // screen: `--environments devv` printing nothing at all reads as "dev is
        // fine", which is the opposite of the truth.
        if !config.environments.contains_key(name) {
            let mut known: Vec<&str> = config.environments.keys().map(|k| k.as_str()).collect();
            known.sort_unstable();
            anyhow::bail!(
                "No environment named '{}'. Configured environments: {}",
                name,
                if known.is_empty() {
                    "(none)".to_string()
                } else {
                    known.join(", ")
                }
            );
        }
        let env = &config.environments[name];
        display_environment_status(context, name, env, config, snapshot)?;
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

/// The headline: what command this is, and which branch the reader is on.
///
/// Deliberately *not* a rollup. The `📊 N environments: … total, locked, need
/// rebuild, never rebuilt` line that used to live here is gone, and each of its
/// four facts has somewhere better to live: the per-environment summary row
/// under the matrix carries the counts and the verdict, and the lock is
/// rendered on that same row. A second pass over the configuration producing a
/// second set of totals is exactly the drift P3 removed from this command, and
/// nothing about deleting it costs a fact.
fn display_headline(context: &GlobalContext) -> Result<()> {
    println!("{}", "🚀 Hitch Environment Status".bright_green().bold());
    println!("{}", "─".repeat(50).dimmed());
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

/// The environments with work outstanding, as commands to run.
///
/// Read from [`EnvironmentHealth`] and nowhere else, so it cannot disagree
/// with the matrix cell or the summary row above it — the same rule the rest of
/// this command follows. `LegacyUnknown` gets no suggestion on purpose: there
/// is nothing obviously wrong to fix, and nudging a rebuild on every
/// environment last published by `hitch release` would be noise.
fn display_suggested_actions(snapshot: &RepositoryStateSnapshot) {
    let mut suggestions = Vec::new();

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
}

/// Display status summary at the end
fn display_status_summary(snapshot: &RepositoryStateSnapshot) -> Result<()> {
    println!("{}", "─".repeat(50).dimmed());

    display_suggested_actions(snapshot);

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
