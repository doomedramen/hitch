use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ApprovalsCommand {
    #[command(subcommand)]
    pub subcommand: ApprovalsSubcommand,
}

impl ApprovalsCommand {
    /// Whether this subcommand mutates repository state (and therefore needs the
    /// repository-wide lock). `list` and `status` are read-only.
    pub fn is_mutating(&self) -> bool {
        match self.subcommand {
            ApprovalsSubcommand::List(_) | ApprovalsSubcommand::Status(_) => false,
            ApprovalsSubcommand::Approve(_)
            | ApprovalsSubcommand::Reject(_)
            | ApprovalsSubcommand::Cancel(_)
            | ApprovalsSubcommand::Cleanup(_)
            | ApprovalsSubcommand::Refresh(_) => true,
        }
    }
}

#[derive(Subcommand)]
pub enum ApprovalsSubcommand {
    /// List approval requests
    List(list::ListArgs),
    /// Show detailed status of a specific approval request
    Status(status::StatusArgs),
    /// Approve a pending request
    Approve(approve::ApproveArgs),
    /// Reject a pending request
    Reject(reject::RejectArgs),
    /// Cancel a pending request
    Cancel(cancel::CancelArgs),
    /// Clean up old approval requests
    Cleanup(cleanup::CleanupArgs),
    /// Re-snapshot a drifted request with current branch SHAs (clears prior approvals)
    Refresh(refresh::RefreshArgs),
}

pub mod approve;
pub mod cancel;
pub mod cleanup;
pub mod list;
pub mod refresh;
pub mod reject;
pub mod status;

pub fn run(args: ApprovalsCommand, context: &GlobalContext) -> Result<()> {
    match args.subcommand {
        ApprovalsSubcommand::List(args) => list::run(args, context),
        ApprovalsSubcommand::Status(args) => status::run(args, context),
        ApprovalsSubcommand::Approve(args) => approve::run(args, context),
        ApprovalsSubcommand::Reject(args) => reject::run(args, context),
        ApprovalsSubcommand::Cancel(args) => cancel::run(args, context),
        ApprovalsSubcommand::Cleanup(args) => cleanup::run(args, context),
        ApprovalsSubcommand::Refresh(args) => refresh::run(args, context),
    }
}
