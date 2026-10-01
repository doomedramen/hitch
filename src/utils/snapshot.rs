use crate::commands::global_context::GlobalContext;
use crate::types::{Environment, RebuildSnapshot};
use anyhow::{anyhow, Result};

/// Capture a snapshot of branch states for an approval request
pub fn capture_rebuild_snapshot(
    context: &GlobalContext,
    environment: &Environment,
    env_name: &str,
    branch_to_promote: &str,
) -> Result<RebuildSnapshot> {
    context.log_verbose("Capturing rebuild snapshot for approval request...");

    // Get base branch SHA
    let base_sha = context
        .git()
        .get_branch_commit_sha(&environment.base)
        .map_err(|e| {
            anyhow!(
                "Failed to get SHA for base branch '{}': {}",
                environment.base,
                e
            )
        })?;

    // Initialize branch SHAs map
    let mut branch_shas = std::collections::HashMap::new();

    // Add SHA for the branch being promoted
    let branch_sha = context
        .git()
        .get_branch_commit_sha(branch_to_promote)
        .map_err(|e| {
            anyhow!(
                "Failed to get SHA for branch '{}': {}",
                branch_to_promote,
                e
            )
        })?;
    branch_shas.insert(branch_to_promote.to_string(), branch_sha);

    // Add SHAs for all currently promoted branches
    for existing_branch in &environment.branches {
        if existing_branch != branch_to_promote {
            let existing_sha = context
                .git()
                .get_branch_commit_sha(existing_branch)
                .map_err(|e| anyhow!("Could not read branch '{}': {}", existing_branch, e))?;
            branch_shas.insert(existing_branch.clone(), existing_sha);
        }
    }

    // Check for merge conflicts
    let merge_conflicts =
        check_for_merge_conflicts(context, environment, env_name, branch_to_promote)?;

    context.log_verbose(&format!(
        "Snapshot captured: base={} ({}), {} branches, conflicts={}",
        environment.base,
        &base_sha[..7.min(base_sha.len())],
        branch_shas.len(),
        merge_conflicts
    ));

    Ok(RebuildSnapshot {
        base_branch: environment.base.clone(),
        base_sha,
        branch_shas,
        merge_conflicts,
    })
}

/// Validate that the snapshot is still current (no branches have changed)
pub fn validate_snapshot(context: &GlobalContext, snapshot: &RebuildSnapshot) -> Result<()> {
    context.log_verbose("Validating snapshot freshness...");

    // Validate base branch hasn't changed
    let current_base_sha = context.git().get_branch_commit_sha(&snapshot.base_branch)?;

    if current_base_sha != snapshot.base_sha {
        return Err(anyhow!(
            "Base branch has changed since approval was requested\n\n\
            Expected SHA: {}\n\
            Current SHA:  {}\n\n\
            A new approval request is required.",
            &snapshot.base_sha[..7.min(snapshot.base_sha.len())],
            &current_base_sha[..7.min(current_base_sha.len())]
        ));
    }

    // Validate all promoted branches haven't changed
    for (branch_name, expected_sha) in &snapshot.branch_shas {
        match context.git().get_branch_commit_sha(branch_name) {
            Ok(current_sha) => {
                if current_sha != *expected_sha {
                    return Err(anyhow!(
                        "Branch '{}' has changed since approval was requested\n\n\
                        Expected SHA: {}\n\
                        Current SHA:  {}\n\n\
                        A new approval request is required.",
                        branch_name,
                        &expected_sha[..7.min(expected_sha.len())],
                        &current_sha[..7.min(current_sha.len())]
                    ));
                }
            }
            Err(_) => {
                return Err(anyhow!(
                    "Branch '{}' no longer exists\n\n\
                    A new approval request is required.",
                    branch_name
                ));
            }
        }
    }

    context.log_verbose("Snapshot validation passed - all branches unchanged");
    Ok(())
}

/// Whether a build of `environment` with `branch_to_promote` declared would
/// hold anything. Asked of the composition a rebuild runs, so a pair of
/// branches that only collide with each other counts, which the base-only
/// pairwise check this replaced missed. Syncs first, as that check did; the
/// prediction itself is offline.
fn check_for_merge_conflicts(
    context: &GlobalContext,
    environment: &Environment,
    env_name: &str,
    branch_to_promote: &str,
) -> Result<bool> {
    context.log_verbose("Checking for merge conflicts...");

    let mut proposed = environment.clone();
    if !proposed.branches.iter().any(|b| b == branch_to_promote) {
        proposed.branches.push(branch_to_promote.to_string());
    }

    let mut all_branches = vec![proposed.base.clone()];
    all_branches.extend(proposed.branches.iter().cloned());
    context.git().synchronize_branches(&all_branches)?;

    let prediction = crate::utils::prelude::predict_composition(context, &proposed, env_name)?;
    Ok(!prediction.held.is_empty())
}

/// Return the list of branch names (including the base branch) that have
/// changed since the snapshot was taken.  Returns an empty vec if nothing
/// has drifted or if git calls fail (treats errors as "not drifted" so that
/// a network outage doesn't show false positives in the list command).
pub fn drifted_branches(context: &GlobalContext, snapshot: &RebuildSnapshot) -> Vec<String> {
    let mut drifted = Vec::new();

    // Check base branch
    if let Ok(current) = context.git().get_branch_commit_sha(&snapshot.base_branch) {
        if current != snapshot.base_sha {
            drifted.push(format!("{} (base)", snapshot.base_branch));
        }
    }

    // Check each tracked branch
    for (branch, expected_sha) in &snapshot.branch_shas {
        match context.git().get_branch_commit_sha(branch) {
            Ok(current) if current != *expected_sha => {
                drifted.push(branch.clone());
            }
            Err(_) => {
                drifted.push(format!("{} (deleted)", branch));
            }
            _ => {}
        }
    }

    drifted
}
