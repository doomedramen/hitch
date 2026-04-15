use crate::commands::global_context::GlobalContext;
use crate::types::Environment;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct StatusSummary {
    pub total_envs: usize,
    pub locked_envs: usize,
    pub needs_rebuild_envs: usize,
    pub never_rebuilt_envs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildState {
    UpToDate,
    NeedsRebuild { newer_branches: Vec<String> },
    NeverRebuilt,
}

#[derive(Debug, Clone)]
pub struct EnvironmentStatusModel {
    pub name: String,
    pub base: String,
    pub branches: Vec<String>,
    pub locked: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub rebuilt_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub requires_approval: bool,
    pub min_approvals: usize,
    pub approvers: Vec<String>,
    pub rebuild_state: RebuildState,
}

#[derive(Debug, Clone)]
pub struct StatusModel {
    #[allow(dead_code)]
    pub current_branch: Option<String>,
    pub summary: StatusSummary,
    pub environments: Vec<EnvironmentStatusModel>,
}

pub fn build_status_model(context: &GlobalContext) -> Result<StatusModel> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    let current_branch = context.git().get_current_branch().ok();

    let mut environments: Vec<EnvironmentStatusModel> = config
        .environments
        .iter()
        .map(|(name, env)| build_env_model(context, name, env))
        .collect::<Result<Vec<_>>>()?;

    environments.sort_by(|a, b| a.name.cmp(&b.name));

    let total_envs = environments.len();
    let locked_envs = environments.iter().filter(|e| e.locked).count();
    let needs_rebuild_envs = environments
        .iter()
        .filter(|e| matches!(e.rebuild_state, RebuildState::NeedsRebuild { .. }))
        .count();
    let never_rebuilt_envs = environments
        .iter()
        .filter(|e| matches!(e.rebuild_state, RebuildState::NeverRebuilt))
        .count();

    Ok(StatusModel {
        current_branch,
        summary: StatusSummary {
            total_envs,
            locked_envs,
            needs_rebuild_envs,
            never_rebuilt_envs,
        },
        environments,
    })
}

fn build_env_model(
    context: &GlobalContext,
    name: &str,
    env: &Environment,
) -> Result<EnvironmentStatusModel> {
    Ok(EnvironmentStatusModel {
        name: name.to_string(),
        base: env.base.clone(),
        branches: env.branches.clone(),
        locked: env.is_locked(),
        locked_by: env.locked_by.clone(),
        locked_at: env.locked_at,
        rebuilt_at: env.rebuilt_at,
        released_at: env.released_at,
        requires_approval: env.requires_approval,
        min_approvals: env.min_approvals,
        approvers: env.approvers.clone(),
        rebuild_state: determine_rebuild_state(context, env),
    })
}

fn determine_rebuild_state(context: &GlobalContext, env: &Environment) -> RebuildState {
    let rebuilt_at = match env.rebuilt_at {
        Some(ts) => ts,
        None => return RebuildState::NeverRebuilt,
    };

    let mut newer = Vec::new();

    // Base branch
    if context
        .git()
        .branch_exists_anywhere(&env.base)
        .ok()
        .unwrap_or(false)
    {
        if let Ok(sha) = context.git().get_branch_commit_sha(&env.base) {
            if let Ok(ts) = context.git().get_commit_timestamp(&sha) {
                if ts > rebuilt_at {
                    newer.push(env.base.clone());
                }
            }
        }
    }

    // Promoted branches
    for branch in &env.branches {
        if context
            .git()
            .branch_exists_anywhere(branch)
            .ok()
            .unwrap_or(false)
        {
            if let Ok(sha) = context.git().get_branch_commit_sha(branch) {
                if let Ok(ts) = context.git().get_commit_timestamp(&sha) {
                    if ts > rebuilt_at {
                        newer.push(branch.clone());
                    }
                }
            }
        }
    }

    if newer.is_empty() {
        RebuildState::UpToDate
    } else {
        RebuildState::NeedsRebuild {
            newer_branches: newer,
        }
    }
}
