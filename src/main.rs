use clap::{Parser, Subcommand};

mod commands;
mod types;
mod utils;

use commands::global_context::GlobalContext;

#[derive(Parser)]
#[command(name = "hitch")]
#[command(about = "Git branch management for environment-based deployments")]
#[command(
    long_about = "Hitch is a CLI tool that brings environment branch management to Git. It helps you organize and track deployment branches (like `dev`, `qa`, `main`) with proper promotion workflows, locking mechanisms, and rebuild automation—turning chaotic branch-based releases into a structured, auditable process."
)]
#[command(version = env!("CARGO_PKG_VERSION"))]
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
    /// Initialize Hitch for environment branch management
    Init(commands::init::InitCommand),
    /// Add a new environment (e.g., dev, qa, staging)
    Add(commands::add::AddCommand),
    /// Remove an environment from configuration
    Remove(commands::remove::RemoveCommand),
    /// Promote a branch to an environment (deploy)
    Promote(commands::promote::PromoteCommand),
    /// Demote a branch from an environment (undeploy)
    Demote(commands::demote::DemoteCommand),
    /// Rebuild environment by merging promoted branches
    Rebuild(commands::rebuild::RebuildCommand),
    /// Show status of environments and promoted branches
    Status(commands::status::StatusCommand),
    /// Lock environment to prevent deployments
    Lock(commands::lock::LockCommand),
    /// Unlock environment to allow deployments
    Unlock(commands::unlock::UnlockCommand),
    /// Guard against direct commits to environment branches
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
