//! Shared helpers for locating and inspecting the GitHub CLI (`gh`).
//!
//! Used by `hitch pr` (which shells out to `gh pr create`), `hitch doctor`
//! (which diagnoses whether that shell-out is likely to work), and
//! `hitch setup` (which manages rulesets via `gh api`).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

// ── gh binary discovery ────────────────────────────────────────────

/// Locate the `gh` binary. Returns `None` if it isn't on PATH / runnable.
pub fn find_gh() -> Option<String> {
    // `which` is neither git nor gh, but the same "never inherit a real
    // terminal's stdin" discipline applies to every subprocess this module
    // spawns — it just never has any reason to read stdin.
    #[allow(clippy::disallowed_methods)]
    let output = Command::new("which")
        .arg("gh")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }

    // `which` itself may be unavailable (unlikely, but cheap to guard); fall
    // back to just trying to run `gh --version` directly.
    // `gh` is a separate binary with its own auth flow; stdin is nulled just
    // like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
    let status = Command::new("gh")
        .arg("--version")
        .stdin(std::process::Stdio::null())
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

// ── auth status parsing ────────────────────────────────────────────

/// One authenticated account as reported by `gh auth status` (one host can
/// have more than one logged-in account).
#[derive(Debug, Default, Clone)]
pub struct GhAuthAccount {
    pub host: String,
    pub account: Option<String>,
    pub active: bool,
    /// Classic OAuth token scopes, e.g. `repo`, `workflow`. Empty for
    /// fine-grained PATs / GitHub App tokens, which don't report scopes this
    /// way — that is not itself a problem.
    pub scopes: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct GhAuthStatus {
    pub authenticated: bool,
    pub accounts: Vec<GhAuthAccount>,
    pub raw_output: String,
}

/// Run `gh auth status` and parse its output. Best-effort: `gh`'s text format
/// isn't a stable contract, so callers should treat `accounts` as a helpful
/// summary and fall back to `raw_output` when something looks unparsed.
pub fn check_auth_status(gh_path: &str) -> GhAuthStatus {
    // `gh` is a separate binary with its own auth flow; stdin is nulled just
    // like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
    let output = Command::new(gh_path)
        .arg("auth")
        .arg("status")
        .stdin(std::process::Stdio::null())
        .output();

    let (authenticated, raw_output) = match output {
        Ok(out) => {
            // gh has written this to stdout and to stderr across different
            // versions; capture both so we don't miss it either way.
            let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), combined)
        }
        Err(e) => (false, format!("Failed to run 'gh auth status': {}", e)),
    };

    let accounts = parse_auth_status(&raw_output);

    GhAuthStatus {
        authenticated,
        accounts,
        raw_output,
    }
}

