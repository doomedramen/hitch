use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

// ── deploy key ─────────────────────────────────────────────────────

pub fn key_path(owner: &str, repo: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".ssh")
        .join(format!("hitch_{}_{}", owner, repo))
}

pub fn is_setup(owner: &str, repo: &str) -> bool {
    key_path(owner, repo).exists()
}

pub fn generate_deploy_key(owner: &str, repo: &str) -> Result<()> {
    let path = key_path(owner, repo);
    let comment = format!("hitch@{}:{}", owner, repo);

    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create .ssh directory")?;
    }

    // ssh-keygen is neither git nor gh, but the same "never inherit a real
    // terminal's stdin" discipline applies — `-N ""` and the existence check
    // above already avoid every interactive prompt this could hit.
    #[allow(clippy::disallowed_methods)]
    let status = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-C",
            &comment,
            "-f",
            &path.to_string_lossy(),
            "-N",
            "",
            "-q",
        ])
        .stdin(std::process::Stdio::null())
        .status()
        .context("Failed to run ssh-keygen. Is ssh-keygen installed?")?;

    if !status.success() {
        return Err(anyhow::anyhow!("ssh-keygen failed to generate key"));
    }

    Ok(())
}

pub fn add_deploy_key_to_repo(owner: &str, repo: &str, gh_path: &str) -> Result<u64> {
    let path = key_path(owner, repo);
    let pubkey_path = format!("{}.pub", path.display());
    let pubkey = std::fs::read_to_string(&pubkey_path)
        .context("Failed to read public key")?
        .trim()
        .to_string();

    let title = format!("hitch-{}", whoami());

    let body = serde_json::json!({
        "title": title,
        "key": pubkey,
        "read_only": false,
    });
    let body_str = serde_json::to_string(&body)?;
    let endpoint = format!("/repos/{}/{}/keys", owner, repo);

    // Piped stdin is the point here (the JSON body is written to `gh`, then
    // the pipe is closed), so this cannot use a null-stdin builder.
    #[allow(clippy::disallowed_methods)]
    let output = Command::new(gh_path)
        .args([
            "api",
            &endpoint,
            "--input",
            "-",
            "-X",
            "POST",
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run gh api to add deploy key")?;

    let write_ok = {
        if let Some(mut stdin) = output.stdin.as_ref() {
            use std::io::Write;
            stdin
                .write_all(body_str.as_bytes())
                .context("Failed to write deploy key body")?;
            true
        } else {
            false
        }
    };

    if !write_ok {
        drop(output);
        // `gh` is a separate binary with its own auth flow; stdin is nulled
        // (below) just like git so it can never block on a terminal.
        #[allow(clippy::disallowed_methods)]
        let fallback = Command::new(gh_path)
            .args([
                "api",
                &endpoint,
                "-f",
                &format!("title={}", title),
                "-f",
                &format!("key={}", pubkey),
                "-f",
                "read_only=false",
                "--jq",
                ".id",
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .context("Failed to run gh api fallback")?;

        if !fallback.status.success() {
            let stderr = String::from_utf8_lossy(&fallback.stderr);
            return Err(anyhow::anyhow!(
                "Failed to add deploy key to repository: {}",
                stderr.trim()
            ));
        }
        let stdout = String::from_utf8_lossy(&fallback.stdout).trim().to_string();
        return stdout
            .parse()
            .with_context(|| format!("Failed to parse key ID from: {}", stdout));
    }

    let result = output
        .wait_with_output()
        .context("Failed to wait for gh api")?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        return Err(anyhow::anyhow!(
            "Failed to add deploy key to repository: {} {}",
            stderr.trim(),
            stdout.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    let created: serde_json::Value = serde_json::from_str(&stdout)
        .with_context(|| format!("Failed to parse deploy key response: {}", stdout))?;
    created
        .get("id")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow::anyhow!("No id in deploy key response: {}", stdout))
}

/// Look up an existing deploy key's numeric ID by matching title, then
/// falling back to matching the public key on disk. Returns `None` if the
/// key cannot be found (not yet uploaded, API error, etc.).
pub fn lookup_deploy_key_id(owner: &str, repo: &str, gh_path: &str) -> Option<u64> {
    let title = format!("hitch-{}", whoami());
    let pubkey = {
        let path = key_path(owner, repo);
        let pubkey_path = format!("{}.pub", path.display());
        let raw = std::fs::read_to_string(&pubkey_path)
            .ok()?
            .trim()
            .to_string();
        // Trim trailing comment — the API returns just "<type> <base64>"
        // while the local file includes " <comment>" at the end.
        raw.split(' ').take(2).collect::<Vec<_>>().join(" ")
    };

    let endpoint = format!("/repos/{}/{}/keys", owner, repo);
    // Do NOT use --silent here: on some gh versions it suppresses the
    // JSON response for GET requests, not just the progress indicator.
    // `gh` is a separate binary with its own auth flow; stdin is nulled
    // (below) just like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
    let output = Command::new(gh_path)
        .args([
            "api",
            &endpoint,
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!(
                "hitch: failed to list deploy keys (gh api error): {}",
                stderr.trim()
            );
        }
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let keys: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;

    // Match by title first (fast path).
    for key in &keys {
        if key.get("title").and_then(|t| t.as_str()) == Some(&title) {
            return key.get("id").and_then(|id| id.as_u64());
        }
    }

    // Fall back to matching by public key (without comment, both sides).
    for key in &keys {
        if let Some(k) = key.get("key").and_then(|k| k.as_str()) {
            let api_key = k.split(' ').take(2).collect::<Vec<_>>().join(" ");
            if api_key == pubkey {
                return key.get("id").and_then(|id| id.as_u64());
            }
        }
    }

    // Last resort: let gh + jq do the matching so Rust JSON parsing
    // quirks and field name differences can't cause a miss.
    let jq = format!(
        ".[] | select(.title == \"{}\") | .id",
        title.replace('"', "\\\"")
    );
    // `gh` is a separate binary with its own auth flow; stdin is nulled
    // (below) just like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
    let output = Command::new(gh_path)
        .args([
            "api",
            &endpoint,
            "--jq",
            &jq,
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        if let Ok(id) = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
        {
            return Some(id);
        }
    }

    None
}

pub fn configure_git_ssh(owner: &str, repo: &str) -> Result<()> {
    let path = key_path(owner, repo);
    let ssh_command = format!("ssh -i {} -o IdentitiesOnly=yes", path.display());

    // Route through GitOperations rather than a raw Command::new("git") so
    // this is anchored to the repo (not the process's cwd) and gets the same
    // locale/hardening/stdin discipline as every other git call.
    let git = crate::utils::git_operations::GitOperations::new()?;
    git.run_git_command(&["config", "--local", "core.sshCommand", &ssh_command])
        .context("Failed to configure git sshCommand")?;

    Ok(())
}

// ── protection cache ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionCache {
    pub repo_url: String,
    pub ruleset_id: u64,
    pub deploy_key_id: Option<u64>,
    pub protected_branches: Vec<String>,
    pub updated_at: String,
}

fn protection_cache_path(owner: &str, repo: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".config")
        .join("hitch")
        .join(format!("{}_{}_protection.json", owner, repo))
}

pub fn save_protection_cache(
    owner: &str,
    repo: &str,
    branches: &[String],
    ruleset_id: u64,
    deploy_key_id: Option<u64>,
) -> Result<()> {
    let path = protection_cache_path(owner, repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create hitch config directory")?;
    }

    let cache = ProtectionCache {
        repo_url: format!("https://github.com/{}/{}", owner, repo),
        ruleset_id,
        deploy_key_id,
        protected_branches: branches.to_vec(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string_pretty(&cache)?;
    std::fs::write(&path, json).context("Failed to write protection cache")?;

    Ok(())
}

pub fn load_protection_cache(owner: &str, repo: &str) -> Option<ProtectionCache> {
    let path = protection_cache_path(owner, repo);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn get_unprotected_branches(
    owner: &str,
    repo: &str,
    current_envs: &[String],
    current_bases: &[String],
) -> Vec<String> {
    let cache = match load_protection_cache(owner, repo) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut unprotected: Vec<String> = Vec::new();

    for branch in current_envs {
        if !cache.protected_branches.contains(branch) {
            unprotected.push(branch.clone());
        }
    }

    for base in current_bases {
        if !cache.protected_branches.contains(base) {
            unprotected.push(base.clone());
        }
    }

    unprotected.sort_unstable();
    unprotected.dedup();
    unprotected
}

// ── helpers ────────────────────────────────────────────────────────

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
