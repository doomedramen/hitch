use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::utils::file_lock::FileLock;

#[derive(Serialize, Deserialize)]
struct LockInfo {
    pid: u32,
    env_name: String,
    started_at: String,
}

/// RAII guard that holds a per-environment rebuild lock.
///
/// The lock is an OS advisory lock (`flock`) on `.git/hitch-rebuild-<env>.lock`.
/// Acquisition is atomic and the kernel releases it automatically when the
/// holding process exits for any reason, so there is no stale-lock cleanup or
/// PID-liveness heuristic to get wrong. If another live process is already
/// rebuilding the same environment, `acquire` returns an error immediately.
pub struct RebuildLock {
    _inner: FileLock,
}

impl RebuildLock {
    /// Try to acquire the rebuild lock for `env_name`.
    pub fn acquire(git_dir: &Path, env_name: &str) -> Result<Self> {
        let path = git_dir.join(format!("hitch-rebuild-{}.lock", env_name));

        let info = serde_json::to_string(&LockInfo {
            pid: std::process::id(),
            env_name: env_name.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        })?;

        let env = env_name.to_string();
        let lock_path = path.clone();
        let inner =
            FileLock::try_acquire(&path, &info, move |content| {
                match serde_json::from_str::<LockInfo>(content) {
                    Ok(info) => format!(
                        "Rebuild of environment '{}' is already in progress (PID {}, started {}). \
                     Wait for it to finish.",
                        env, info.pid, info.started_at
                    ),
                    Err(_) => format!(
                        "Rebuild of environment '{}' is already in progress (lock held on '{}'). \
                     Wait for it to finish.",
                        env,
                        lock_path.display()
                    ),
                }
            })?;

        Ok(Self { _inner: inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_blocked_until_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path();

        let first = RebuildLock::acquire(git_dir, "dev").expect("first acquire should succeed");
        let second = RebuildLock::acquire(git_dir, "dev");
        assert!(
            second.is_err(),
            "second acquire should be blocked while the lock is held"
        );

        drop(first);
        let third = RebuildLock::acquire(git_dir, "dev");
        assert!(third.is_ok(), "acquire should succeed after release");
    }

    #[test]
    fn different_environments_do_not_block_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path();

        let dev = RebuildLock::acquire(git_dir, "dev").expect("dev acquire should succeed");
        let qa = RebuildLock::acquire(git_dir, "qa");
        assert!(
            qa.is_ok(),
            "locks for different environments must be independent"
        );
        drop(dev);
    }
}
