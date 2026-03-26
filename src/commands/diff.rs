use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;
use colored::*;

#[derive(Args)]
pub struct DiffCommand {
    /// The environment to preview (e.g., dev, qa, staging)
    #[arg()]
    pub env_name: String,

    /// Show only the diff for this specific promoted branch
    #[arg(long)]
    pub branch: Option<String>,

    /// Show file-level change statistics instead of commit summaries
    #[arg(long)]
    pub stat: bool,
}

pub fn run(args: DiffCommand, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    let config =
        crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;

    let environment = config.environments.get(&args.env_name).ok_or_else(|| {
        anyhow::anyhow!("Environment '{}' does not exist", args.env_name)
    })?;

    // Validate --branch argument before the empty-env check so we can give
    // a specific error even when no branches are promoted.
    if let Some(ref b) = args.branch {
        if !environment.branches.contains(b) {
            return Err(anyhow::anyhow!(
                "Branch '{}' is not promoted to environment '{}'",
                b,
                args.env_name
            ));
        }
    }

    if environment.branches.is_empty() {
        context.log_info(&format!(
            "Environment '{}' has no promoted branches — nothing to diff.",
            args.env_name
        ));
        return Ok(());
    }

    // Filter to a single branch if --branch was given
    let branches_to_show: Vec<&str> = if let Some(ref b) = args.branch {
        vec![b.as_str()]
    } else {
        environment.branches.iter().map(|b| b.as_str()).collect()
    };

    println!(
        "\n{} {} vs base branch '{}'\n",
        "Preview:".bold(),
        args.env_name.cyan().bold(),
        environment.base.yellow()
    );

    for branch in &branches_to_show {
        println!("{} {}", "Branch:".bold(), branch.cyan());

        if args.stat {
            match context.git().get_diff_stat(&environment.base, branch) {
                Ok(stat) if !stat.trim().is_empty() => {
                    for line in stat.lines() {
                        println!("  {}", line);
                    }
                }
                Ok(_) => println!("  (no changes relative to base)"),
                Err(e) => context.log_warning(&format!("  Could not compute diff stat: {}", e)),
            }
        } else {
            match context.git().get_commits_between(&environment.base, branch) {
                Ok(commits) if !commits.is_empty() => {
                    for commit in &commits {
                        println!("  {} {}", "+".green(), commit);
                    }
                    println!("  {} commit(s)", commits.len());
                }
                Ok(_) => println!("  {} (no commits ahead of base)", "(up-to-date)".dimmed()),
                Err(e) => context.log_warning(&format!("  Could not list commits: {}", e)),
            }
        }
        println!();
    }

    Ok(())
}
