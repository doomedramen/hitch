use clap::{Parser, Subcommand};
use std::sync::Arc;

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
pub struct Cli {
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
    /// Release environment branches to target branch
    Release(commands::release::ReleaseCommand),
    /// Show status of environments and promoted branches
    Status(commands::status::StatusCommand),
    /// Lock environment to prevent deployments
    Lock(commands::lock::LockCommand),
    /// Unlock environment to allow deployments
    Unlock(commands::unlock::UnlockCommand),
    /// Guard against direct commits to environment branches
    Guard(commands::guard::GuardCommand),
    /// Generate shell completion script
    Completion(commands::completion::CompletionCommand),
    /// Manage approval requests for deployments
    Approvals(commands::approvals::ApprovalsCommand),
    /// Resume or abort a rebuild paused by a merge conflict
    Resolve(commands::resolve::ResolveCommand),
    /// Remove local branches that are no longer promoted to any environment
    Cleanup(commands::cleanup::CleanupCommand),
    /// Preview the commits each promoted branch would add on rebuild
    Diff(commands::diff::DiffCommand),
    /// Update environment configuration (e.g., change base branch)
    Set(commands::set::SetCommand),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize colored output
    colored::control::set_override(true);

    let cli = Cli::parse();

    // Configure logger for the specific command
    let command_name = match &cli.command {
        Commands::Init(_) => "init",
        Commands::Add(_) => "add",
        Commands::Remove(_) => "remove",
        Commands::Promote(_) => "promote",
        Commands::Demote(_) => "demote",
        Commands::Rebuild(_) => "rebuild",
        Commands::Release(_) => "release",
        Commands::Status(_) => "status",
        Commands::Lock(_) => "lock",
        Commands::Unlock(_) => "unlock",
        Commands::Guard(_) => "guard",
        Commands::Completion(_) => "completion",
        Commands::Approvals(_) => "approvals",
        Commands::Resolve(_) => "resolve",
        Commands::Cleanup(_) => "cleanup",
        Commands::Diff(_) => "diff",
        Commands::Set(_) => "set",
    };

    // Create a new logger configured for this command
    use crate::utils::logging::Logger;
    let logger = Arc::new(Logger::for_command(command_name, cli.verbose));

    // Create global context with flags
    let context = GlobalContext::new(cli.verbose, cli.no_push, logger)?;

    // Execute the appropriate command
    match cli.command {
        Commands::Init(args) => commands::init::run(args, &context).map_err(|e| e.into()),
        Commands::Add(args) => commands::add::run(args, &context).map_err(|e| e.into()),
        Commands::Remove(args) => commands::remove::run(args, &context).map_err(|e| e.into()),
        Commands::Promote(args) => commands::promote::run(args, &context).map_err(|e| e.into()),
        Commands::Demote(args) => commands::demote::run(args, &context).map_err(|e| e.into()),
        Commands::Rebuild(args) => commands::rebuild::run(args, &context).map_err(|e| e.into()),
        Commands::Release(args) => commands::release::run(args, &context).map_err(|e| e.into()),
        Commands::Status(args) => commands::status::run(args, &context).map_err(|e| e.into()),
        Commands::Lock(args) => commands::lock::run(args, &context).map_err(|e| e.into()),
        Commands::Unlock(args) => commands::unlock::run(args, &context).map_err(|e| e.into()),
        Commands::Guard(args) => commands::guard::run(args, &context).map_err(|e| e.into()),
        Commands::Completion(args) => {
            commands::completion::run(args, &context).map_err(|e| e.into())
        }
        Commands::Approvals(args) => commands::approvals::run(args, &context).map_err(|e| e.into()),
        Commands::Resolve(args) => commands::resolve::run(args, &context).map_err(|e| e.into()),
        Commands::Cleanup(args) => commands::cleanup::run(args, &context).map_err(|e| e.into()),
        Commands::Diff(args) => commands::diff::run(args, &context).map_err(|e| e.into()),
        Commands::Set(args) => commands::set::run(args, &context).map_err(|e| e.into()),
    }
}
