use crate::commands::global_context::GlobalContext;
use crate::types::{Environment, RebuildSnapshot};
use anyhow::{anyhow, Result};

/// Capture a snapshot of branch states for an approval request
pub fn capture_rebuild_snapshot(
    context: &GlobalContext,
    environment: &Environment,
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
                .map_err(|e| {
                    anyhow!("Failed to get SHA for branch '{}': {}", existing_branch, e)
                })?;
            branch_shas.insert(existing_branch.clone(), existing_sha);
        }
    }

    // Check for merge conflicts
    let merge_conflicts = check_for_merge_conflicts(context, environment, branch_to_promote)?;

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

/// Check for merge conflicts between branches
fn check_for_merge_conflicts(
    context: &GlobalContext,
    environment: &Environment,
    branch_to_promote: &str,
) -> Result<bool> {
    context.log_verbose("Checking for merge conflicts...");

    // Switch to base branch to check conflicts
    let current_branch = context.git().get_current_branch()?;
    context.git().checkout_branch(&environment.base)?;

    // Check conflicts between base and new branch
    let conflict_result = context
        .git()
        .check_merge_conflicts_comprehensive(branch_to_promote)?;

    let mut has_conflicts = conflict_result.has_conflicts;

    // If no conflicts with new branch, check conflicts with existing promoted branches
    if !has_conflicts {
        for existing_branch in &environment.branches {
            if existing_branch != branch_to_promote {
                let conflicts = context
                    .git()
                    .check_merge_conflicts_comprehensive(existing_branch)?;
                if conflicts.has_conflicts {
                    context.log_verbose(&format!(
                        "Merge conflict detected with existing branch: {}",
                        existing_branch
                    ));
                    has_conflicts = true;
                    break;
                }
            }
        }
    }

    // Return to original branch
    context.git().checkout_branch(&current_branch)?;

    Ok(has_conflicts)
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

/// Format snapshot information for display
#[allow(dead_code)]
pub fn format_snapshot_info(snapshot: &RebuildSnapshot) -> String {
    let mut info = String::new();

    info.push_str(&format!(
        "Base branch: {} ({})\n",
        snapshot.base_branch,
        &snapshot.base_sha[..7.min(snapshot.base_sha.len())]
    ));

    if !snapshot.branch_shas.is_empty() {
        info.push_str("Branches to merge:\n");
        for (branch, sha) in &snapshot.branch_shas {
            info.push_str(&format!("  - {} ({})\n", branch, &sha[..7.min(sha.len())]));
        }
    }

    info.push_str(&format!(
        "Merge conflicts: {}\n",
        if snapshot.merge_conflicts {
            "Yes"
        } else {
            "No"
        }
    ));

    info
}
