use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::utils::rebuild_lock::is_process_alive;

#[derive(Serialize, Deserialize)]
struct RepoLockInfo {
    pid: u32,
    command: String,
    started_at: String,
}

/// RAII guard that serializes mutating Hitch operations within a single
/// repository (working tree / `.git` directory).
///
/// Mutating commands switch branches and rewrite the shared `hitch-metadata`
/// branch in the working directory, so two of them running at the same time in
/// the same repository would clobber each other (and a user's working tree).
/// This lock makes such operations mutually exclusive across processes — for
/// example two `hitch` invocations in different terminals, or a `hitch`
/// invocation racing the desktop app on the same repo.
///
/// The lock file lives at `.git/hitch-repo.lock`. It is created on `acquire` and
/// removed on `Drop` (whether the command succeeds or fails). If a still-running
/// process already holds it, `acquire` returns an error; a stale lock left by a
/// process that has since exited is silently overwritten.
pub struct RepoLock {
    path: PathBuf,
}

impl RepoLock {
    /// Try to acquire the repository-wide lock. `command` is the name of the
    /// command requesting the lock and is only used for the diagnostic message
    /// shown to whoever is blocked.
    pub fn acquire(git_dir: &Path, command: &str) -> Result<Self> {
        let path = git_dir.join("hitch-repo.lock");

        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            if let Ok(info) = serde_json::from_str::<RepoLockInfo>(&content) {
                if is_process_alive(info.pid) {
                    return Err(anyhow::anyhow!(
                        "Another Hitch operation is already in progress in this repository \
                         (command '{}', PID {}, started {}).\n\
                         Wait for it to finish, or delete '{}' if that process is no longer running.",
                        info.command,
                        info.pid,
                        info.started_at,
                        path.display()
                    ));
                }
                // Stale lock (the recorded process is gone) — overwrite silently.
            }
        }

        let info = RepoLockInfo {
            pid: std::process::id(),
            command: command.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        std::fs::write(&path, serde_json::to_string(&info)?)?;

        Ok(Self { path })
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_blocked_until_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path();

        let first = RepoLock::acquire(git_dir, "promote").expect("first acquire should succeed");

        // A second acquire while the first is still held (same live PID) must fail.
        let second = RepoLock::acquire(git_dir, "rebuild");
        assert!(
            second.is_err(),
            "second acquire should be blocked while the lock is held"
        );

        // After the first guard is dropped, the lock file is removed and a new
        // acquire succeeds.
        drop(first);
        assert!(
            !git_dir.join("hitch-repo.lock").exists(),
            "lock file should be removed on drop"
        );
        let third = RepoLock::acquire(git_dir, "release");
        assert!(third.is_ok(), "acquire should succeed after release");
    }

    #[test]
    fn stale_lock_from_dead_process_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path();
        let path = git_dir.join("hitch-repo.lock");

        // Write a lock file referencing a PID that is essentially certain to be
        // dead (0 is not a normal user process and `kill -0 0` does not report it
        // as a live, signalable process for our purposes).
        let stale =
            r#"{"pid":999999990,"command":"promote","started_at":"2020-01-01T00:00:00+00:00"}"#;
        std::fs::write(&path, stale).unwrap();

        let lock = RepoLock::acquire(git_dir, "promote");
        assert!(
            lock.is_ok(),
            "a stale lock from a dead process should be overwritten"
        );
    }
}
