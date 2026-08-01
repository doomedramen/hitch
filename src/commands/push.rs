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
        // Lease against what we last observed of origin, the same pattern
        // every other force-push caller uses (see `publish_environment_build`
        // and `hitch resolve`'s landing step). Leasing against `None` instead
        // tells `--force-with-lease` to expect the remote branch not to exist
        // at all, so it always fails once origin already has the branch —
        // the common case, not an edge case.
        let remote_sha_before = context
            .git()
            .rev_parse_opt(&format!("refs/remotes/origin/{}", args.branch))?;
        force_push_with_deploy_key_if_configured(context, &args.branch, &remote_sha_before)?;
    } else {
        context.log_info(&format!("Pushing '{}' to origin...", args.branch));
        push_branch_with_deploy_key_if_configured(context, &args.branch)?;
    }

    context.log_success(&format!("✓ Pushed '{}' to origin", args.branch));
    Ok(())
}