fn parse_auth_status(raw: &str) -> Vec<GhAuthAccount> {
    let mut accounts = Vec::new();
    let mut current_host: Option<String> = None;
    let mut current: Option<GhAuthAccount> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Host lines are unindented and unmarked, e.g. "github.com".
        if !line.starts_with(' ') && !line.starts_with('\t') {
            if let Some(acc) = current.take() {
                accounts.push(acc);
            }
            current_host = Some(trimmed.to_string());
            continue;
        }

        if trimmed.contains("Logged in to") {
            if let Some(acc) = current.take() {
                accounts.push(acc);
            }
            let account_name = trimmed
                .rsplit("account ")
                .next()
                .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty());
            current = Some(GhAuthAccount {
                host: current_host.clone().unwrap_or_default(),
                account: account_name,
                active: false,
                scopes: Vec::new(),
            });
            continue;
        }

        if let Some(acc) = current.as_mut() {
            if let Some(rest) = trimmed.strip_prefix("- Active account:") {
                acc.active = rest.trim().eq_ignore_ascii_case("true");
            } else if let Some(rest) = trimmed.strip_prefix("- Token scopes:") {
                acc.scopes = rest
                    .split(',')
                    .map(|s| s.trim().trim_matches('\'').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }

    if let Some(acc) = current.take() {
        accounts.push(acc);
    }

    accounts
}

// ── gh api helpers ─────────────────────────────────────────────────

/// Run `gh api <endpoint>` and return the raw JSON output.
fn gh_api(endpoint: &str, flags: &[&str]) -> Result<String> {
    let gh = find_gh().context("gh CLI not found on PATH")?;
    // `gh` is a separate binary with its own auth flow; stdin is nulled
    // (below) just like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = Command::new(&gh);
    cmd.arg("api");
    cmd.arg(endpoint);
    for flag in flags {
        cmd.arg(flag);
    }
    // Never let `gh` block reading from a real inherited terminal (e.g. an
    // auth device-code prompt) — see the matching comment in
    // git_operations.rs::run_git_command.
    cmd.stdin(std::process::Stdio::null());

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run 'gh api {}'", endpoint))?;

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

/// Run `gh api` with a JSON body via stdin. Adds `-X POST` unless the
/// caller already provides a method override in flags.
fn gh_api_with_body(
    endpoint: &str,
    body: &str,
    flags: &[&str],
    default_method: &str,
) -> Result<String> {
    let gh = find_gh().context("gh CLI not found on PATH")?;
    // Piped stdin is the point here (the JSON body is written to `gh`, then
    // the pipe is closed), so this cannot use a null-stdin builder.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = Command::new(&gh);
    cmd.arg("api");
    cmd.arg(endpoint);
    cmd.arg("--input").arg("-");
    for flag in flags {
        cmd.arg(flag);
    }

    let has_method = flags.windows(2).any(|w| w[0] == "-X");
    if !has_method {
        cmd.arg("-X").arg(default_method);
    }

    use std::process::Stdio;
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn 'gh api {}'", endpoint))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(body.as_bytes())
            .context("Failed to write request body to gh api")?;
    }

    let output = child.wait_with_output()?;

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

// ── repo info ──────────────────────────────────────────────────────

/// Extract owner/repo from a git remote URL.
pub fn owner_repo_from_remote() -> Result<(String, String)> {
    // No current_dir here historically, which meant this answered for the
    // process's cwd rather than the repository; run it through GitOperations
    // so it is anchored to the repo and gets the same locale/stdin discipline
    // as every other git call.
    let git = crate::utils::git_operations::GitOperations::new()?;
    let output = git.run_git_command(&["remote", "get-url", "origin"])?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Handle HTTPS: https://github.com/owner/repo.git
    // Handle SSH:   git@github.com:owner/repo.git
    let path = if let Some(rest) = url.strip_prefix("https://github.com/") {
        rest.to_string()
    } else if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest.to_string()
    } else {
        return Err(anyhow::anyhow!(
            "Could not parse owner/repo from remote URL: {}",
            url
        ));
    };

    let path = path.trim_end_matches(".git");
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(anyhow::anyhow!(
            "Could not parse owner/repo from remote URL: {}",
            url
        ));
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}

// ── branch listing ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GhBranch {
    pub name: String,
}

/// List branches via `gh api`.
pub fn list_remote_branches(owner: &str, repo: &str) -> Result<Vec<GhBranch>> {
    let endpoint = format!("/repos/{}/{}/branches?per_page=100", owner, repo);
    let output = gh_api(&endpoint, &["--jq", ".[].name"])?;

    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|name| GhBranch { name })
        .collect())
}

// ── pull requests & comments ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GhPullRequest {
    pub number: u64,
}

/// Find the open PR whose head branch is `branch`, if any. Used to locate
/// where to post a held/re-included status comment for a promoted branch —
/// returns `None` (not an error) when there simply isn't one.
pub fn find_open_pr_for_branch(owner: &str, repo: &str, branch: &str) -> Result<Option<u64>> {
    let endpoint = format!(
        "/repos/{}/{}/pulls?head={}:{}&state=open",
        owner, repo, owner, branch
    );
    let output = gh_api(&endpoint, &[])?;
    let prs: Vec<GhPullRequest> = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse PR list: {}", output))?;
    Ok(prs.into_iter().next().map(|p| p.number))
}

#[derive(Debug, Deserialize)]
pub struct GhIssueComment {
    pub id: u64,
    pub body: String,
}

/// List a PR's comments (PRs are issues, so this is the issue-comments
/// endpoint — the same one `gh pr comment` uses).
pub fn list_pr_comments(owner: &str, repo: &str, pr_number: u64) -> Result<Vec<GhIssueComment>> {
    let endpoint = format!(
        "/repos/{}/{}/issues/{}/comments?per_page=100",
        owner, repo, pr_number
    );
    let output = gh_api(&endpoint, &[])?;
    let comments: Vec<GhIssueComment> = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse PR comments: {}", output))?;
    Ok(comments)
}

/// Create a new PR comment. Returns the new comment's id.
pub fn create_pr_comment(owner: &str, repo: &str, pr_number: u64, body: &str) -> Result<u64> {
    let endpoint = format!("/repos/{}/{}/issues/{}/comments", owner, repo, pr_number);
    let payload = serde_json::json!({ "body": body });
    let body_str = serde_json::to_string(&payload)?;
    let output = gh_api_with_body(&endpoint, &body_str, &[], "POST")?;

    let created: serde_json::Value = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse created comment: {}", output))?;
    created
        .get("id")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Created comment has no id in response: {}", output))
}

