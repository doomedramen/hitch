use crate::commands::global_context::GlobalContext;
use crate::utils::gh;
use anyhow::Result;
use clap::Args;

/// Scopes `hitch pr` needs from a classic OAuth token (it shells out to
/// `gh pr create`, which reads/writes the repository).
const RECOMMENDED_SCOPES: &[&str] = &["repo"];

#[derive(Args)]
pub struct DoctorCommand {}

/// Diagnose whether the environment can support `hitch pr` (i.e. whether
/// `gh` is installed, authenticated, and holds the scopes that command needs).
pub fn run(_args: DoctorCommand, context: &GlobalContext) -> Result<()> {
    context.log_info("Checking GitHub CLI ('gh') setup for 'hitch pr'...");

    let mut problems = 0usize;

    let gh_path = match gh::find_gh() {
        Some(path) => {
            context.log_success(&format!("gh found on PATH ({})", path));
            path
        }
        None => {
            context.log_error(
                "gh not found on PATH. Install it from https://cli.github.com/ — \
                 'hitch pr' shells out to it to open pull requests.",
            );
            return summarize(context, problems + 1);
        }
    };

    let status = gh::check_auth_status(&gh_path);

    if !status.authenticated || status.accounts.is_empty() {
        context.log_error(
            "gh is not authenticated to any GitHub host. Run 'gh auth login' — \
             'hitch pr' will fail without it.",
        );
        if !status.raw_output.trim().is_empty() {
            context.log_info(&format!(
                "Raw output from 'gh auth status':\n{}",
                indent(&status.raw_output)
            ));
        }
        return summarize(context, problems + 1);
    }

    for account in &status.accounts {
        let who = account.account.as_deref().unwrap_or("unknown account");
        let active_note = if account.active { " (active)" } else { "" };
        context.log_success(&format!(
            "Authenticated to {} as {}{}",
            account.host, who, active_note
        ));

        if account.scopes.is_empty() {
            context.log_info(&format!(
                "  No classic token scopes reported for {} — likely a fine-grained \
                 token or GitHub App auth, which doesn't expose scopes this way. \
                 Skipping scope checks.",
                account.host
            ));
            continue;
        }

        context.log_info(&format!("  Scopes: {}", account.scopes.join(", ")));

        let missing: Vec<&str> = RECOMMENDED_SCOPES
            .iter()
            .filter(|needed| !account.scopes.iter().any(|s| s == *needed))
            .copied()
            .collect();

        if !missing.is_empty() {
            problems += 1;
            context.log_warning(&format!(
                "  Missing recommended scope(s) for {}: {} — 'hitch pr' may fail. \
                 Run: gh auth refresh -h {} -s {}",
                account.host,
                missing.join(", "),
                account.host,
                missing.join(",")
            ));
        }
    }

    summarize(context, problems)
}

fn summarize(context: &GlobalContext, problems: usize) -> Result<()> {
    if problems == 0 {
        context.log_success("All checks passed — 'hitch pr' should work.");
    } else {
        context.log_warning(&format!(
            "{} issue(s) found above; 'hitch pr' may not work until they're resolved.",
            problems
        ));
    }
    Ok(())
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}
