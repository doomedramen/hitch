use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const HITCH_APP_CLIENT_ID: &str = "PLACEHOLDER_CLIENT_ID";
pub const HITCH_APP_BACKEND_URL: &str = "https://api.hitch.dev";
pub const HITCH_APP_INSTALL_URL: &str = "https://github.com/apps/hitch/installations/new";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupConfig {
    pub repo_url: String,
    pub installation_id: u64,
    pub setup_token: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: String,
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupTokenResponse {
    pub setup_token: String,
    pub installation_id: u64,
}

pub fn config_path(repo_url: &str) -> PathBuf {
    let hash = Sha256::digest(repo_url.as_bytes());
    let filename = format!("{:x}.json", hash);
    dirs_config().join("hitch").join(filename)
}

pub fn load_setup_config(repo_url: &str) -> Option<SetupConfig> {
    let path = config_path(repo_url);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_setup_config(config: &SetupConfig) -> Result<()> {
    let path = config_path(&config.repo_url);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create hitch config directory")?;
    }
    let content =
        serde_json::to_string_pretty(config).context("Failed to serialize setup config")?;
    std::fs::write(&path, content)
        .context("Failed to write setup config")
        .map_err(Into::into)
}

pub fn delete_setup_config(repo_url: &str) -> Result<()> {
    let path = config_path(repo_url);
    if path.exists() {
        std::fs::remove_file(&path).context("Failed to remove setup config")?;
    }
    Ok(())
}

pub fn start_device_flow() -> Result<DeviceCodeResponse> {
    let params: Vec<(&str, &str)> = vec![
        ("client_id", HITCH_APP_CLIENT_ID),
        ("scope", "repo"),
    ];
    let response: DeviceCodeResponse = ureq::post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .send_form(params)
        .context("Failed to start device flow with GitHub")?
        .into_body()
        .read_json()
        .context("Failed to parse device code response")?;

    Ok(response)
}

pub fn poll_for_token(device_code: &str) -> Result<Option<AccessTokenResponse>> {
    let params: Vec<(&str, &str)> = vec![
        ("client_id", HITCH_APP_CLIENT_ID),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ];
    let response = ureq::post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .send_form(params)
        .context("Failed to poll for access token")?;

    let body = response.into_body().read_to_string()?;

    if let Ok(error) = serde_json::from_str::<serde_json::Value>(&body) {
        if error.get("error").and_then(|e| e.as_str()) == Some("authorization_pending") {
            return Ok(None);
        }
        if error.get("error").and_then(|e| e.as_str()) == Some("slow_down") {
            return Ok(None);
        }
    }

    let token: AccessTokenResponse =
        serde_json::from_str(&body).context("Failed to parse access token response")?;
    Ok(Some(token))
}

pub fn exchange_token_for_setup(
    oauth_token: &str,
    repo_url: &str,
) -> Result<SetupConfig> {
    let resp = ureq::post(&format!("{}/setup", HITCH_APP_BACKEND_URL))
        .header("Content-Type", "application/json")
        .send_json(serde_json::json!({
            "oauth_token": oauth_token,
            "repo_url": repo_url,
        }))
        .context("Failed to contact hitch backend for setup")?;

    let setup_resp: SetupTokenResponse = resp
        .into_body()
        .read_json()
        .context("Failed to parse setup token response")?;

    Ok(SetupConfig {
        repo_url: repo_url.to_string(),
        installation_id: setup_resp.installation_id,
        setup_token: setup_resp.setup_token,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn fetch_installation_token(setup_token: &str) -> Result<String> {
    let resp = ureq::post(&format!("{}/token", HITCH_APP_BACKEND_URL))
        .header("Content-Type", "application/json")
        .send_json(serde_json::json!({
            "setup_token": setup_token,
        }))
        .context("Failed to contact hitch backend for installation token")?;

    let body: serde_json::Value = resp
        .into_body()
        .read_json()
        .context("Failed to parse installation token response")?;

    body.get("token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Backend did not return an installation token"))
}

/// Build an HTTPS push URL with the installation token embedded.
/// Converts SSH remote URLs to HTTPS so the token can be used.
pub fn build_token_url(remote_url: &str, token: &str) -> Result<String> {
    let (owner, repo) = parse_owner_repo(remote_url)?;

    Ok(format!(
        "https://x-access-token:{}@github.com/{}/{}.git",
        token, owner, repo
    ))
}

/// Derive the canonical repo URL from a remote URL for config lookup.
pub fn repo_url_from_remote(remote_url: &str) -> Result<String> {
    let (owner, repo) = parse_owner_repo(remote_url)?;
    Ok(format!("https://github.com/{}/{}", owner, repo))
}

fn parse_owner_repo(remote_url: &str) -> Result<(String, String)> {
    let path = if let Some(rest) = remote_url.strip_prefix("https://github.com/") {
        rest.trim_end_matches(".git").to_string()
    } else if let Some(rest) = remote_url.strip_prefix("git@github.com:") {
        rest.trim_end_matches(".git").to_string()
    } else {
        return Err(anyhow::anyhow!(
            "Unsupported remote URL format: {}",
            remote_url
        ));
    };

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(anyhow::anyhow!(
            "Could not parse owner/repo from: {}",
            remote_url
        ));
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn dirs_config() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_deterministic() {
        let a = config_path("https://github.com/owner/repo.git");
        let b = config_path("https://github.com/owner/repo.git");
        assert_eq!(a, b);
    }

    #[test]
    fn config_path_differs_per_repo() {
        let a = config_path("https://github.com/owner/repo-a.git");
        let b = config_path("https://github.com/owner/repo-b.git");
        assert_ne!(a, b);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let config = SetupConfig {
            repo_url: "https://github.com/test/repo.git".to_string(),
            installation_id: 42,
            setup_token: "test-token".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };
        save_setup_config(&config).unwrap();
        let loaded = load_setup_config(&config.repo_url).unwrap();
        assert_eq!(loaded.repo_url, config.repo_url);
        assert_eq!(loaded.installation_id, config.installation_id);
        assert_eq!(loaded.setup_token, config.setup_token);

        delete_setup_config(&config.repo_url).unwrap();
        assert!(load_setup_config(&config.repo_url).is_none());
    }
}
