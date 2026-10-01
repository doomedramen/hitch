use crate::commands::global_context::GlobalContext;
use crate::core::render::{render_equation, EnvironmentEquation};
use crate::types::HitchConfig;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use clap::Args;
use colored::*;
use std::collections::{HashMap, HashSet};

#[derive(Args)]
pub struct TreeCommand {
    /// Print detailed step-by-step logs
    #[arg(long)]
    pub verbose: bool,
}

pub fn run(args: TreeCommand, context: &GlobalContext) -> Result<()> {
    // Create a new context with the verbose flag
    let mut context = context.clone();
    context.verbose = context.verbose || args.verbose;

    context.log_verbose("Starting tree command...");

    // Use read-only metadata access
    let config = access_metadata_read_only(&context, |config: &HitchConfig| Ok(config.clone()))?;

    context.log_verbose("Successfully retrieved metadata");

    // Display the branch hierarchy tree
    display_tree(&context, &config)?;

    context.log_verbose("Tree command completed successfully");
    Ok(())
}

/// Display the branch hierarchy tree
fn display_tree(context: &GlobalContext, config: &HitchConfig) -> Result<()> {
    if config.environments.is_empty() {
        context.log_info("No environments configured.");
        context.log_info("Use 'hitch add <environment>' to create your first environment.");
        return Ok(());
    }

    println!("{}", "Branch Hierarchy".bright_green().bold());
    println!("{}", "─".repeat(50).dimmed());

    // Build a map of base branch -> environments that use it
    let mut base_to_envs: HashMap<String, Vec<String>> = HashMap::new();
    for (env_name, env) in &config.environments {
        base_to_envs
            .entry(env.base.clone())
            .or_default()
            .push(env_name.clone());
    }

    // Find root branches (branches that are not environments themselves)
    let env_names: HashSet<String> = config.environments.keys().cloned().collect();
    let mut root_branches: HashSet<String> = HashSet::new();

    for env in config.environments.values() {
        // If the base branch is not an environment, it's a root
        if !env_names.contains(&env.base) {
            root_branches.insert(env.base.clone());
        }
    }

    // If no root branches found, use all base branches as roots
    if root_branches.is_empty() {
        root_branches = base_to_envs.keys().cloned().collect();
    }

    // Sort root branches for consistent output
    let mut sorted_roots: Vec<String> = root_branches.into_iter().collect();
    sorted_roots.sort();

    // Display tree starting from each root
    for (idx, root) in sorted_roots.iter().enumerate() {
        let is_last_root = idx == sorted_roots.len() - 1;
        display_branch_tree(context, config, root, &base_to_envs, "", is_last_root, true)?;
    }

    println!();
    Ok(())
}

/// Recursively display the branch tree
fn display_branch_tree(
    context: &GlobalContext,
    config: &HitchConfig,
    branch: &str,
    base_to_envs: &HashMap<String, Vec<String>>,
    prefix: &str,
    _is_last: bool,
    is_root: bool,
) -> Result<()> {
    // Display the root branch
    if is_root {
        println!("* {}", branch.bright_blue());
    }

    // Get environments that use this branch as base
    if let Some(envs) = base_to_envs.get(branch) {
        let mut sorted_envs: Vec<&String> = envs.iter().collect();
        sorted_envs.sort();

        for (idx, env_name) in sorted_envs.iter().enumerate() {
            let is_last_env = idx == sorted_envs.len() - 1;
            let env = &config.environments[*env_name];

            // Calculate connector and prefix for this level
            let connector = if is_last_env { "└─ " } else { "├─ " };

            // Calculate prefix for children
            let child_prefix = if is_last_env {
                format!("{}  ", prefix)
            } else {
                format!("{}│ ", prefix)
            };

            // Environment node with additional info
            // Colourised only when there is something to colourise. `"".
            // yellow()` is not the empty string — it is the escape codes with
            // nothing between them, so every unlocked environment used to print
            // a stray `[33m[0m` at the end of its line. Pre-existing, and
            // invisible until the equation made the line worth reading.
            let lock_indicator = if env.is_locked() {
                " [LOCKED]".yellow().to_string()
            } else {
                String::new()
            };
            // The composition is the shared environment equation, not a
            // `(base: main, 3 promoted)` restatement of it. Spec §13 lists
            // `tree` as a place the equation has to appear, and a parenthetical
            // count is the one place the user most needs to see *which* branches
            // compose an environment — the promoted branches are the children of
            // this very node, and the equation names them at the node too, so
            // the two are read together rather than one standing in for the
            // other.
            //
            // A declaration equation has no excluded terms, so it is always a
            // single line and cannot break the tree's indentation. An
            // environment with no promoted branches still has a composition
            // (`dev = main`), which is the whole of it rather than a truncated
            // one — so there is no "base only" wording here, and the node's own
            // child list is the thing that reads as empty.
            let equation = render_equation(&EnvironmentEquation::from_config(env_name, env));
            println!(
                "{}{}[env] {}{lock_indicator}",
                prefix,
                connector,
                equation.bright_green(),
            );

            // Display promoted branches as children
            if !env.branches.is_empty() {
                // Local-only, no-fetch compatibility check (same one `hitch
                // status` uses) so a branch that would be held on the next
                // rebuild shows up here too, without slowing tree down with
                // a network fetch.
                let held = crate::utils::prelude::predict_composition(context, env, env_name)
                    .map(|p| p.held)
                    .unwrap_or_default();

                let mut sorted_branches: Vec<&String> = env.branches.iter().collect();
                sorted_branches.sort();

                for (branch_idx, promoted_branch) in sorted_branches.iter().enumerate() {
                    let is_last_branch = branch_idx == sorted_branches.len() - 1;
                    let branch_connector = if is_last_branch { "└─ " } else { "├─ " };

                    // Check if this promoted branch is also a base for another environment
                    let is_base_for_other = base_to_envs.contains_key(*promoted_branch);
                    let branch_icon = if is_base_for_other {
                        "[base] " // Base for another env
                    } else {
                        "" // Regular branch
                    };

                    let held_conflict = held.iter().find(|c| &c.branch == *promoted_branch);
                    let held_glyph = if held_conflict.is_some() { "⛔ " } else { "" };

                    let mut branch_display = if is_base_for_other {
                        format!(
                            "{}{}{} (also a base branch)",
                            held_glyph,
                            branch_icon,
                            promoted_branch.bright_cyan()
                        )
                    } else {
                        format!(
                            "{}{}{}",
                            held_glyph,
                            branch_icon,
                            promoted_branch.bright_white()
                        )
                    };
                    if let Some(c) = held_conflict {
                        branch_display.push_str(
                            &format!(" (conflicts with {} — held on rebuild)", c.conflicts_with)
                                .red()
                                .to_string(),
                        );
                    }

                    println!("{}{}{}", child_prefix, branch_connector, branch_display);
                }
            }

            // Recursively display environments that are based on this environment
            if base_to_envs.contains_key(env_name.as_str()) {
                display_branch_tree(
                    context,
                    config,
                    env_name,
                    base_to_envs,
                    &child_prefix,
                    is_last_env,
                    false,
                )?;
            }
        }
    }

    Ok(())
}
