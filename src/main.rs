use clap::{Parser, Subcommand};

mod commands;
mod types;
mod utils;

use commands::global_context::GlobalContext;

#[derive(Parser)]
#[command(name = "hitch")]
#[command(about = "A CLI tool for managing environment-specific git branches and metadata")]
#[command(version = "1.0.0")]
#[command(author = "Martin Page")]
struct Cli {
    /// Print detailed step-by-step logs for commands
    #[arg(long, global = true)]
    verbose: bool,

    /// Skip automatic pushes when metadata is committed
    #[arg(long, global = true)]
    no_push: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize hitch metadata in the current git repository
    Init(commands::init::InitCommand),
    /// Add a new environment to hitch configuration
    Add(commands::add::AddCommand),
    /// Remove an environment from hitch configuration
    Remove(commands::remove::RemoveCommand),
    /// Promote a branch to an environment for deployment
    Promote(commands::promote::PromoteCommand),
    /// Demote a branch from an environment
    Demote(commands::demote::DemoteCommand),
    /// Rebuild an environment by merging its branches
    Rebuild(commands::rebuild::RebuildCommand),
    /// Show the status of Hitch environments and branches
    Status(commands::status::StatusCommand),
    /// Lock an environment to prevent changes
    Lock(commands::lock::LockCommand),
    /// Unlock an environment to allow changes
    Unlock(commands::unlock::UnlockCommand),
    /// Guard against commits to environment branches (for pre-commit hooks)
    Guard(commands::guard::GuardCommand),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize colored output
    colored::control::set_override(true);

    let cli = Cli::parse();

    // Create global context with flags
    let context = GlobalContext::new(cli.verbose, cli.no_push)?;

    // Execute the appropriate command
    match cli.command {
        Commands::Init(args) => commands::init::run(args, &context).map_err(|e| e.into()),
        Commands::Add(args) => commands::add::run(args, &context).map_err(|e| e.into()),
        Commands::Remove(args) => commands::remove::run(args, &context).map_err(|e| e.into()),
        Commands::Promote(args) => commands::promote::run(args, &context).map_err(|e| e.into()),
        Commands::Demote(args) => commands::demote::run(args, &context).map_err(|e| e.into()),
        Commands::Rebuild(args) => commands::rebuild::run(args, &context).map_err(|e| e.into()),
        Commands::Status(args) => commands::status::run(args, &context).map_err(|e| e.into()),
        Commands::Lock(args) => commands::lock::run(args, &context).map_err(|e| e.into()),
        Commands::Unlock(args) => commands::unlock::run(args, &context).map_err(|e| e.into()),
        Commands::Guard(args) => commands::guard::run(args, &context).map_err(|e| e.into()),
    }
}
