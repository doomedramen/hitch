use crate::commands::global_context::GlobalContext;
use crate::types::{RollbackInfo, RollbackOperation};
use crate::utils::command_helpers::{environment::get_locked_by_user, logging::validation_success};
use crate::utils::validation::validate_name;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct DemoteCommand {
    /// The branch to demote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to demote the branch from
    #[arg()]
    pub env_name: String,

    /// Skip the automatic rebuild after demotion.
    /// Use this to batch multiple demotes and then run 'hitch rebuild <env>' once.
    #[arg(long)]
    pub no_rebuild: bool,
}

pub fn run(args: DemoteCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!(
        "Demoting branch '{}' from environment '{}'...",
        args.branch, args.env_name
    ));

    // Step 1: Ensure we are in a Git repository
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Step 2: Resolve branches — if the branch argument is an environment name,
    // demote all branches from that environment instead.
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
        RollbackOperation::Demote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Snapshot which resolved branches are actually promoted to the target env, so the
    // success message reports what was really demoted. Demotion no-ops branches that
    // aren't present, and resolving a source environment can include such branches.
    // Validation guarantees at least one resolved branch is present here.
    let demoted: Vec<String> = {
        let config =
            crate::utils::prelude::access_metadata_read_only(context, |c| Ok(c.clone()))?;
        match config.environments.get(&args.env_name) {
            Some(e) => branches
                .iter()
                .filter(|b| e.branches.contains(*b))
                .cloned()
                .collect(),
            None => branches.clone(),
        }
    };

    // Step 4: Auto-stash dirty changes, execute demotion, then pop stash
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            demote_branches_from_environment(
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
            if demoted.len() == 1 {
                context.log_success(&format!(
                    "Successfully demoted '{}' from environment '{}'!",
                    demoted[0], args.env_name
                ));
            } else {
                context.log_success(&format!(
                    "Successfully demoted {} branches from environment '{}'!",
                    demoted.len(),
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

    // Check if the branch arg is an environment name.
    // If so, resolve it to that environment's promoted branches.
    if let Some(source_env) = config.environments.get(branch) {
        // Prevent demoting an environment's own branches from itself
        if branch == env_name {
            return Err(anyhow::anyhow!(
                "Cannot demote environment '{}' from itself",
                branch
            ));
        }

        if source_env.branches.is_empty() {
            return Err(anyhow::anyhow!(
                "Environment '{}' has no branches promoted. Nothing to demote.",
                branch
            ));
        }

        Ok(source_env.branches.clone())
    } else {
        Ok(vec![branch.to_string()])
    }
}

/// Validate that each branch and the target environment are ready for demotion.
///
/// Returns `Ok(true)` when the environment requires approval and approval
/// requests were created for every branch — the caller must stop and NOT apply
/// the demotion. Returns `Ok(false)` when validation passed and the caller
/// should proceed to apply the demotion.
fn validate_preconditions(
    context: &GlobalContext,
    branches: &[String],
    env_name: &str,
) -> anyhow::Result<bool> {
    context.log_verbose("Validating demotion preconditions...");

    // Validate each branch name individually
    for branch in branches {
        validate_name(branch, "Branch")?;
    }
    validate_name(env_name, "Environment")?;

    // Check if environment exists
    crate::utils::command_helpers::ensure_environment_exists(context, env_name)?;

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

    // Check each branch to ensure at least one is promoted to this environment.
    // For the single-branch case this behaves the same as before. For the
    // environment-resolved case, branches not in the target are silently
    // skipped rather than erroring (partial demotion).
    let mut found = false;
    for branch in branches {
        if environment.branches.contains(branch) {
            found = true;
        }
    }
    if !found {
        return Err(anyhow::anyhow!(
            "None of the branches from '{}' are promoted to environment '{}'",
            branches.join(", "),
            env_name
        ));
    }

    // Check if approval is required for this environment
    if environment.requires_approval_check() {
        context.log_info(&format!(
            "Environment '{}' requires approval before demotion",
            env_name
        ));

        // Only request approval for branches actually promoted to this environment.
        // Resolving a source environment can include branches that were never promoted
        // here; demoting those is a no-op, so creating approval requests for them would
        // leave bogus pending requests that demote nothing when approved.
        let present: Vec<String> = branches
            .iter()
            .filter(|b| environment.branches.contains(*b))
            .cloned()
            .collect();

        // Create all requests in one transaction so a mid-batch failure doesn't leave the
        // earlier ones committed.
        let request_ids = crate::utils::prelude::create_approval_requests_for_operation(
            context,
            env_name,
            &present,
            crate::types::Operation::Demote,
        )?;
        for request_id in &request_ids {
            crate::utils::prelude::display_approval_request_created(context, request_id)?;
        }

        return Ok(true);
    }

    validation_success(
        context,
        &format!("{} branch(es) from '{}'", branches.len(), env_name),
        "Demotion validation",
    );
    Ok(false)
}

/// Remove branches from environment and trigger rebuild
fn demote_branches_from_environment(
    context: &GlobalContext,
    branches: &[String],
    env_name: &str,
    rollback_info: &mut RollbackInfo,
    no_rebuild: bool,
) -> anyhow::Result<()> {
    context.log_verbose(&format!(
        "Removing {} branch(es) from environment '{}'...",
        branches.len(),
        env_name
    ));

    // Capture pre-operation configuration for rollback
    rollback_info.previous_config = crate::utils::rollback::capture_config_state(context)?;

    // Modify metadata to remove each branch present in the environment
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
            environment.remove_branch(branch);
            context.log_verbose(&format!(
                "✓ Removed '{}' from environment '{}'",
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
