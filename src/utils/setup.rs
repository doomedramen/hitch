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
        .status()
        .context("Failed to run ssh-keygen. Is ssh-keygen installed?")?;

    if !status.success() {
        return Err(anyhow::anyhow!("ssh-keygen failed to generate key"));
    }

    Ok(())
}

pub fn add_deploy_key_to_repo(owner: &str, repo: &str, gh_path: &str) -> Result<()> {
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

    let output = Command::new(gh_path)
        .args([
            "api",
            &format!("/repos/{}/{}/keys", owner, repo),
            "--input",
            "-",
            "-X",
            "POST",
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .arg("--silent")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run gh api to add deploy key")?;

    if let Some(mut stdin) = output.stdin.as_ref() {
        use std::io::Write;
        stdin
            .write_all(body_str.as_bytes())
            .context("Failed to write deploy key body")?;
    } else {
        drop(output);
        let status = Command::new(gh_path)
            .args([
                "api",
                &format!("/repos/{}/{}/keys", owner, repo),
                "-f",
                &format!("title={}", title),
                "-f",
                &format!("key={}", pubkey),
                "-f",
                "read_only=false",
                "--silent",
            ])
            .status()
            .context("Failed to run gh api to add deploy key")?;

        if !status.success() {
            return Err(anyhow::anyhow!("Failed to add deploy key to repository"));
        }
        return Ok(());
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

    Ok(())
}

pub fn configure_git_ssh(owner: &str, repo: &str) -> Result<()> {
    let path = key_path(owner, repo);
    let ssh_command = format!("ssh -i {} -o IdentitiesOnly=yes", path.display());

    Command::new("git")
        .args(["config", "--local", "core.sshCommand", &ssh_command])
        .status()
        .context("Failed to configure git sshCommand")?;

    Ok(())
}

// ── protection cache ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionCache {
    pub repo_url: String,
    pub ruleset_id: u64,
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
) -> Result<()> {
    let path = protection_cache_path(owner, repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create hitch config directory")?;
    }

    let cache = ProtectionCache {
        repo_url: format!("https://github.com/{}/{}", owner, repo),
        ruleset_id,
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
