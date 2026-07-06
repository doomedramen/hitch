use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::utils::file_lock::FileLock;

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
/// The lock is an OS advisory lock (`flock`) on `.git/hitch-repo.lock`. Because
/// it lives on an open file descriptor, acquisition is atomic and the kernel
/// releases it automatically if the process dies for any reason (including
/// Ctrl-C and SIGKILL) — there is no stale-lock cleanup to get wrong. If another
/// live process holds it, `acquire` returns an error immediately.
pub struct RepoLock {
    _inner: FileLock,
}

impl RepoLock {
    /// Try to acquire the repository-wide lock. `command` is the name of the
    /// command requesting the lock and is only used for the diagnostic message
    /// shown to whoever is blocked.
    pub fn acquire(git_dir: &Path, command: &str) -> Result<Self> {
        let path = git_dir.join("hitch-repo.lock");

        let info = serde_json::to_string(&RepoLockInfo {
            pid: std::process::id(),
            command: command.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        })?;

        let lock_path = path.clone();
        let inner =
            FileLock::try_acquire(&path, &info, move |content| {
                match serde_json::from_str::<RepoLockInfo>(content) {
                    Ok(info) => format!(
                        "Another Hitch operation is already in progress in this repository \
                     (command '{}', PID {}, started {}).\n\
                     Wait for it to finish before running another mutating command.",
                        info.command, info.pid, info.started_at
                    ),
                    Err(_) => format!(
                        "Another Hitch operation is already in progress in this repository \
                     (lock held on '{}'). Wait for it to finish.",
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

        let first = RepoLock::acquire(git_dir, "promote").expect("first acquire should succeed");

        // A second acquire while the first is still held must fail.
        let second = RepoLock::acquire(git_dir, "rebuild");
        assert!(
            second.is_err(),
            "second acquire should be blocked while the lock is held"
        );

        // After the first guard is dropped the advisory lock is released and a new
        // acquire succeeds.
        drop(first);
        let third = RepoLock::acquire(git_dir, "release");
        assert!(third.is_ok(), "acquire should succeed after release");
    }

    #[test]
    fn leftover_lock_file_without_live_holder_is_reacquired() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path();
        let path = git_dir.join("hitch-repo.lock");

        // A lock file left behind by a crashed process holds no live flock, so a
        // fresh acquire must succeed (the kernel released the dead process's lock).
        std::fs::write(
            &path,
            r#"{"pid":999999990,"command":"promote","started_at":"2020-01-01T00:00:00+00:00"}"#,
        )
        .unwrap();

        let lock = RepoLock::acquire(git_dir, "promote");
        assert!(
            lock.is_ok(),
            "a leftover lock file from a dead process should be re-acquirable"
        );
    }
}
