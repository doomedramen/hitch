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

    // Step 1: Pre-checks (ensure clean working tree, etc.)
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Create the branch from the specified base
    context
        .git()
        .create_branch_from(&args.branch_name, &args.from_branch)?;

    // Step 3: Checkout the new branch
    context.git().checkout_branch(&args.branch_name)?;

    // Step 4: Add the new branch to the promotion targets in metadata
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
