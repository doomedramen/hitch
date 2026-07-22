use crate::commands;
use clap::{Parser, Subcommand};

/// Lives in the library crate (rather than the `hitch` binary) so both
/// `main.rs` and `commands::completion` can share a single source of truth
/// for the command tree — completions are generated straight from this via
/// `clap_complete::generate`, so they can never drift from the real commands,
/// subcommands, and flags.
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
    pub verbose: bool,

    /// Skip automatic pushes when metadata is committed
    #[arg(long, global = true)]
    pub no_push: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// Show hierarchy of branches and environments
    Tree(commands::tree::TreeCommand),
    /// Lock environment to prevent deployments
    Lock(commands::lock::LockCommand),
    /// Unlock environment to allow deployments
    Unlock(commands::unlock::UnlockCommand),
    /// Create or open a GitHub pull request for the current branch
    Pr(commands::pr::PrCommand),
    /// Check that 'gh' is installed, authenticated, and has the scopes 'hitch pr' needs
    Doctor(commands::doctor::DoctorCommand),
    /// Guard against direct commits to environment branches
    Guard(commands::guard::GuardCommand),
    /// Generate shell completion script
    Completion(commands::completion::CompletionCommand),
    /// Manage approval requests for deployments
    Approvals(commands::approvals::ApprovalsCommand),
    /// Create a new branch and set promotion targets
    Branch(commands::branch::BranchCommand),
    /// Configure branch protection for hitch PR workflow
    Setup(commands::setup::SetupCommand),
    /// Remove local branches that are no longer promoted to any environment
    Cleanup(commands::cleanup::CleanupCommand),
    /// Preview the commits each promoted branch would add on rebuild
    Diff(commands::diff::DiffCommand),
    /// Update environment configuration (e.g., change base branch)
    Set(commands::set::SetCommand),
}
