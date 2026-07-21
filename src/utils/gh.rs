//! Shared helpers for locating and inspecting the GitHub CLI (`gh`).
//!
//! Used by both `hitch pr` (which shells out to `gh pr create`) and
//! `hitch doctor` (which diagnoses whether that shell-out is likely to work).

use std::process::Command;

/// Locate the `gh` binary. Returns `None` if it isn't on PATH / runnable.
pub fn find_gh() -> Option<String> {
    let output = Command::new("which").arg("gh").output().ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }

    // `which` itself may be unavailable (unlikely, but cheap to guard); fall
    // back to just trying to run `gh --version` directly.
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
    let output = Command::new(gh_path).arg("auth").arg("status").output();

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
        assert_eq!(
            acc.scopes,
            vec!["gist", "read:org", "repo", "workflow"]
        );
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
}
