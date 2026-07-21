use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use clap::Args;
use std::process::Command;

#[derive(Args)]
pub struct PrCommand {
    /// PR title (optional; gh will prompt if omitted)
    #[arg(long)]
    pub title: Option<String>,

    /// PR body/description (optional)
    #[arg(long)]
    pub body: Option<String>,

    /// Open as a draft pull request
    #[arg(long)]
    pub draft: bool,

    /// Target branch for the PR (defaults to the shared base of all environments
    /// this branch is promoted to)
    #[arg(long)]
    pub base: Option<String>,
}

pub fn run(args: PrCommand, context: &GlobalContext) -> Result<()> {
    let current_branch = context.git().get_current_branch()?;

    let base = if let Some(base) = &args.base {
        base.clone()
    } else {
        infer_pr_base(context, &current_branch)?
    };

    context.log_info(&format!("Pushing branch '{}' to origin...", current_branch));
    match context.git().push_branch(&current_branch) {
        Ok(()) => context.log_verbose("✓ Branch pushed"),
        Err(e) => {
            context.log_warning(&format!(
                "Failed to push '{}' to origin: {}",
                current_branch, e
            ));
        }
    }

    run_gh_pr_create(&current_branch, &base, args.title.as_deref(), args.body.as_deref(), args.draft)
}

pub(crate) fn infer_pr_base(context: &GlobalContext, branch: &str) -> Result<String> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    let mut bases: Vec<(&String, &String)> = Vec::new();
    for (env_name, env) in &config.environments {
        if env.branches.contains(&branch.to_string()) {
            bases.push((env_name, &env.base));
        }
    }

    if bases.is_empty() {
        return Err(anyhow::anyhow!(
            "Branch '{}' is not promoted to any environment.\n\
             Cannot infer the PR target base.\n\
             Either specify --base <target> or promote to an environment first:\n\
               hitch promote {} <env>",
            branch, branch
        ));
    }

    let first_base = bases[0].1;
    for (_env_name, base) in &bases[1..] {
        if *base != first_base {
            let mut base_map: std::collections::HashMap<&String, Vec<&String>> =
                std::collections::HashMap::new();
            for (n, b) in &bases {
                base_map.entry(b).or_default().push(n);
            }
            let mut details = String::new();
            for (_b, names) in &base_map {
                details.push_str(&format!(
                    "    base '{}': {}\n",
                    _b,
                    names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
            return Err(anyhow::anyhow!(
                "Branch '{}' is promoted to environments with different base branches.\n\
                 Cannot choose a PR target automatically.\n\
                 Environments and their bases:\n{}",
                branch, details
            ));
        }
    }

    Ok(first_base.clone())
}

pub(crate) fn run_gh_pr_create(
    head: &str,
    base: &str,
    title: Option<&str>,
    body: Option<&str>,
    draft: bool,
) -> Result<()> {
    let gh_path = match which_gh() {
        Some(p) => p,
        None => {
            let cmd = build_gh_command(head, base, title, body, draft);
            return Err(anyhow::anyhow!(
                "GitHub CLI ('gh') not found on PATH.\n\
                 Install it from https://cli.github.com/ or run manually:\n\
                   {}",
                cmd
            ));
        }
    };

    let mut cmd = Command::new(&gh_path);
    cmd.arg("pr").arg("create");
    cmd.arg("--head").arg(head);
    cmd.arg("--base").arg(base);

    if let Some(t) = title {
        cmd.arg("--title").arg(t);
    }
    if let Some(b) = body {
        cmd.arg("--body").arg(b);
    }
    if draft {
        cmd.arg("--draft");
    }

    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run gh: {}", e))?;

    if !status.success() {
        let manual_cmd = build_gh_command(head, base, title, body, draft);
        return Err(anyhow::anyhow!(
            "gh pr create exited with a non-zero status.\n\
             Run it manually:\n  {}",
            manual_cmd
        ));
    }

    Ok(())
}

fn build_gh_command(
    head: &str,
    base: &str,
    title: Option<&str>,
    body: Option<&str>,
    draft: bool,
) -> String {
    let mut parts: Vec<String> = vec![
        "gh pr create".to_string(),
        format!("--head {}", head),
        format!("--base {}", base),
    ];
    if let Some(t) = title {
        parts.push(format!("--title \"{}\"", t));
    }
    if let Some(b) = body {
        parts.push(format!("--body \"{}\"", b));
    }
    if draft {
        parts.push("--draft".to_string());
    }
    parts.join(" ")
}

fn which_gh() -> Option<String> {
    let output = Command::new("which")
        .arg("gh")
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }

    // Fallback: try running `gh --version` to see if it's available
    let status = Command::new("gh")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if status.success() {
        Some("gh".to_string())
    } else {
        None
    }
}
