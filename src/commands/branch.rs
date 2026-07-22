use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct BranchCommand {
    /// Name of the new branch to create
    pub branch_name: String,

    /// The branch to create the new branch from
    pub from_branch: String,

    /// Branches this branch should be promoted to (repeatable)
    #[arg(long = "to")]
    pub promote_to: Vec<String>,
}

pub fn run(args: BranchCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Creating branch '{}' from '{}'...",
        args.branch_name, args.from_branch
    ));

    crate::utils::prelude::pre_check(context)?;

    context
        .git()
        .create_branch_from(&args.branch_name, &args.from_branch)?;

    context.git().checkout_branch(&args.branch_name)?;

    if !args.promote_to.is_empty() {
        crate::utils::prelude::modify_metadata(context, |config| {
            for env_name in &args.promote_to {
                if let Some(env) = config.environments.get_mut(env_name) {
                    env.add_branch(args.branch_name.clone());
                } else {
                    return Err(anyhow::anyhow!(
                        "Promotion target environment '{}' does not exist in hitch metadata.",
                        env_name
                    ));
                }
            }
            Ok(())
        })?;
    }

    context.log_success(&format!(
        "Branch '{}' created from '{}' and checked out!",
        args.branch_name, args.from_branch
    ));
    Ok(())
}
