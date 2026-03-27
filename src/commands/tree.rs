use crate::commands::global_context::GlobalContext;
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
    context.verbose = args.verbose;

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
        display_branch_tree(config, root, &base_to_envs, "", is_last_root, true)?;
    }

    println!();
    Ok(())
}

/// Recursively display the branch tree
fn display_branch_tree(
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
            let lock_indicator = if env.is_locked() { " [LOCKED]" } else { "" };
            let branch_count = env.branches.len();
            let branch_info = format!(
                "{} (base: {}, {} promoted{})",
                env_name.bright_green(),
                env.base.bright_blue(),
                branch_count.to_string().bright_cyan(),
                lock_indicator.yellow()
            );

            println!("{}{}[env] {}", prefix, connector, branch_info);

            // Display promoted branches as children
            if !env.branches.is_empty() {
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

                    let branch_display = if is_base_for_other {
                        format!(
                            "{}{} (also a base branch)",
                            branch_icon,
                            promoted_branch.bright_cyan()
                        )
                    } else {
                        format!("{}{}", branch_icon, promoted_branch.bright_white())
                    };

                    println!("{}{}{}", child_prefix, branch_connector, branch_display);
                }
            }

            // Recursively display environments that are based on this environment
            if base_to_envs.contains_key(env_name.as_str()) {
                display_branch_tree(
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
