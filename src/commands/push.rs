use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{
    force_push_with_deploy_key_if_configured, push_branch_with_deploy_key_if_configured,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PushCommand {
    /// Branch to push to origin
    pub branch: String,

    /// Force-push the branch (with-lease, for rebuild recovery)
    #[arg(long, short = 'f')]
    pub force: bool,
}

pub fn run(args: PushCommand, context: &GlobalContext) -> Result<()> {
    if args.force {
        context.log_info(&format!("Force-pushing '{}' to origin...", args.branch));
        force_push_with_deploy_key_if_configured(context, &args.branch, &None)?;
    } else {
        context.log_info(&format!("Pushing '{}' to origin...", args.branch));
        push_branch_with_deploy_key_if_configured(context, &args.branch)?;
    }

    context.log_success(&format!("✓ Pushed '{}' to origin", args.branch));
    Ok(())
}
