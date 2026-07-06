//! Cross-process advisory file lock used by the repo-wide and per-environment
//! rebuild locks.
//!
//! Backed by an OS advisory lock (`flock` on Unix, `LockFileEx` on Windows) via
//! the `fs4` crate, held on an open file handle. This gives three properties the
//! previous check-then-create lockfile scheme lacked, on all of Linux, macOS, and
//! Windows:
//!
//! 1. **Atomic acquisition** — no `exists()` → `write()` TOCTOU window in which
//!    two processes could both "acquire" the lock.
//! 2. **Automatic release on death** — the OS drops the lock when the process
//!    exits for any reason (normal exit, panic, Ctrl-C, SIGKILL, crash, power
//!    loss). No stale-lock cleanup, no PID-liveness heuristics, and no risk of a
//!    permanently stuck lock from PID reuse.
//! 3. **Unambiguous ownership** — the lock lives on the file handle, not on the
//!    file's existence, so a leftover lock file is simply re-locked by the next
//!    caller.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;

/// An acquired advisory lock. Dropping it closes the underlying file handle,
/// which releases the OS lock (the kernel also does this automatically if the
/// process dies without dropping it).
pub struct FileLock {
    // Kept alive purely to hold the lock; released when the handle is dropped.
    _file: std::fs::File,
}

impl FileLock {
    /// Try to acquire the lock at `path` without blocking.
    ///
    /// `info` is written into the file for diagnostics (pid/command/etc.). If the
    /// lock is already held by another process, `holder_message(existing_contents)`
    /// builds the error message shown to the blocked caller.
    pub fn try_acquire(
        path: &Path,
        info: &str,
        holder_message: impl Fn(&str) -> String,
    ) -> Result<Self> {
        use std::io::{Seek, SeekFrom, Write};

        // Open (creating if needed) WITHOUT truncating, so a failed acquire can
        // still read the current holder's diagnostics.
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("Failed to open lock file '{}'", path.display()))?;

        // Non-blocking exclusive advisory lock. We retry briefly on WouldBlock to
        // absorb the release-lag race: when a holder exits/drops, the OS release
        // can lag a hair behind the handle close under load, so an immediate
        // re-acquire could transiently see the lock as held. A genuinely contended
        // lock (held for the duration of another operation) stays held across all
        // attempts and still fails fast — the total wait is only tens of ms.
        //
        // Fully-qualified call so we bind to fs4's trait method rather than the
        // inherent `File::try_lock` stabilized in Rust 1.89 (whose error type
        // differs).
        const MAX_ATTEMPTS: u32 = 10;
        const RETRY_DELAY: Duration = Duration::from_millis(5);

        let mut blocked = false;
        for attempt in 0..MAX_ATTEMPTS {
            match fs4::FileExt::try_lock(&file) {
                Ok(()) => {
                    blocked = false;
                    break;
                }
                Err(fs4::TryLockError::WouldBlock) => {
                    blocked = true;
                    if attempt + 1 < MAX_ATTEMPTS {
                        std::thread::sleep(RETRY_DELAY);
                    }
                }
                Err(fs4::TryLockError::Error(e)) => {
                    return Err(anyhow::anyhow!(
                        "Failed to acquire lock '{}': {}",
                        path.display(),
                        e
                    ));
                }
            }
        }

        if blocked {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            return Err(anyhow::anyhow!("{}", holder_message(&content)));
        }

        // We hold the lock — (re)write our diagnostics as the file contents.
        let _ = file.set_len(0);
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.write_all(info.as_bytes());
        let _ = file.flush();

        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_blocked_until_first_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");

        let first = FileLock::try_acquire(&path, "first", |_| "held".to_string())
            .expect("first acquire should succeed");

        let second = FileLock::try_acquire(&path, "second", |_| "held".to_string());
        assert!(
            second.is_err(),
            "second acquire should be blocked while the lock is held"
        );

        drop(first);

        let third = FileLock::try_acquire(&path, "third", |_| "held".to_string());
        assert!(third.is_ok(), "acquire should succeed after release");
    }

    #[test]
    fn leftover_file_without_live_holder_is_reacquirable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");

        // A leftover lock file from a previous run that no live process holds.
        std::fs::write(&path, "stale contents").unwrap();

        let lock = FileLock::try_acquire(&path, "fresh", |_| "held".to_string());
        assert!(
            lock.is_ok(),
            "a leftover lock file with no live holder should be re-acquirable"
        );
    }
}