/// Update an existing PR comment's body in place.
pub fn update_pr_comment(owner: &str, repo: &str, comment_id: u64, body: &str) -> Result<()> {
    let endpoint = format!("/repos/{}/{}/issues/comments/{}", owner, repo, comment_id);
    let payload = serde_json::json!({ "body": body });
    let body_str = serde_json::to_string(&payload)?;
    gh_api_with_body(&endpoint, &body_str, &["-X", "PATCH"], "POST")?;
    Ok(())
}

// ── rulesets ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GhRuleset {
    pub id: u64,
    pub name: String,
    pub enforcement: String,
    pub bypass_actors: Option<Vec<serde_json::Value>>,
    pub rules: Option<Vec<serde_json::Value>>,
    pub conditions: Option<serde_json::Value>,
}

/// List rulesets for a repo.
pub fn list_rulesets(owner: &str, repo: &str) -> Result<Vec<GhRuleset>> {
    let endpoint = format!("/repos/{}/{}/rulesets?per_page=100", owner, repo);
    let output = gh_api(&endpoint, &[])?;
    let rulesets: Vec<GhRuleset> = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse rulesets: {}", output))?;
    Ok(rulesets)
}

/// Get a single ruleset by ID.
pub fn get_ruleset(owner: &str, repo: &str, ruleset_id: u64) -> Result<GhRuleset> {
    let endpoint = format!("/repos/{}/{}/rulesets/{}", owner, repo, ruleset_id);
    let output = gh_api(&endpoint, &[])?;
    let ruleset: GhRuleset = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse ruleset: {}", output))?;
    Ok(ruleset)
}

/// Create a ruleset with the given JSON body.
pub fn create_ruleset_raw(owner: &str, repo: &str, body: &str) -> Result<u64> {
    let endpoint = format!("/repos/{}/{}/rulesets", owner, repo);
    let output = gh_api_with_body(&endpoint, body, &[], "POST")?;

    let created: serde_json::Value = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse created ruleset: {}", output))?;

    created
        .get("id")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Created ruleset has no id in response: {}", output))
}

/// Activate a ruleset (set enforcement to "active").
pub fn activate_ruleset(owner: &str, repo: &str, ruleset_id: u64) -> Result<()> {
    let body = serde_json::json!({
        "enforcement": "active",
    });
    let body_str = serde_json::to_string_pretty(&body)?;

    let endpoint = format!("/repos/{}/{}/rulesets/{}", owner, repo, ruleset_id);
    gh_api_with_body(&endpoint, &body_str, &["-X", "PUT"], "POST")?;

    Ok(())
}

/// Delete a ruleset.
pub fn delete_ruleset(owner: &str, repo: &str, ruleset_id: u64) -> Result<()> {
    let endpoint = format!("/repos/{}/{}/rulesets/{}", owner, repo, ruleset_id);
    gh_api(&endpoint, &["-X", "DELETE"])?;
    Ok(())
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_authenticated_account() {
        let raw = "github.com\n  \u{2713} Logged in to github.com account doomedramen (keyring)\n  - Active account: true\n  - Git operations protocol: https\n  - Token: gho_****\n  - Token scopes: 'gist', 'read:org', 'repo', 'workflow'\n";
        let accounts = parse_auth_status(raw);
        assert_eq!(accounts.len(), 1);
        let acc = &accounts[0];
        assert_eq!(acc.host, "github.com");
        assert_eq!(acc.account.as_deref(), Some("doomedramen"));
        assert!(acc.active);
        assert_eq!(acc.scopes, vec!["gist", "read:org", "repo", "workflow"]);
    }

    #[test]
    fn parses_no_accounts_from_empty_output() {
        assert!(parse_auth_status("").is_empty());
    }

    #[test]
    fn parses_not_logged_in_output() {
        let raw = "You are not logged into any GitHub hosts. Run gh auth login to authenticate.\n";
        assert!(parse_auth_status(raw).is_empty());
    }

    #[test]
    fn parses_owner_repo_from_https_url() {
        let result = parse_remote_url("https://github.com/doomedramen/hitch.git");
        assert_eq!(
            result,
            Some(("doomedramen".to_string(), "hitch".to_string()))
        );
    }

    #[test]
    fn parses_owner_repo_from_ssh_url() {
        let result = parse_remote_url("git@github.com:doomedramen/hitch.git");
        assert_eq!(
            result,
            Some(("doomedramen".to_string(), "hitch".to_string()))
        );
    }

    fn parse_remote_url(url: &str) -> Option<(String, String)> {
        let path = if let Some(rest) = url.strip_prefix("https://github.com/") {
            rest.to_string()
        } else if let Some(rest) = url.strip_prefix("git@github.com:") {
            rest.to_string()
        } else {
            return None;
        };
        let path = path.trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() < 2 {
            return None;
        }
        Some((parts[0].to_string(), parts[1].to_string()))
    }
}
