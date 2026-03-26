use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ApprovalsCommand {
    #[command(subcommand)]
    pub subcommand: ApprovalsSubcommand,
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
