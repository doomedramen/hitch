use crate::commands::global_context::GlobalContext;
use crate::core::status::{build_status_model, EnvironmentStatusModel, RebuildState, StatusModel};
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct BranchRow {
    pub name: String,
    pub local: bool,
    pub remote: bool,
    pub is_environment: bool,
    pub promoted_to: Vec<String>,
    pub base_for: Vec<String>,
}

impl BranchRow {
    /// The branch name Hitch commands expect (without any `origin/` prefix).
    pub fn cli_ref(&self) -> &str {
        &self.name
    }

    /// A git reference usable in `git log` / `git diff` when the branch may be remote-only.
    pub fn git_ref(&self) -> String {
        if self.local {
            self.name.clone()
        } else if self.remote {
            format!("origin/{}", self.name)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkspaceModel {
    pub status: StatusModel,
    pub environments: Vec<EnvironmentStatusModel>,
    pub promoted_branches: Vec<BranchRow>,
    pub branches: Vec<BranchRow>,
}

#[allow(dead_code)]
pub fn build_workspace_model(
    context: &GlobalContext,
    filter: Option<&str>,
) -> Result<WorkspaceModel> {
    let status = build_status_model(context)?;
    let mut environments = status.environments.clone();

    // Derive sets from config without printing.
    let cfg = access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let env_names: std::collections::HashSet<String> = cfg.environments.keys().cloned().collect();

    let mut promoted_to: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut base_for: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for (env, e) in &cfg.environments {
        base_for
            .entry(e.base.clone())
            .or_default()
            .push(env.clone());
        for b in &e.branches {
            promoted_to.entry(b.clone()).or_default().push(env.clone());
        }
    }

    let locals = context.git().list_local_branches().unwrap_or_default();
    let remotes = context
        .git()
        .list_remote_branches("origin")
        .unwrap_or_default();
    let local_set: std::collections::HashSet<_> = locals.iter().cloned().collect();
    let remote_set: std::collections::HashSet<_> = remotes.iter().cloned().collect();

    // Build a unified branch name list without the origin/ prefix.
    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for b in locals {
        all.insert(b);
    }
    for b in remotes {
        all.insert(b);
    }

    let matches = |name: &str| -> bool {
        if let Some(f) = filter {
            if f.trim().is_empty() {
                return true;
            }
            name.to_lowercase().contains(&f.to_lowercase())
        } else {
            true
        }
    };

    let mut promoted_branches = Vec::new();
    let mut branches = Vec::new();

    for name in all {
        if !matches(&name) && !env_names.contains(&name) {
            // Still allow env names to match via env list; branch-only filtering is fine.
            continue;
        }

        let local = local_set.contains(&name);
        let remote = remote_set.contains(&name);
        let is_environment = env_names.contains(&name);
        let promoted_envs = promoted_to.get(&name).cloned().unwrap_or_default();
        let base_envs = base_for.get(&name).cloned().unwrap_or_default();

        let row = BranchRow {
            name: name.clone(),
            local,
            remote,
            is_environment,
            promoted_to: promoted_envs.clone(),
            base_for: base_envs,
        };

        if is_environment {
            // Environments are shown in their own section; don't duplicate here.
            continue;
        }

        if !promoted_envs.is_empty() {
            promoted_branches.push(row);
        } else {
            branches.push(row);
        }
    }

    promoted_branches.sort_by(|a, b| a.name.cmp(&b.name));
    branches.sort_by(|a, b| a.name.cmp(&b.name));

    // Filter environments after we have the list.
    if let Some(f) = filter {
        if !f.trim().is_empty() {
            let f = f.to_lowercase();
            environments.retain(|e| e.name.to_lowercase().contains(&f));
        }
    }

    Ok(WorkspaceModel {
        environments,
        promoted_branches,
        branches,
        status,
    })
}

#[allow(dead_code)]
pub fn env_rebuild_badge(env: &EnvironmentStatusModel) -> Option<&'static str> {
    match env.rebuild_state {
        RebuildState::UpToDate => None,
        RebuildState::NeverRebuilt => Some("NEVER"),
        RebuildState::NeedsRebuild { .. } => Some("REBUILD"),
    }
}
