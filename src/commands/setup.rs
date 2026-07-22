use crate::commands::global_context::GlobalContext;
use crate::utils::gh;
use crate::utils::prelude::access_metadata_read_only;
use crate::utils::setup;
use anyhow::{Context, Result};
use clap::Args;
use inquire::{Confirm, MultiSelect};

#[derive(Args)]
pub struct SetupCommand {}

pub fn run(_args: SetupCommand, context: &GlobalContext) -> Result<()> {
    context.log_info("Hitch setup — configure GitHub branch protection for PR workflow\n");

    let (owner, repo) = gh::owner_repo_from_remote()?;
    context.log_info(&format!("Repository: {}/{}", owner, repo));

    let gh_path = gh::find_gh()
        .context("GitHub CLI ('gh') not found on PATH. Install from https://cli.github.com/")?;

    let auth_status = gh::check_auth_status(&gh_path);
    if !auth_status.authenticated {
        return Err(anyhow::anyhow!(
            "gh is not authenticated. Run 'gh auth login' first."
        ));
    }

    // ── Branch selection ────────────────────────────────────────────

    let (branches, pre_selected) = discover_branches(context, &owner, &repo)?;

    if branches.is_empty() {
        context.log_warning("No branches found in the repository.");
        return Ok(());
    }

    let selected = MultiSelect::new(
        "Select branches to protect from PR merges (hitch release will still work):",
        branches.clone(),
    )
    .with_default(&pre_selected)
    .prompt()
    .context("Branch selection cancelled")?;

    if selected.is_empty() {
        context.log_info("No branches selected — nothing to configure.");
        return Ok(());
    }

    context.log_info(&format!("\nProtecting: {}\n", selected.join(", ")));

    // ── Deploy key setup ────────────────────────────────────────────

    let key_path = setup::key_path(&owner, &repo);

    if setup::is_setup(&owner, &repo) {
        context.log_info(&format!(
            "Found existing deploy key at {}",
            key_path.display()
        ));
    } else {
        context.log_info("Generating deploy key for bot-authenticated pushes...");
        setup::generate_deploy_key(&owner, &repo)?;
        context.log_success(&format!("Generated SSH key at {}", key_path.display()));

        context.log_info("Adding deploy key to repository...");
        setup::add_deploy_key_to_repo(&owner, &repo, &gh_path)?;
        context.log_success("Deploy key added to repository.");

        context.log_info("Configuring git to use deploy key...");
        setup::configure_git_ssh(&owner, &repo)?;
        context.log_success("Git configured to use deploy key for this repository.");
    }

    // ── Ruleset ─────────────────────────────────────────────────────

    let existing = gh::list_rulesets(&owner, &repo).unwrap_or_default();
    let hitch_ruleset = existing.iter().find(|r| r.name == "hitch-protection");

    if let Some(rs) = hitch_ruleset {
        context.log_info(&format!(
            "Found existing hitch-protection ruleset (id: {}, enforcement: {}).",
            rs.id, rs.enforcement
        ));
        let update = Confirm::new("Update the existing ruleset?")
            .with_default(false)
            .prompt()
            .context("Confirmation cancelled")?;
        if !update {
            context.log_info("Keeping existing ruleset. Done.");
            return Ok(());
        }
    }

    let body = serde_json::json!({
        "name": "hitch-protection",
        "target": "branch",
        "enforcement": "disabled",
        "bypass_actors": [{
            "actor_type": "DeployKey",
            "bypass_mode": "always"
        }],
        "conditions": {
            "ref_name": {
                "include": selected.iter().map(|b| format!("refs/heads/{}", b)).collect::<Vec<_>>(),
                "exclude": []
            }
        },
        "rules": [
            {"type": "update"},
            {"type": "deletion"},
            {"type": "non_fast_forward"}
        ]
    });

    let body_str = serde_json::to_string_pretty(&body)?;
    context.log_verbose(&format!("\nRuleset:\n{}\n", body_str));

    context.log_info("Creating ruleset (disabled)...");

    let ruleset_id = if let Some(rs) = hitch_ruleset {
        // Update existing ruleset
        let endpoint = format!("/repos/{}/{}/rulesets/{}", owner, repo, rs.id);
        let _ = gh_put(&gh_path, &endpoint, &body_str)?;
        context.log_success(&format!("Updated ruleset (id: {})", rs.id));
        rs.id
    } else {
        gh::create_ruleset_raw(&owner, &repo, &body_str)?
    };

    if hitch_ruleset.is_none() {
        context.log_success(&format!("Created ruleset (id: {})", ruleset_id));
    }

    context.log_info("\nRuleset details:");
    context.log_info("  Name: hitch-protection");
    context.log_info(&format!("  Branches: {}", selected.join(", ")));
    context.log_info("  Rules: update, deletion, non_fast_forward");
    context.log_info("  Bypass: All deploy keys");

    let activate = Confirm::new(
        "Activate this ruleset? (This will block all direct pushes and PR merges to the protected branches)"
    )
    .with_default(false)
    .prompt()
    .context("Confirmation cancelled")?;

    if !activate {
        context.log_info(&format!(
            "Ruleset created but left disabled. Activate it with:\n  gh ruleset enable {} --repo {}/{}",
            ruleset_id, owner, repo
        ));
        return Ok(());
    }

    gh::activate_ruleset(&owner, &repo, ruleset_id)?;
    context.log_success("Ruleset activated.\n");

    context.log_info(&format!("SSH key: {}", key_path.display()));
    context.log_success(
        "Setup complete! Protected branches can now only receive pushes via 'hitch release' (which uses the deploy key).",
    );

    Ok(())
}

fn discover_branches(
    context: &GlobalContext,
    owner: &str,
    repo: &str,
) -> Result<(Vec<String>, Vec<usize>)> {
    let config = access_metadata_read_only(context, |c| Ok(c.clone()))?;

    let mut base_branches: Vec<String> = config
        .environments
        .values()
        .map(|env| env.base.clone())
        .collect();
    base_branches.sort_unstable();
    base_branches.dedup();

    let all_branches = gh::list_remote_branches(owner, repo).unwrap_or_default();

    let mut ordered: Vec<String> = Vec::new();
    let mut pre_selected: Vec<usize> = Vec::new();

    for base in &base_branches {
        if all_branches.iter().any(|b| b.name == *base) && !ordered.contains(base) {
            pre_selected.push(ordered.len());
            ordered.push(base.clone());
        }
    }

    for branch in &all_branches {
        if !ordered.contains(&branch.name) {
            ordered.push(branch.name.clone());
        }
    }

    Ok((ordered, pre_selected))
}

fn gh_put(gh_path: &str, endpoint: &str, body: &str) -> Result<String> {
    let mut child = std::process::Command::new(gh_path)
        .args(["api", endpoint, "--input", "-", "-X", "PUT"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run gh api PATCH")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(body.as_bytes())
            .context("Failed to write body to gh api")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for gh api")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "gh api {} failed: {}",
            endpoint,
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
