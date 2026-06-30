use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct LockInfo {
    pid: u32,
    env_name: String,
    started_at: String,
}

/// RAII guard that holds a per-environment rebuild lock.
///
/// The lock file lives at `.git/hitch-rebuild-<env>.lock`. It is created on
/// `acquire` and deleted on `Drop` (whether the rebuild succeeds or fails).
///
/// If a lock file already exists **and** the PID recorded in it is still
/// alive, `acquire` returns an error so concurrent rebuilds are blocked
/// immediately. If the recorded process is gone the stale lock is silently
/// overwritten.
pub struct RebuildLock {
    path: PathBuf,
}

impl RebuildLock {
    /// Try to acquire the rebuild lock for `env_name`.
    pub fn acquire(git_dir: &Path, env_name: &str) -> Result<Self> {
        let path = git_dir.join(format!("hitch-rebuild-{}.lock", env_name));

        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            if let Ok(info) = serde_json::from_str::<LockInfo>(&content) {
                if is_process_alive(info.pid) {
                    return Err(anyhow::anyhow!(
                        "Rebuild of environment '{}' is already in progress \
                         (PID {}, started {}). Wait for it to finish, or delete \
                         '{}' if the process is no longer running.",
                        env_name,
                        info.pid,
                        info.started_at,
                        path.display()
                    ));
                }
                // Stale lock (process is gone) — overwrite silently.
            }
        }

        let info = LockInfo {
            pid: std::process::id(),
            env_name: env_name.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        std::fs::write(&path, serde_json::to_string(&info)?)?;

        Ok(Self { path })
    }
}

impl Drop for RebuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Returns `true` if the process with the given PID is currently running.
///
/// Uses `kill -0 <pid>` which sends no signal but succeeds if the process
/// exists. Works on macOS and Linux.
pub(crate) fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
