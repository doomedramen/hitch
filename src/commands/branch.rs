use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::access_metadata_read_only;
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

    /// Create a GitHub PR after creating the branch
    #[arg(long)]
    pub pr: bool,

    /// PR title (optional; gh will prompt if omitted)
    #[arg(long, requires = "pr")]
    pub title: Option<String>,

    /// PR body/description (optional)
    #[arg(long, requires = "pr")]
    pub body: Option<String>,

    /// Open as a draft pull request
    #[arg(long, requires = "pr")]
    pub draft: bool,
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

    if args.pr {
        let base = infer_pr_base_for_branch_command(
            context,
            &args.from_branch,
            &args.promote_to,
        )?;

        context.log_info(&format!("Pushing branch '{}' to origin...", args.branch_name));
        match context.git().push_branch(&args.branch_name) {
            Ok(()) => context.log_verbose("✓ Branch pushed"),
            Err(e) => {
                context.log_warning(&format!(
                    "Failed to push '{}' to origin: {}\n\
                     Push manually and run: gh pr create --head {} --base {}",
                    args.branch_name, e, args.branch_name, base
                ));
                return Ok(());
            }
        }

        super::pr::run_gh_pr_create(
            &args.branch_name,
            &base,
            args.title.as_deref(),
            args.body.as_deref(),
            args.draft,
        )?;
    }

    context.log_success(&format!(
        "Branch '{}' created from '{}' and checked out!",
        args.branch_name, args.from_branch
    ));
    Ok(())
}

fn infer_pr_base_for_branch_command(
    context: &GlobalContext,
    from_branch: &str,
    promote_to: &[String],
) -> Result<String> {
    if promote_to.is_empty() {
        return Ok(from_branch.to_string());
    }

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    let first_base = match config.environments.get(&promote_to[0]) {
        Some(env) => env.base.clone(),
        None => {
            return Err(anyhow::anyhow!(
                "Promotion target environment '{}' does not exist",
                promote_to[0]
            ));
        }
    };

    for env_name in &promote_to[1..] {
        match config.environments.get(env_name) {
            Some(env) if env.base == first_base => {}
            Some(env) => {
                return Err(anyhow::anyhow!(
                    "Promotion targets have different base branches: '{}' has base '{}', but '{}' has base '{}'.\n\
                     Specify the PR target explicitly with: hitch pr --base <target>",
                    promote_to[0], first_base, env_name, env.base
                ));
            }
            None => {
                return Err(anyhow::anyhow!(
                    "Promotion target environment '{}' does not exist",
                    env_name
                ));
            }
        }
    }

    Ok(first_base)
}
