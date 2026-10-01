//! Desktop-local adapters from the core's typed models to the JSON the React
//! UI reads. The core deliberately has no desktop-shaped views; these are built
//! over `RepositoryStateSnapshot` and `ActivityLog`, and keep the DTO shapes
//! (including the `key: value` overview text) the UI parses.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use hitch::commands::global_context::GlobalContext;
use hitch::core::activity::{build_activity, ActivityQuery, HitchEvent};
use hitch::core::render::render_event;
use hitch::core::state::{build_state_snapshot, EnvironmentHealth, EnvironmentState};
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::types::{
    BranchDetailsDto, BranchRowDto, EnvironmentDetailsDto, EnvironmentMinDto, TimelineItemDto,
    TimelineKindDto, WorkspaceIndexDto,
};

const COMMIT_LIMIT: usize = 50;
const EVENT_LIMIT: usize = 80;
const LISTED_BRANCH_LIMIT: usize = 25;

pub fn workspace_index(ctx: &GlobalContext) -> Result<WorkspaceIndexDto> {
    let snapshot = build_state_snapshot(ctx)?;
    let rows = branch_rows(ctx, &snapshot.environments);

    let mut environments: Vec<EnvironmentMinDto> = snapshot
        .environments
        .iter()
        .map(|e| EnvironmentMinDto {
            name: e.name.clone(),
            base: e.base.clone(),
            promoted_count: e.desired.branches.len(),
            locked: e.locked,
            requires_approval: e.approval_policy.required,
            min_approvals: e.approval_policy.min_approvals,
            approvers_count: e.approval_policy.approvers.len(),
        })
        .collect();
    environments.sort_by(|a, b| a.name.cmp(&b.name));

    let mut promoted_branches = Vec::new();
    let mut branches = Vec::new();
    for row in rows {
        if row.is_environment {
            continue;
        }
        if row.promoted_to.is_empty() {
            branches.push(row);
        } else {
            promoted_branches.push(row);
        }
    }

    Ok(WorkspaceIndexDto {
        current_branch: snapshot.current_branch,
        environments,
        promoted_branches,
        branches,
    })
}

/// Every local and `origin` branch, sorted by name, with its environment
/// relationships read from the declared compositions.
fn branch_rows(ctx: &GlobalContext, envs: &[EnvironmentState]) -> Vec<BranchRowDto> {
    let env_names: HashSet<&str> = envs.iter().map(|e| e.name.as_str()).collect();
    let mut promoted_to: HashMap<&str, Vec<String>> = HashMap::new();
    let mut base_for: HashMap<&str, Vec<String>> = HashMap::new();
    for env in envs {
        base_for
            .entry(env.base.as_str())
            .or_default()
            .push(env.name.clone());
        for b in &env.desired.branches {
            promoted_to
                .entry(b.name.as_str())
                .or_default()
                .push(env.name.clone());
        }
    }

    let locals = ctx.git().list_local_branches().unwrap_or_default();
    let remotes = ctx.git().list_remote_branches("origin").unwrap_or_default();
    let local_set: HashSet<&String> = locals.iter().collect();
    let remote_set: HashSet<&String> = remotes.iter().collect();
    let all: BTreeSet<&String> = locals.iter().chain(remotes.iter()).collect();

    all.into_iter()
        .map(|name| BranchRowDto {
            name: name.clone(),
            local: local_set.contains(name),
            remote: remote_set.contains(name),
            is_environment: env_names.contains(name.as_str()),
            promoted_to: promoted_to.get(name.as_str()).cloned().unwrap_or_default(),
            base_for: base_for.get(name.as_str()).cloned().unwrap_or_default(),
        })
        .collect()
}

/// A ref usable in `git log` / `git diff` when the branch may be remote-only.
fn git_ref(row: &BranchRowDto) -> String {
    if !row.local && row.remote {
        format!("origin/{}", row.name)
    } else {
        row.name.clone()
    }
}

