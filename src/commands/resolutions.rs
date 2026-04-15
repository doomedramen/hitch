use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ResolutionsCommand {
    #[command(subcommand)]
    pub command: ResolutionsSubcommand,
}

#[derive(Subcommand)]
pub enum ResolutionsSubcommand {
    /// Show shared rerere cache status (stored in hitch-metadata)
    Status,
    /// Prune shared rerere cache down to a size cap
    Prune(ResolutionsPruneCommand),
}

#[derive(Args)]
pub struct ResolutionsPruneCommand {
    /// Maximum size of shared cache in MB
    #[arg(long, default_value_t = 200)]
    pub max_size: u64,
}

pub fn run(args: ResolutionsCommand, context: &GlobalContext) -> Result<()> {
    match args.command {
        ResolutionsSubcommand::Status => run_status(context),
        ResolutionsSubcommand::Prune(p) => run_prune(context, p),
    }
}

fn run_status(context: &GlobalContext) -> Result<()> {
    let idx = crate::utils::rerere::load_shared_rerere_index(context)?;
    let Some(idx) = idx else {
        context.log_info("No shared conflict resolutions found (rerere cache is empty).");
        return Ok(());
    };

    let total_bytes: u64 = idx.entries.values().map(|e| e.size_bytes).sum();
    let count = idx.entries.len();

    println!("Shared conflict resolutions (rerere):");
    println!("  Entries: {}", count);
    println!("  Size   : {} bytes", total_bytes);
    println!();

    let candidates = crate::utils::rerere::compute_prune_candidates(context, &idx);
    let top: Vec<_> = candidates.into_iter().take(10).collect();
    if !top.is_empty() {
        println!("Top prune candidates:");
        for c in top {
            println!(
                "  {}  size:{}  contexts:{}{}",
                c.entry_id,
                c.size_bytes,
                c.contexts,
                if c.all_context_branches_missing {
                    "  (all branches missing)"
                } else {
                    ""
                }
            );
        }
    }

    Ok(())
}

fn run_prune(context: &GlobalContext, args: ResolutionsPruneCommand) -> Result<()> {
    crate::utils::rerere::prune_shared_rerere_cache(context, args.max_size)
}
