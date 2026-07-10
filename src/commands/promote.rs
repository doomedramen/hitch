use crate::commands::global_context::GlobalContext;
use crate::types::{RollbackInfo, RollbackOperation};
use crate::utils::command_helpers::{
    ensure_environment_exists, environment::get_locked_by_user, logging::validation_success,
    validate_branch_for_promotion,
};
use crate::utils::validation::validate_name;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PromoteCommand {
    /// The branch to promote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to promote the branch to
    #[arg()]
    pub env_name: String,

    /// Skip the automatic rebuild after promotion.
    /// Use this to batch multiple promotes and then run 'hitch rebuild <env>' once.
    #[arg(long)]
    pub no_rebuild: bool,
}

pub fn run(args: PromoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Promoting branch '{}' to environment '{}'...",
        args.branch, args.env_name
    ));

    // Step 1: Ensure we are in a Git repository
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Step 2: Resolve branches — if the branch argument is an environment name,
    // promote all branches from that environment instead.
    let branches = resolve_to_branches(context, &args.branch, &args.env_name)?;
    let is_resolved =
        branches.len() != 1 || branches.first().map(|b| b.as_str()) != Some(&args.branch);
    if is_resolved {
        context.log_info(&format!(
            "Resolved '{}' → {} branch(es): {}",
            args.branch,
            branches.len(),
            branches.join(", ")
        ));
    }

    // Step 3: Validate preconditions for all branches.
    // If any require approval, approval requests are created for them and we return early.
    if validate_preconditions(context, &branches, &args.env_name)? {
        return Ok(());
    }

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 4: Auto-stash dirty changes, execute promotion, then pop stash
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            promote_branches_to_environment(
                context,
                &branches,
                &args.env_name,
                &mut rollback_info,
                args.no_rebuild,
            )
        })
    });

    // Step 5: Handle result with automatic rollback on failure
    match result {
        Ok(()) => {
            if branches.len() == 1 {
                context.log_success(&format!(
                    "Successfully promoted '{}' to environment '{}'!",
                    branches[0], args.env_name
                ));
            } else {
                context.log_success(&format!(
                    "Successfully promoted {} branches to environment '{}'!",
                    branches.len(),
                    args.env_name
                ));
            }
            Ok(())
        }
        Err(e) => {
            // Show the actual error FIRST so user knows why it failed
            context.log_error(&format!("Error: {}", e));

            // Attempt automatic rollback
            if let Err(rollback_err) =
                crate::utils::rollback::rollback_metadata_changes(context, &rollback_info)
            {
                context.log_error(&format!(
                    "CRITICAL: Failed to rollback metadata changes: {}. Manual intervention may be required.",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}

/// Check if the branch argument is actually an environment name.
/// If so, return the branches promoted in that environment.
/// If not, return the original branch name as a single-element vector.
fn resolve_to_branches(
    context: &GlobalContext,
    branch: &str,
    env_name: &str,
) -> Result<Vec<String>> {
    let config =
        crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;

    if let Some(source_env) = config.environments.get(branch) {
        if source_env.branches.is_empty() {
            return Err(anyhow::anyhow!(
                "Environment '{}' has no branches promoted. Nothing to promote.",
                branch
            ));
        }

        // Prevent promoting an environment's own branches back into itself
        if branch == env_name {
            return Err(anyhow::anyhow!(
                "Cannot promote environment '{}' into itself",
                branch
            ));
        }

        Ok(source_env.branches.clone())
    } else {
        Ok(vec![branch.to_string()])
    }
}

/// Validate that each branch and the target environment are ready for promotion.
///
/// Returns `Ok(true)` when the environment requires approval and approval
/// requests were created for every branch — the caller must stop and NOT apply
/// the promotion. Returns `Ok(false)` when validation passed and the caller
/// should proceed to apply the promotion.
fn validate_preconditions(
    context: &GlobalContext,
    branches: &[String],
    env_name: &str,
) -> anyhow::Result<bool> {
    context.log_verbose("Validating promotion preconditions...");

    // Validate each branch name individually
    for branch in branches {
        validate_name(branch, "Branch")?;
    }
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    ensure_environment_exists(context, env_name)?;

    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = &config.environments[env_name];

    // Check if environment is locked
    if environment.is_locked() {
        return Err(anyhow::anyhow!(
            "Environment '{}' is currently locked by '{}'",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    // Check if any branch is already promoted to this environment
    for branch in branches {
        if environment.branches.contains(branch) {
            return Err(anyhow::anyhow!(
                "Branch '{}' is already promoted to environment '{}'",
                branch,
                env_name
            ));
        }
    }

    // Check each branch exists and is valid
    for branch in branches {
        validate_branch_for_promotion(context, branch)?;
    }

    // Check each new branch against already-promoted sibling branches
    if !environment.branches.is_empty() {
        context.log_verbose("Checking for conflicts with already-promoted branches...");
        for branch in branches {
            crate::utils::prelude::check_pre_promote_conflicts(
                context,
                branch,
                &environment.branches,
                &environment.base,
                env_name,
            )?;
        }
    }

    // Check if approval is required for this environment
    if environment.requires_approval_check() {
        context.log_info(&format!(
            "Environment '{}' requires approval before promotion",
            env_name
        ));

        // Create all requests in one transaction so a mid-batch failure (e.g. one branch
        // already has a pending request) doesn't leave the earlier ones committed.
        let request_ids = crate::utils::prelude::create_approval_requests_for_operation(
            context,
            env_name,
            branches,
            crate::types::Operation::Promote,
        )?;
        for request_id in &request_ids {
            crate::utils::prelude::display_approval_request_created(context, request_id)?;
        }

        return Ok(true);
    }

    validation_success(
        context,
        &format!("{} branch(es) to '{}'", branches.len(), env_name),
        "Promotion validation",
    );
    Ok(false)
}

/// Add branches to environment and trigger rebuild
fn promote_branches_to_environment(
    context: &GlobalContext,
    branches: &[String],
    env_name: &str,
    rollback_info: &mut RollbackInfo,
    no_rebuild: bool,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Adding {} branch(es) to environment '{}'...",
        branches.len(),
        env_name
    ));

    // Capture pre-operation configuration for rollback
    rollback_info.previous_config = crate::utils::rollback::capture_config_state(context)?;

    // Modify metadata to add each branch
    crate::utils::prelude::modify_metadata(context, |config| {
        let available_envs = config.get_environment_names().join(", ");
        let environment = config.get_environment_mut(env_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Environment '{}' not found in hitch configuration. Available environments: {}",
                env_name,
                available_envs
            )
        })?;

        for branch in branches {
            environment.add_branch(branch.to_string());
            context.log_verbose(&format!(
                "✓ Added '{}' to environment '{}'",
                branch, env_name
            ));
        }
        Ok(())
    })?;

    if no_rebuild {
        context.log_info(&format!(
            "Skipping rebuild for environment '{}' (--no-rebuild flag set). Run 'hitch rebuild {}' when ready.",
            env_name, env_name
        ));
    } else {
        context.log_info(&format!(
            "Triggering rebuild for environment '{}'...",
            env_name
        ));
        crate::utils::prelude::rebuild_environment(context, env_name)?;

        context.log_verbose(&format!(
            "✓ Environment '{}' rebuilt successfully",
            env_name
        ));
    }
    Ok(())
}
