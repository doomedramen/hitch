use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CleanupCommand {
    /// Actually delete the candidate branches.
    /// Without this flag the command only shows what would be deleted (dry-run).
    #[arg(long)]
    pub force: bool,

    /// Limit candidates to branches not promoted to this specific environment.
    /// Without this option the command considers all environments.
    #[arg(long)]
    pub env: Option<String>,
}

pub fn run(args: CleanupCommand, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Read current hitch configuration
    let config = crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;

    // Collect all branch names that are currently promoted (in any / the target env)
    let mut promoted: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Also collect base branches so we never suggest deleting them
    let mut reserved: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (env_name, environment) in &config.environments {
        reserved.insert(environment.base.clone());
        let in_scope = args.env.as_deref().map(|e| e == env_name).unwrap_or(true);
        if in_scope {
            for b in &environment.branches {
                promoted.insert(b.clone());
            }
        }
    }

    // Built-in reserved names
    reserved.insert("hitch-metadata".to_string());

    // List all local branches
    let all_local = context.git().list_local_branches_with_prefix("")?;
    let current = context.git().get_current_branch().unwrap_or_default();

    // Candidates: local branches that are not currently promoted (in scope),
    // not a reserved/base branch, not the current branch, and not a hitch internal branch.
    let candidates: Vec<String> = all_local
        .into_iter()
        .filter(|b| {
            !promoted.contains(b)
                && !reserved.contains(b)
                && b != &current
                && !b.starts_with("hitch-tmp-")
                && b != "hitch-metadata"
        })
        .collect();

    if candidates.is_empty() {
        context.log_success("No branches to clean up.");
        return Ok(());
    }

    if args.force {
        context.log_info(&format!("Deleting {} branch(es)...", candidates.len()));
        let mut deleted = 0;
        let mut skipped = 0;
        for branch in &candidates {
            match context.git().delete_branch(branch, false) {
                Ok(()) => {
                    context.log_success(&format!("  Deleted '{}'", branch));
                    deleted += 1;
                }
                Err(e) => {
                    context.log_warning(&format!(
                        "  Skipped '{}' (not fully merged — use 'git branch -D {}' to force): {}",
                        branch, branch, e
                    ));
                    skipped += 1;
                }
            }
        }
        context.log_info(&format!(
            "Done: {} deleted, {} skipped (not fully merged).",
            deleted, skipped
        ));
    } else {
        context.log_info(&format!(
            "Found {} branch(es) not currently promoted{}. \
             Run 'hitch cleanup --force' to delete them:",
            candidates.len(),
            args.env
                .as_deref()
                .map(|e| format!(" to '{}'", e))
                .unwrap_or_default()
        ));
        for branch in &candidates {
            println!("  {}", branch);
        }
        context.log_info("(dry-run — no branches were deleted)");
    }

    Ok(())
}
