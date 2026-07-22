use crate::commands::global_context::GlobalContext;
use crate::utils::gh;
use crate::utils::prelude::access_metadata_read_only;
use crate::utils::setup;
use anyhow::{Context, Result};
use clap::Args;
use inquire::{Confirm, MultiSelect};
use std::io::Write;

#[derive(Args)]
pub struct SetupCommand {}

pub fn run(_args: SetupCommand, context: &GlobalContext) -> Result<()> {
    context.log_info("Hitch setup — configure GitHub branch protection for PR workflow\n");

    let (owner, repo) = gh::owner_repo_from_remote()?;
    let repo_url = format!("https://github.com/{}/{}", owner, repo);
    context.log_info(&format!("Repository: {}/{}", owner, repo));

    let gh_path = gh::find_gh()
        .context("GitHub CLI ('gh') not found on PATH. Install from https://cli.github.com/")?;

    let auth_status = gh::check_auth_status(&gh_path);
    if !auth_status.authenticated {
        return Err(anyhow::anyhow!(
            "gh is not authenticated. Run 'gh auth login' first."
        ));
    }

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

    context.log_info(&format!(
        "\nProtecting: {}\n",
        selected.join(", ")
    ));

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

    let config = match setup::load_setup_config(&repo_url) {
        Some(config) => {
            context.log_info("Found existing Hitch setup. Reusing authentication.");
            config
        }
        None => authenticate_with_hitch_app(context, &repo_url)?,
    };

    let body = serde_json::json!({
        "name": "hitch-protection",
        "target": "branch",
        "enforcement": "disabled",
        "bypass_actors": [{
            "actor_id": config.installation_id,
            "actor_type": "Integration",
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
    context.log_verbose(&format!("\nRuleset to create:\n{}\n", body_str));

    context.log_info("Creating ruleset (disabled)...");
    let ruleset_id = gh::create_ruleset_raw(&owner, &repo, &body_str)?;
    context.log_success(&format!("Created ruleset (id: {})", ruleset_id));

    context.log_info("\nRuleset details:");
    context.log_info(&format!("  Name: hitch-protection"));
    context.log_info(&format!("  Branches: {}", selected.join(", ")));
    context.log_info("  Rules: update, deletion, non_fast_forward");
    context.log_info(&format!(
        "  Bypass: Hitch GitHub App (installation #{})",
        config.installation_id
    ));

    let activate = Confirm::new("Activate this ruleset? (This will block all direct pushes and PR merges to the protected branches)")
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
    context.log_success(
        "Setup complete! Protected branches can now only receive pushes via 'hitch release'.",
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

fn authenticate_with_hitch_app(
    context: &GlobalContext,
    repo_url: &str,
) -> Result<setup::SetupConfig> {
    context.log_info("\nAuthenticating with Hitch GitHub App...\n");

    let device = setup::start_device_flow()?;

    println!("  ┌─────────────────────────────────────────┐");
    println!("  │  Open this URL in your browser:          │");
    println!(
        "  │  {:<43}│",
        device.verification_uri
    );
    println!("  │                                         │");
    println!("  │  Then enter this code:                   │");
    println!(
        "  │    > {} <{:<35}│",
        device.user_code,
        ""
    );
    println!("  └─────────────────────────────────────────┘");
    println!();

    context.log_info("Waiting for authorization...");

    let interval = device.interval.max(1);
    let mut token = None;

    for attempt in 0..60 {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        match setup::poll_for_token(&device.device_code)? {
            Some(t) => {
                token = Some(t);
                break;
            }
            None => {
                if attempt % 5 == 4 {
                    let _ = std::io::stderr()
                        .write_all(format!("  Still waiting (attempt {})...\r", attempt + 1).as_bytes());
                }
            }
        }
    }

    let token = token.ok_or_else(|| {
        anyhow::anyhow!("Timed out waiting for authorization. Try 'hitch setup' again.")
    })?;

    context.log_success("Authorization received!");
    context.log_info("Exchanging token for setup configuration...");

    setup::exchange_token_for_setup(&token.access_token, repo_url)
}
