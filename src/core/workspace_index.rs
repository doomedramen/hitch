use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct EnvironmentMinModel {
    pub name: String,
    #[allow(dead_code)]
    pub base: String,
    #[allow(dead_code)]
    pub promoted_count: usize,
    pub locked: bool,
    pub requires_approval: bool,
    #[allow(dead_code)]
    pub min_approvals: usize,
    #[allow(dead_code)]
    pub approvers_count: usize,
}

#[derive(Debug, Clone)]
pub struct WorkspaceIndexModel {
    pub current_branch: Option<String>,
    #[allow(dead_code)]
    pub metadata_sha: Option<String>,
    pub environments: Vec<EnvironmentMinModel>,
    pub promoted_branches: Vec<crate::core::workspace::BranchRow>,
    pub branches: Vec<crate::core::workspace::BranchRow>,
}

pub fn build_workspace_index_model(context: &GlobalContext) -> Result<WorkspaceIndexModel> {
    let current_branch = context.git().get_current_branch().ok();
    let metadata_sha = context.git().get_branch_commit_sha("hitch-metadata").ok();

    let cfg = access_metadata_read_only(context, |c| Ok(c.clone()))?;
    let env_names: std::collections::HashSet<String> = cfg.environments.keys().cloned().collect();

    let mut promoted_to: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut base_for: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    let mut environments: Vec<EnvironmentMinModel> = cfg
        .environments
        .iter()
        .map(|(name, env)| {
            base_for
                .entry(env.base.clone())
                .or_default()
                .push(name.clone());
            for b in &env.branches {
                promoted_to.entry(b.clone()).or_default().push(name.clone());
            }
            EnvironmentMinModel {
                name: name.clone(),
                base: env.base.clone(),
                promoted_count: env.branches.len(),
                locked: env.is_locked(),
                requires_approval: env.requires_approval,
                min_approvals: env.min_approvals,
                approvers_count: env.approvers.len(),
            }
        })
        .collect();
    environments.sort_by(|a, b| a.name.cmp(&b.name));

    let locals = context.git().list_local_branches().unwrap_or_default();
    let remotes = context
        .git()
        .list_remote_branches("origin")
        .unwrap_or_default();
    let local_set: std::collections::HashSet<_> = locals.iter().cloned().collect();
    let remote_set: std::collections::HashSet<_> = remotes.iter().cloned().collect();

    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for b in locals {
        all.insert(b);
    }
    for b in remotes {
        all.insert(b);
    }

    let mut promoted_branches = Vec::new();
    let mut branches = Vec::new();

    for name in all {
        let local = local_set.contains(&name);
        let remote = remote_set.contains(&name);
        let is_environment = env_names.contains(&name);
        let promoted_envs = promoted_to.get(&name).cloned().unwrap_or_default();
        let base_envs = base_for.get(&name).cloned().unwrap_or_default();

        let row = crate::core::workspace::BranchRow {
            name: name.clone(),
            local,
            remote,
            is_environment,
            promoted_to: promoted_envs.clone(),
            base_for: base_envs,
        };

        if is_environment {
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

    Ok(WorkspaceIndexModel {
        current_branch,
        metadata_sha,
        environments,
        promoted_branches,
        branches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::logging::Logger;
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        // Test-only helper that simulates plain git usage against a scratch
        // repo; nulls stdin for the same reason as the shared test harness in
        // tests/test_framework/command_runners.rs.
        #[allow(clippy::disallowed_methods)]
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn workspace_index_maps_promotions_and_bases() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();

        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "Test User"]);

        // Create main branch with an initial commit.
        run_git(repo, &["checkout", "-b", "main"]);
        fs::write(repo.join("README.md"), "hello").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);

        // Feature branch.
        run_git(repo, &["checkout", "-b", "feature-a"]);
        fs::write(repo.join("feature.txt"), "a").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "feature"]);

        // Hitch metadata branch with config.
        run_git(repo, &["checkout", "-b", "hitch-metadata"]);
        let hitch = serde_json::json!({
            "version": "1.0",
            "environments": {
                "dev": {
                    "base": "main",
                    "branches": ["feature-a"],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": null,
                    "released_at": null,
                    "requires_approval": false,
                    "min_approvals": 0,
                    "approvers": []
                },
                "qa": {
                    "base": "main",
                    "branches": [],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": null,
                    "released_at": null,
                    "requires_approval": false,
                    "min_approvals": 0,
                    "approvers": []
                }
            }
        });
        fs::write(repo.join("hitch.json"), hitch.to_string()).expect("write hitch.json");
        run_git(repo, &["add", "hitch.json"]);
        run_git(repo, &["commit", "-m", "hitch config"]);

        run_git(repo, &["checkout", "main"]);

        let logger = Arc::new(Logger::for_command("test", false));
        let context =
            GlobalContext::new_at_path(repo.to_str().unwrap(), false, true, true, logger).unwrap();

        let index = build_workspace_index_model(&context).expect("index");

        assert!(index.environments.iter().any(|e| e.name == "dev"));
        assert!(index.environments.iter().any(|e| e.name == "qa"));

        let promoted = index
            .promoted_branches
            .iter()
            .find(|b| b.name == "feature-a")
            .expect("feature-a promoted");
        assert_eq!(promoted.promoted_to, vec!["dev".to_string()]);

        let main = index
            .branches
            .iter()
            .find(|b| b.name == "main")
            .expect("main exists");
        assert!(main.base_for.contains(&"dev".to_string()));
        assert!(main.base_for.contains(&"qa".to_string()));
    }
}
