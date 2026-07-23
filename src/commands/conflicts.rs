use crate::commands::global_context::GlobalContext;
use crate::types::OnConflict;
use anyhow::Result;
use clap::Args;
use colored::*;

#[derive(Args)]
pub struct ConflictsCommand {
    /// The environment to check (e.g., dev, qa, staging)
    #[arg()]
    pub env_name: String,
}

/// Read-only standup board: which promoted branches currently conflict (and
/// with what), without rebuilding anything. Runs the same exhaustive
/// preflight `hitch rebuild --dry-run` uses, scoped to a single environment.
pub fn run(args: ConflictsCommand, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    let config = crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let environment = config
        .environments
        .get(&args.env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", args.env_name))?;

    if environment.branches.is_empty() {
        context.log_info(&format!(
            "Environment '{}' has no promoted branches — nothing to check.",
            args.env_name
        ));
        return Ok(());
    }

    context.log_info(&format!(
        "Checking compatibility of {} promoted branch{}...",
        environment.branches.len(),
        if environment.branches.len() == 1 {
            ""
        } else {
            "es"
        }
    ));

    let conflicts = crate::utils::prelude::preflight_compatibility_report(
        context,
        &environment.base,
        &environment.branches,
    )?;

    if conflicts.is_empty() {
        context.log_success(&format!(
            "'{}' has no conflicts — every promoted branch composes cleanly.",
            args.env_name
        ));
        return Ok(());
    }

    println!(
        "\n{} {} — {} branch{} would be held on rebuild (policy: {:?})\n",
        "⛔".red(),
        args.env_name.cyan().bold(),
        conflicts.len(),
        if conflicts.len() == 1 { "" } else { "es" },
        environment.on_conflict
    );

    for c in &conflicts {
        println!(
            "  {} {} {} {}",
            "•".yellow(),
            c.branch.bold(),
            "conflicts with".dimmed(),
            c.conflicts_with
        );
        for f in &c.conflicted_files {
            println!("      {}", f.dimmed());
        }
        println!(
            "      {}",
            format!(
                "git checkout {} && git rebase {}",
                c.branch, c.conflicts_with
            )
            .dimmed()
        );
    }
    println!();

    if environment.on_conflict == OnConflict::Eject {
        context.log_info(&format!(
            "These will be excluded (held) on the next 'hitch rebuild {}'; the rest still build.",
            args.env_name
        ));
    } else {
        context.log_warning(&format!(
            "on_conflict is 'halt' for '{}' — the next 'hitch rebuild {}' will refuse entirely \
             until these are fixed.",
            args.env_name, args.env_name
        ));
    }

    Ok(())
}
