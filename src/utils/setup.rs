use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

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
        std::fs::create_dir_all(parent)
            .context("Failed to create .ssh directory")?;
    }

    let status = Command::new("ssh-keygen")
        .args([
            "-t", "ed25519",
            "-C", &comment,
            "-f", &path.to_string_lossy(),
            "-N", "",
            "-q",
        ])
        .status()
        .context("Failed to run ssh-keygen. Is ssh-keygen installed?")?;

    if !status.success() {
        return Err(anyhow::anyhow!("ssh-keygen failed to generate key"));
    }

    Ok(())
}

pub fn add_deploy_key_to_repo(
    owner: &str,
    repo: &str,
    gh_path: &str,
) -> Result<()> {
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
            "--input", "-",
            "-X", "POST",
            "-H", "Accept: application/vnd.github+json",
        ])
        .arg("--silent")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run gh api to add deploy key")?;

    // Write body to stdin
    if let Some(mut stdin) = output.stdin.as_ref() {
        use std::io::Write;
        stdin
            .write_all(body_str.as_bytes())
            .context("Failed to write deploy key body")?;
    } else {
        // Re-spawn with explicit stdin
        drop(output);
        let status = Command::new(gh_path)
            .args([
                "api",
                &format!("/repos/{}/{}/keys", owner, repo),
                "-f", &format!("title={}", title),
                "-f", &format!("key={}", pubkey),
                "-f", "read_only=false",
                "--silent",
            ])
            .status()
            .context("Failed to run gh api to add deploy key")?;

        if !status.success() {
            return Err(anyhow::anyhow!("Failed to add deploy key to repository"));
        }
        return Ok(());
    }

    let result = output.wait_with_output()
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

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
