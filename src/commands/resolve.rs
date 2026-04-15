use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ResolveCommand {
    /// Continue the rebuild after resolving all conflict markers
    #[arg(long = "continue", conflicts_with = "abort")]
    pub continue_flag: bool,

    /// Abort the in-progress rebuild and restore your original branch
    #[arg(long)]
    pub abort: bool,
}

pub fn run(args: ResolveCommand, context: &GlobalContext) -> Result<()> {
    let git_dir = context.git().get_git_dir();

    // Read resolve state — all three modes require it
    let state = crate::utils::resolve_state::read_resolve_state(&git_dir)?;

    if args.abort {
        run_abort(context, &state)
    } else if args.continue_flag {
        run_continue(context, &state)
    } else {
        run_status(context, &state)
    }
}

/// Default: show conflict status and instructions
fn run_status(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<()> {
    context.log_info(&format!(
        "Rebuild of '{}' is paused due to a merge conflict.",
        state.env_name
    ));
    println!();
    println!("  Environment   : {}", state.env_name);
    println!("  Temp branch   : {}", state.temp_branch);
    println!("  Conflict with : {}", state.conflict_branch);
    println!("  Base branch   : {}", state.base_branch);

    if !state.merged_so_far.is_empty() {
        println!("  Already merged: {}", state.merged_so_far.join(", "));
    }

    let still_pending: Vec<&String> = state
        .remaining_branches
        .iter()
        .skip(1) // first is the conflict branch
        .collect();
    if !still_pending.is_empty() {
        println!(
            "  Still pending : {}",
            still_pending
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    println!();

    // Show conflicted files from git status
    let output = context
        .git()
        .run_git_command(&["diff", "--name-only", "--diff-filter=U"])?;
    if output.status.success() {
        let files = String::from_utf8_lossy(&output.stdout);
        let files = files.trim();
        if !files.is_empty() {
            println!("Conflicted files:");
            for f in files.lines() {
                println!("  {}", f);
            }
            println!();
        }
    }

    println!("To resolve:");
    println!("  1. Edit the conflicted files and remove all conflict markers (<<<<<<<, =======, >>>>>>>)");
    println!("  2. Stage your resolved files: git add <file>");
    println!("  3. Run: hitch resolve --continue");
    println!();
    println!("To abandon and start over:");
    println!("  hitch resolve --abort");

    Ok(())
}

/// --continue: commit resolution and finish the rebuild
fn run_continue(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<()> {
    context.log_info(&format!(
        "Continuing rebuild of '{}' after conflict resolution...",
        state.env_name
    ));

    crate::utils::prelude::continue_rebuild_after_resolve(context, state)?;

    if state.reuse_resolutions {
        if let Ok(summary) = crate::utils::rerere::export_rerere_cache_to_metadata(context, state) {
            context.log_verbose(&format!(
                "Exported {} rerere entr(y/ies) ({} files) to hitch-metadata{}.",
                summary.exported_entries,
                summary.exported_files,
                if summary.committed { "" } else { " (noop)" }
            ));
        }
    }
    if state.rerere_restore {
        restore_rerere_config(context, state.rerere_original.clone())?;
    }

    context.log_success(&format!(
        "Environment '{}' rebuilt successfully!",
        state.env_name
    ));
    Ok(())
}

/// --abort: clean up and restore original branch
fn run_abort(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<()> {
    context.log_info(&format!("Aborting rebuild of '{}'...", state.env_name));

    crate::utils::prelude::abort_rebuild_resolve(context, state)?;

    if state.rerere_restore {
        restore_rerere_config(context, state.rerere_original.clone())?;
    }

    context.log_success(&format!(
        "Rebuild of '{}' aborted. Returned to branch '{}'.",
        state.env_name, state.original_branch
    ));
    Ok(())
}

fn restore_rerere_config(context: &GlobalContext, original: Option<String>) -> Result<()> {
    match original {
        Some(v) => context.git().set_git_config("rerere.enabled", &v),
        None => context.git().unset_git_config("rerere.enabled"),
    }
}