pub fn branch_details(ctx: &GlobalContext, branch_name: &str) -> Result<BranchDetailsDto> {
    let snapshot = build_state_snapshot(ctx)?;
    let row = branch_rows(ctx, &snapshot.environments)
        .into_iter()
        .find(|b| b.name == branch_name)
        .ok_or_else(|| anyhow!("Branch '{}' not found", branch_name))?;

    let branch_sha = ctx.git().get_branch_commit_sha(&row.name)?;
    let bases: Vec<&str> = row
        .promoted_to
        .iter()
        .filter_map(|n| snapshot.environments.iter().find(|e| &e.name == n))
        .map(|e| e.base.as_str())
        .collect();
    let overview = branch_overview(ctx, &row, &bases);
    let timeline = timeline(
        ctx,
        &git_ref(&row),
        ActivityQuery {
            branch: Some(row.name.clone()),
            limit: EVENT_LIMIT,
            ..Default::default()
        },
        |e| e.branches().contains(&row.name.as_str()),
    );

    Ok(BranchDetailsDto {
        branch: row,
        branch_sha,
        metadata_sha: snapshot.metadata_sha,
        overview,
        timeline,
    })
}

pub fn env_details(ctx: &GlobalContext, env_name: &str) -> Result<EnvironmentDetailsDto> {
    let snapshot = build_state_snapshot(ctx)?;
    let env = snapshot
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .ok_or_else(|| anyhow!("Environment '{}' not found", env_name))?;
    let env_sha = ctx.git().get_branch_commit_sha(env_name)?;

    let timeline = timeline(
        ctx,
        env_name,
        ActivityQuery {
            environment: Some(env_name.to_string()),
            limit: EVENT_LIMIT,
            ..Default::default()
        },
        |e| e.environment() == env_name,
    );

    Ok(EnvironmentDetailsDto {
        name: env_name.to_string(),
        env_sha,
        metadata_sha: snapshot.metadata_sha.clone(),
        overview: env_overview(env),
        timeline,
    })
}

/// Recent commits plus hitch events, newest first. Either half failing leaves
/// the other (a timeline is a convenience, not a verdict).
fn timeline(
    ctx: &GlobalContext,
    reference: &str,
    query: ActivityQuery,
    keep: impl Fn(&HitchEvent) -> bool,
) -> Vec<TimelineItemDto> {
    let mut items: Vec<(DateTime<Utc>, TimelineItemDto)> = Vec::new();

    if let Ok(commits) = ctx.git().list_commits(reference, COMMIT_LIMIT) {
        for c in commits {
            items.push((
                c.timestamp,
                TimelineItemDto {
                    when: c.timestamp.to_rfc3339(),
                    kind: TimelineKindDto::GitCommit,
                    summary: format!("{} {}", &c.sha[..7.min(c.sha.len())], c.summary),
                    detail: None,
                },
            ));
        }
    }

    if let Ok(log) = build_activity(ctx, &query) {
        for entry in &log.entries {
            for event in entry.events.iter().filter(|e| keep(e)) {
                items.push((
                    entry.when,
                    TimelineItemDto {
                        when: entry.when.to_rfc3339(),
                        kind: TimelineKindDto::HitchEvent,
                        summary: render_event(event),
                        detail: None,
                    },
                ));
            }
        }
    }

    items.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    items.into_iter().map(|(_, item)| item).collect()
}

/// Short, jargon-free phrase for the overview's `rebuild:` line.
fn rebuild_phrase(health: &EnvironmentHealth) -> String {
    match health {
        EnvironmentHealth::Realised => "up to date".to_string(),
        EnvironmentHealth::PartiallyRealised { held } => {
            format!("up to date, holding back {}", held.join(", "))
        }
        EnvironmentHealth::NeedsRebuild {
            changed_inputs,
            added,
            removed,
        } => {
            let mut parts = Vec::new();
            if !changed_inputs.is_empty() {
                let names: Vec<&str> = changed_inputs.iter().map(|c| c.branch.as_str()).collect();
                parts.push(format!("new commits in {}", names.join(", ")));
            }
            if !added.is_empty() {
                parts.push(format!("added {}", added.join(", ")));
            }
            if !removed.is_empty() {
                parts.push(format!("removed {}", removed.join(", ")));
            }
            if parts.is_empty() {
                "needed".to_string()
            } else {
                format!("needed ({})", parts.join("; "))
            }
        }
        EnvironmentHealth::NeverBuilt => "never rebuilt".to_string(),
        EnvironmentHealth::LegacyUnknown => "unknown (no build record)".to_string(),
        EnvironmentHealth::MissingBranch => "branch missing".to_string(),
    }
}

