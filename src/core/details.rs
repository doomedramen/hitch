use crate::commands::global_context::GlobalContext;
use crate::core::status::{build_status_model, EnvironmentStatusModel, StatusModel};
use crate::core::timeline::{
    build_combined_timeline, HitchEventFilter, HitchEventScope, TimelineItem,
};
use crate::core::workspace::BranchRow;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct BranchDetailsModel {
    pub branch: BranchRow,
    pub branch_sha: String,
    pub metadata_sha: Option<String>,
    pub overview: String,
    pub timeline: Vec<TimelineItem>,
}

#[derive(Debug, Clone)]
pub struct EnvironmentDetailsModel {
    pub name: String,
    pub env_sha: String,
    pub metadata_sha: Option<String>,
    #[allow(dead_code)]
    pub env: EnvironmentStatusModel,
    #[allow(dead_code)]
    pub status: StatusModel,
    pub overview: String,
    pub timeline: Vec<TimelineItem>,
}

pub fn build_branch_details_model(
    context: &GlobalContext,
    branch: &BranchRow,
    branch_sha: String,
    metadata_sha: Option<String>,
) -> Result<BranchDetailsModel> {
    let overview = build_branch_overview(context, branch)?;
    let timeline = build_combined_timeline(
        context,
        &branch.git_ref(),
        50,
        80,
        HitchEventFilter {
            scope: HitchEventScope::Branch,
            environment: None,
            branch: Some(branch.name.as_str()),
        },
    )
    .unwrap_or_default();

    Ok(BranchDetailsModel {
        branch: branch.clone(),
        branch_sha,
        metadata_sha,
        overview,
        timeline,
    })
}

pub fn build_environment_details_model(
    context: &GlobalContext,
    env_name: &str,
    env_sha: String,
    metadata_sha: Option<String>,
) -> Result<EnvironmentDetailsModel> {
    let status = build_status_model(context)?;
    let env = status
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", env_name))?;

    let overview = build_env_overview(&env);
    let timeline = build_combined_timeline(
        context,
        env_name,
        50,
        80,
        HitchEventFilter {
            scope: HitchEventScope::Environment,
            environment: Some(env_name),
            branch: None,
        },
    )
    .unwrap_or_default();

    Ok(EnvironmentDetailsModel {
        name: env_name.to_string(),
        env_sha,
        metadata_sha,
        env,
        status,
        overview,
        timeline,
    })
}

fn build_env_overview(env: &EnvironmentStatusModel) -> String {
    let mut lines = Vec::new();
    lines.push(format!("base: {}", env.base));
    lines.push(format!("locked: {}", if env.locked { "yes" } else { "no" }));
    if env.locked {
        if let Some(by) = &env.locked_by {
            lines.push(format!("locked_by: {}", by));
        }
        if let Some(ts) = env.locked_at {
            lines.push(format!("locked_at: {}", ts.format("%Y-%m-%d %H:%M UTC")));
        }
    }
    lines.push(format!("branches ({}):", env.branches.len()));
    let limit = 25usize;
    for b in env.branches.iter().take(limit) {
        lines.push(format!("  - {}", b));
    }
    if env.branches.len() > limit {
        lines.push(format!("  … +{} more", env.branches.len() - limit));
    }
    match &env.rebuild_state {
        crate::core::status::RebuildState::UpToDate => {
            lines.push("rebuild: up to date".to_string())
        }
        crate::core::status::RebuildState::NeverRebuilt => {
            lines.push("rebuild: never rebuilt".to_string())
        }
        crate::core::status::RebuildState::NeedsRebuild { newer_branches } => lines.push(format!(
            "rebuild: needed (new commits in {})",
            newer_branches.join(", ")
        )),
    }
    if env.requires_approval {
        lines.push(format!(
            "approvals: required (min {}, approvers {})",
            env.min_approvals,
            env.approvers.len()
        ));
    }
    if let Some(ts) = env.rebuilt_at {
        lines.push(format!("rebuilt_at: {}", ts.format("%Y-%m-%d %H:%M UTC")));
    }
    if let Some(ts) = env.released_at {
        lines.push(format!("released_at: {}", ts.format("%Y-%m-%d %H:%M UTC")));
    }
    lines.join("\n")
}

fn build_branch_overview(context: &GlobalContext, row: &BranchRow) -> Result<String> {
    let mut lines = Vec::new();

    if !row.promoted_to.is_empty() {
        lines.push(format!("promoted_to: {}", row.promoted_to.join(", ")));
    } else {
        lines.push("promoted_to: (none)".to_string());
    }

    let compare_base = if !row.promoted_to.is_empty() {
        // Try to pick the base branch for the promoted environments for more relevant diff stats.
        let cfg = access_metadata_read_only(context, |c| Ok(c.clone())).ok();
        if let Some(cfg) = cfg {
            let mut bases = row
                .promoted_to
                .iter()
                .filter_map(|env_name| cfg.environments.get(env_name).map(|e| e.base.clone()))
                .collect::<Vec<_>>();
            bases.sort();
            bases.dedup();
            if bases.len() == 1 {
                bases[0].clone()
            } else if bases.is_empty() {
                context
                    .git()
                    .get_default_branch_ref()
                    .unwrap_or_else(|_| "main".to_string())
            } else {
                bases[0].clone()
            }
        } else {
            context
                .git()
                .get_default_branch_ref()
                .unwrap_or_else(|_| "main".to_string())
        }
    } else {
        context
            .git()
            .get_default_branch_ref()
            .unwrap_or_else(|_| "main".to_string())
    };
    lines.push(format!("compare_base: {}", compare_base));

    let branch_ref = row.git_ref();
    if let Ok((behind, ahead)) = context.git().ahead_behind(&compare_base, &branch_ref) {
        lines.push(format!("ahead/behind: +{} / -{}", ahead, behind));
    }
    if let Ok(stat) = context.git().get_diff_stat(&compare_base, &branch_ref) {
        let stat = stat.trim().to_string();
        if !stat.is_empty() {
            lines.push("diff_stat:".to_string());
            for l in stat.lines().take(8) {
                lines.push(format!("  {}", l));
            }
        }
    }
    if let Ok(c) = context.git().get_last_commit(&branch_ref) {
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

    Ok(lines.join("\n"))
}