fn env_overview(env: &EnvironmentState) -> String {
    let mut lines = Vec::new();
    lines.push(format!("base: {}", env.base));
    lines.push(format!("locked: {}", if env.locked { "yes" } else { "no" }));
    if env.locked {
        if let Some(by) = &env.locked_by {
            lines.push(format!("locked_by: {}", by));
        }
        if let Some(ts) = env.locked_at {
            lines.push(format!("locked_at: {}", ts.to_rfc3339()));
        }
    }
    let declared = &env.desired.branches;
    lines.push(format!("branches ({}):", declared.len()));
    for b in declared.iter().take(LISTED_BRANCH_LIMIT) {
        lines.push(format!("  - {}", b.name));
    }
    if declared.len() > LISTED_BRANCH_LIMIT {
        lines.push(format!(
            "  … +{} more",
            declared.len() - LISTED_BRANCH_LIMIT
        ));
    }
    lines.push(format!("rebuild: {}", rebuild_phrase(&env.health)));
    if env.approval_policy.required {
        lines.push(format!(
            "approvals: required (min {}, approvers {})",
            env.approval_policy.min_approvals,
            env.approval_policy.approvers.len()
        ));
    }
    if let Some(ts) = env.rebuilt_at {
        lines.push(format!("rebuilt_at: {}", ts.to_rfc3339()));
    }
    if let Some(ts) = env.released_at {
        lines.push(format!("released_at: {}", ts.to_rfc3339()));
    }
    lines.join("\n")
}

fn branch_overview(ctx: &GlobalContext, row: &BranchRowDto, env_bases: &[&str]) -> String {
    let mut lines = Vec::new();

    if row.promoted_to.is_empty() {
        lines.push("promoted_to: (none)".to_string());
    } else {
        lines.push(format!("promoted_to: {}", row.promoted_to.join(", ")));
    }

    let mut bases: Vec<&str> = env_bases.to_vec();
    bases.sort();
    bases.dedup();
    let compare_base = match bases.first() {
        Some(b) => (*b).to_string(),
        None => ctx
            .git()
            .get_default_branch_ref()
            .unwrap_or_else(|_| "main".to_string()),
    };
    lines.push(format!("compare_base: {}", compare_base));

    let branch_ref = git_ref(row);
    if let Ok((behind, ahead)) = ctx.git().ahead_behind(&compare_base, &branch_ref) {
        lines.push(format!("ahead/behind: +{} / -{}", ahead, behind));
    }
    if let Ok(stat) = ctx.git().get_diff_stat(&compare_base, &branch_ref) {
        let stat = stat.trim();
        if !stat.is_empty() {
            lines.push("diff_stat:".to_string());
            for l in stat.lines().take(8) {
                lines.push(format!("  {}", l));
            }
        }
    }
    if let Ok(c) = ctx.git().get_last_commit(&branch_ref) {
        lines.push(format!(
            "last_commit: {} {}",
            &c.sha[..7.min(c.sha.len())],
            c.summary
        ));
        lines.push(format!(
            "last_commit_at: {}",
            c.timestamp.format("%Y-%m-%d %H:%M UTC")
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hitch::core::state::ChangedInput;

    #[test]
    fn rebuild_phrase_names_what_moved() {
        assert_eq!(rebuild_phrase(&EnvironmentHealth::Realised), "up to date");
        assert_eq!(
            rebuild_phrase(&EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![ChangedInput {
                    branch: "feat".into(),
                    previous_sha: None,
                    current_sha: None,
                }],
                added: vec![],
                removed: vec!["old".into()],
            }),
            "needed (new commits in feat; removed old)"
        );
    }
}
