//! A record of what an environment branch's last build actually contained.
//!
//! The Desired / Actual distinction only means something if "Actual" is a fact
//! rather than a guess, and for most of hitch's history it was a guess: the
//! state layer inferred what an environment branch contained by comparing
//! *commit timestamps* against a *wall-clock* rebuild time. That is wrong for
//! every rebased or cherry-picked branch and for any skewed-clock commit, and
//! it is wrong silently — it just reports "up to date" when it is not.
//!
//! So hitch writes down what it built. This module is that writing and that
//! reading. The record is **derived state**: hitch produces it, nobody
//! authors it, and it never lives in `hitch.json` (which stays exactly the
//! user's declaration and nothing else).
//!
//! It is stored as a JSON blob at `refs/hitch/state/<environment>`, a **live
//! pointer** to the most recent build — not an archive. That distinction is
//! load-bearing: `refs/hitch/prev/*` and `refs/hitch/backup/*` are timestamped
//! archives that `hitch cleanup` prunes down to a retention count, and
//! `refs/hitch/state/*` must never be added to that prunable set. There is
//! exactly one record per environment, and it is overwritten in place.
//!
//! **The record and the branch move in the same ref transaction.** `rebuild`
//! hands `publish_branch` the record's `RefEdit` as an extra, so
//! `git update-ref --stdin` applies "environment tip moved" and "record
//! describing that tip now exists" all-or-nothing. A record that contradicts
//! its branch is therefore not a race to be handled — it is a signal, and the
//! reader reports it as such rather than trusting either side.

use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::CompatibilityConflict;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The version of [`EnvironmentBuildRecord`] this build of hitch writes and
/// understands. A record claiming a *higher* version was written by a newer
/// hitch, whose fields this one may misread; [`read_state`] reports it as
/// [`EnvironmentBuildState::Unreadable`] rather than parsing past it.
pub const SCHEMA_VERSION: u32 = 1;

/// A branch name paired with the exact commit a build consumed from it.
///
/// Order within a record is the configured promotion order and is semantic —
/// composition is sequential, and a later branch is checked against everything
/// that actually accumulated before it. Never sort these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedBranch {
    pub branch: String,
    pub sha: String,
}

/// A recorded resolution that a build replayed instead of holding a conflict.
///
/// The `resolution_key` is the content-addressed identity of the resolution
/// (see [`crate::utils::resolutions`]) and names the
/// `refs/hitch/resolutions/<key>` ref it came from. It is recorded, not just
/// the branch name, because the same branch replayed against a *later*
/// conflict resolves under a *different* key — the name alone cannot say which
/// human-authored fix produced a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionUse {
    pub branch: String,
    pub resolution_key: String,
}

/// What hitch knows about one environment's most recent build.
///
/// Every field is something hitch observed while building, not something it
/// inferred afterwards. There is deliberately no `Default` impl: a record that
/// silently filled in a missing `result_sha` would describe a build that never
/// happened, which is the one failure this whole module exists to prevent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentBuildRecord {
    /// Always [`SCHEMA_VERSION`] as written.
    pub schema_version: u32,
    pub environment: String,
    /// Commit of `hitch-metadata` this build's declared branch list came from.
    /// A recorded fact, and nothing more — it is **not** a usable staleness
    /// signal, for a reason worth stating precisely because it is
    /// counter-intuitive and looks like a bug on first inspection.
    ///
    /// A rebuild brackets the commit this records on *both* sides with its own
    /// metadata writes. `with_locked_env` commits the environment's lock
    /// before the declaration is read, and publishing commits the `rebuilt_at`
    /// stamp and the unlock after — so the recorded SHA is a commit that only
    /// ever existed as a transient tip of `hitch-metadata`, and is not
    /// observable from outside the command at all. It is always an *ancestor*
    /// of the branch's final tip (metadata writes are linear on that branch),
    /// and never equal to it.
    ///
    /// That is why it cannot be the "did the declaration change?" answer, on
    /// top of the two more obvious reasons: it moves when an unrelated
    /// environment is promoted, and it returns to a previous value when a
    /// declaration edit is reverted. Staleness is decided by comparing
    /// `desired_branches` against the live refs — see
    /// `RepositoryStateSnapshot` in P3 — never by this field.
    pub metadata_sha: String,
    pub base_name: String,
    pub base_sha: String,
    /// Every branch the environment declared, in promotion order — including
    /// any that ended up held. "Desired" means declared, not delivered.
    pub desired_branches: Vec<PinnedBranch>,
    /// The subset that actually made it into this build, in promotion order.
    /// The complement of `held` within `desired_branches`.
    pub included_branches: Vec<PinnedBranch>,
    /// Branches declared but excluded because they conflicted. Only ever
    /// non-empty under `OnConflict::Eject`; a hold is a *successful* publish.
    pub held: Vec<CompatibilityConflict>,
    pub replayed_resolutions: Vec<ResolutionUse>,
    /// The commit this build produced. Equal to the environment branch's tip
    /// at the moment the record was written — they move together, or the
    /// reader reports the disagreement.
    pub result_sha: String,
    pub built_at: chrono::DateTime<chrono::Utc>,
    pub hitch_version: String,
}

impl EnvironmentBuildRecord {
    /// Build a record from a composition. Deliberately takes every field
    /// explicitly rather than defaulting any of them.
    #[allow(clippy::too_many_arguments)] // one field per observable fact; a params struct would only move the same list one level down
    pub fn new(
        environment: &str,
        metadata_sha: String,
        base_name: String,
        base_sha: String,
        desired_branches: Vec<PinnedBranch>,
        included_branches: Vec<PinnedBranch>,
        held: Vec<CompatibilityConflict>,
        replayed_resolutions: Vec<ResolutionUse>,
        result_sha: String,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            environment: environment.to_string(),
            metadata_sha,
            base_name,
            base_sha,
            desired_branches,
            included_branches,
            held,
            replayed_resolutions,
            result_sha,
            built_at: chrono::Utc::now(),
            hitch_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Every declared branch's name, in promotion order.
    pub fn desired_branch_names(&self) -> Vec<&str> {
        self.desired_branches
            .iter()
            .map(|b| b.branch.as_str())
            .collect()
    }
}

/// The state of one environment's build record, as a reader finds it.
///
/// Every variant is an honest answer. There is no variant meaning "probably
/// fine" — that is the case this enumeration exists to delete.
#[derive(Debug)]
pub enum EnvironmentBuildState {
    /// No record at all: the environment has never been built by a hitch that
    /// writes records, or was built by an older one.
    ///
    /// This is a normal state, not a defect. Not every publish writes a record
    /// — `hitch release` lands a branch without composing one, and so do both
    /// of `hitch resolve`'s publish paths — so an environment can legitimately
    /// have a branch and no record. A reader must say "unknown" here, never
    /// substitute a guess.
    LegacyUnknown,
    /// A record that matches the environment branch's current tip.
    Known(Box<EnvironmentBuildRecord>),
    /// A record exists but describes a different tip than the branch now has:
    /// a publish that carried no record, or someone moving the ref by hand.
    /// `live_tip` is `None` when the environment branch does not exist at all.
    ResultMismatch {
        record: Box<EnvironmentBuildRecord>,
        live_tip: Option<String>,
    },
    /// A record exists but cannot be trusted: unparseable, or written by a
    /// hitch whose schema version this one does not know. The reason is for
    /// display.
    Unreadable { reason: String },
}

/// The ref an environment's live build record lives at.
pub fn state_ref(env_name: &str) -> String {
    format!("refs/hitch/state/{}", env_name)
}

/// Hash a record into the object database and return `(refname, blob_oid)`
/// *without writing the ref*.
///
/// Splitting the write is the point: it lets the caller put the ref update
/// inside `publish_branch`'s existing atomic transaction, alongside the branch
/// move the record describes, rather than as a separate step with a crash
/// window in between. Mirrors `publish_journal::record_blob` for the same
/// reason.
pub fn record_blob(
    git: &GitOperations,
    record: &EnvironmentBuildRecord,
) -> Result<(String, String)> {
    let payload = serde_json::to_vec_pretty(record)?;
    let blob = git.hash_object_bytes(&payload)?;
    Ok((state_ref(&record.environment), blob))
}

/// Read an environment's build record and classify it against the branch's
/// current tip.
///
/// Returns `Err` only for genuine I/O failure — a git invocation that itself
/// failed. A record that is missing, unparseable, ahead of this hitch's schema,
/// or inconsistent with the branch is a *state*, returned as a value, because
/// the alternative is a `hitch status` that cannot render a repo containing one
/// bad blob.
pub fn read_state(git: &GitOperations, env_name: &str) -> Result<EnvironmentBuildState> {
    let Some(oid) = git.rev_parse_opt(&state_ref(env_name))? else {
        return Ok(EnvironmentBuildState::LegacyUnknown);
    };
    let bytes = git.cat_file_blob(&oid)?;

    // The version is probed on its own, *before* the full parse, and that
    // ordering is load-bearing rather than incidental. Deserializing straight
    // into `EnvironmentBuildRecord` would fail on the first unknown or missing
    // field, so a record written by a newer hitch — which by construction has
    // every field this one knows about *plus* fields it does not — would never
    // reach a version check at all. It would be reported as a corrupt blob,
    // which is a different diagnosis and a misleading one: the record is
    // perfectly valid, just newer than this hitch.
    #[derive(Deserialize)]
    struct VersionProbe {
        schema_version: Option<u32>,
    }

    let probe: VersionProbe = match serde_json::from_slice(&bytes) {
        Ok(probe) => probe,
        Err(_) => {
            return Ok(EnvironmentBuildState::Unreadable {
                reason: format!("{} is not a build record (not JSON)", state_ref(env_name)),
            })
        }
    };

    match probe.schema_version {
        None => {
            return Ok(EnvironmentBuildState::Unreadable {
                reason: format!(
                    "{} carries no schema_version, so it is not a build record hitch wrote",
                    state_ref(env_name)
                ),
            })
        }
        Some(v) if v > SCHEMA_VERSION => {
            return Ok(EnvironmentBuildState::Unreadable {
                reason: format!(
                    "written by a newer hitch (schema_version {}; this hitch understands up to {})",
                    v, SCHEMA_VERSION
                ),
            })
        }
        Some(_) => {}
    }

    let record: EnvironmentBuildRecord = match serde_json::from_slice(&bytes) {
        Ok(record) => record,
        // Structurally broken despite claiming a version we understand. Still
        // a state, not a failure: a half-written or hand-edited ref must not
        // make `hitch status` unrunnable.
        Err(e) => {
            return Ok(EnvironmentBuildState::Unreadable {
                reason: format!(
                    "{} claims schema_version {} but does not parse: {}",
                    state_ref(env_name),
                    SCHEMA_VERSION,
                    e
                ),
            })
        }
    };

    let live_tip = git.rev_parse_opt(&format!("refs/heads/{}", env_name))?;
    if live_tip.as_deref() != Some(record.result_sha.as_str()) {
        return Ok(EnvironmentBuildState::ResultMismatch {
            record: Box::new(record),
            live_tip,
        });
    }

    Ok(EnvironmentBuildState::Known(Box::new(record)))
}

/// The commit of `hitch-metadata` to record as a build's declaration source.
///
/// Prefers the local branch, matching `access_metadata_read_only`, which reads
/// the local branch first and only falls back to `origin/`. In the healthy case
/// they agree — `check_metadata_health` guarantees the local branch is not
/// behind its remote before any metadata read proceeds.
///
/// In the already-degraded case they need not: if the local branch is
/// unreadable, the config came from `origin/hitch-metadata` and the SHA
/// recorded here may name a different commit. That is recorded rather than
/// papered over, because the consequence is bounded — staleness is decided by
/// comparing the per-branch SHAs in the record, never by this field.
pub fn resolve_metadata_sha(git: &GitOperations) -> Result<String> {
    git.rev_parse_opt("refs/heads/hitch-metadata")?
        .or(git.rev_parse_opt("refs/remotes/origin/hitch-metadata")?)
        .context(
            "Cannot record which hitch-metadata commit this build came from: neither \
             refs/heads/hitch-metadata nor refs/remotes/origin/hitch-metadata exists. \
             Run `hitch init` to create hitch metadata.",
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::prelude::CompatibilityConflict;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// Test-only plain-git helper, mirroring the one in
    /// `src/core/workspace_index.rs`: a unit test in `src/` cannot reach the
    /// integration harness in `tests/test_framework/`, and must not spawn git
    /// with an inherited terminal stdin.
    fn run_git(repo: &Path, args: &[&str]) -> String {
        #[allow(clippy::disallowed_methods)]
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A scratch repo with a commit on `main`, a `dev` branch, and a
    /// `hitch-metadata` branch — enough shape for every reader case.
    fn scratch() -> (tempfile::TempDir, GitOperations) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "Test User"]);
        fs::write(repo.join("README.md"), "hello").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);

        let main_sha = run_git(repo, &["rev-parse", "HEAD"]);

        run_git(repo, &["checkout", "-b", "hitch-metadata"]);
        fs::write(repo.join("hitch.json"), "{}").expect("write");
        run_git(repo, &["add", "hitch.json"]);
        run_git(repo, &["commit", "-m", "metadata"]);
        let metadata_sha = run_git(repo, &["rev-parse", "HEAD"]);

        run_git(repo, &["checkout", "-b", "dev", &main_sha]);
        let dev_sha = run_git(repo, &["rev-parse", "HEAD"]);

        let git = GitOperations::new_at_path(&repo.to_string_lossy()).expect("open repo");
        let _ = dev_sha;
        let _ = metadata_sha;
        (dir, git)
    }

    fn sample_record(environment: &str, result_sha: &str) -> EnvironmentBuildRecord {
        EnvironmentBuildRecord::new(
            environment,
            "meta-sha".to_string(),
            "main".to_string(),
            "base-sha".to_string(),
            vec![
                PinnedBranch {
                    branch: "feature-b".to_string(),
                    sha: "b-sha".to_string(),
                },
                PinnedBranch {
                    branch: "feature-a".to_string(),
                    sha: "a-sha".to_string(),
                },
            ],
            vec![PinnedBranch {
                branch: "feature-b".to_string(),
                sha: "b-sha".to_string(),
            }],
            vec![CompatibilityConflict {
                branch: "feature-a".to_string(),
                conflicts_with: "main".to_string(),
                conflicted_files: vec!["README.md".to_string()],
            }],
            vec![ResolutionUse {
                branch: "feature-c".to_string(),
                resolution_key: "deadbeef".to_string(),
            }],
            result_sha.to_string(),
        )
    }

    /// Point a ref at arbitrary bytes without needing a worktree commit.
    fn write_state_ref(git: &GitOperations, env: &str, payload: &[u8]) -> String {
        let oid = git.hash_object_bytes(payload).expect("hash");
        git.update_ref(&state_ref(env), &oid).expect("update-ref");
        oid
    }

    #[test]
    fn record_blob_then_read_state_round_trips() -> Result<()> {
        let (_dir, git) = scratch();
        let live = git.rev_parse("refs/heads/dev")?;

        // Deliberately not in alphabetical order: the round trip must preserve
        // what went in, because promotion order is semantic.
        let record = sample_record("dev", &live);
        let (refname, oid) = record_blob(&git, &record)?;
        assert_eq!(refname, "refs/hitch/state/dev");
        git.update_ref(&refname, &oid)?;

        match read_state(&git, "dev")? {
            EnvironmentBuildState::Known(read) => {
                assert_eq!(read.environment, "dev");
                assert_eq!(read.schema_version, SCHEMA_VERSION);
                assert_eq!(read.result_sha, live);
                assert_eq!(read.base_name, "main");
                assert_eq!(read.metadata_sha, "meta-sha");
                assert_eq!(read.hitch_version, env!("CARGO_PKG_VERSION"));
                assert_eq!(
                    read.desired_branch_names(),
                    vec!["feature-b", "feature-a"],
                    "desired_branches lost its order"
                );
                assert_eq!(read.included_branches.len(), 1);
                assert_eq!(read.held[0].branch, "feature-a");
                assert_eq!(read.held[0].conflicts_with, "main");
                assert_eq!(read.replayed_resolutions[0].resolution_key, "deadbeef");
            }
            other => panic!("expected Known, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn read_state_on_a_fresh_environment_is_legacy_unknown() -> Result<()> {
        let (_dir, git) = scratch();
        match read_state(&git, "never-built")? {
            EnvironmentBuildState::LegacyUnknown => {}
            other => panic!("expected LegacyUnknown, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn read_state_flags_a_result_mismatch() -> Result<()> {
        let (_dir, git) = scratch();
        let live = git.rev_parse("refs/heads/dev")?;
        let record = sample_record("dev", "0000000000000000000000000000000000000000");
        let (refname, oid) = record_blob(&git, &record)?;
        git.update_ref(&refname, &oid)?;

        match read_state(&git, "dev")? {
            EnvironmentBuildState::ResultMismatch { record, live_tip } => {
                assert_eq!(live_tip.as_deref(), Some(live.as_str()));
                assert_eq!(record.result_sha, "0".repeat(40));
            }
            other => panic!("expected ResultMismatch, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn read_state_flags_a_missing_tip() -> Result<()> {
        let (_dir, git) = scratch();
        let record = sample_record("ghost", &"0".repeat(40));
        let (refname, oid) = record_blob(&git, &record)?;
        git.update_ref(&refname, &oid)?;

        // The record exists but `refs/heads/ghost` never did. That is still a
        // disagreement, and the reader must say so rather than report a
        // confident Actual for a branch that isn't there.
        match read_state(&git, "ghost")? {
            EnvironmentBuildState::ResultMismatch { live_tip, .. } => {
                assert!(
                    live_tip.is_none(),
                    "expected no live tip, got {:?}",
                    live_tip
                );
            }
            other => panic!("expected ResultMismatch, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn read_state_flags_a_corrupt_blob() -> Result<()> {
        let (_dir, git) = scratch();
        let live = git.rev_parse("refs/heads/dev")?;
        write_state_ref(&git, "dev", b"this is not json");

        match read_state(&git, "dev")? {
            EnvironmentBuildState::Unreadable { reason } => {
                assert!(!reason.is_empty(), "Unreadable must explain itself")
            }
            other => panic!("expected Unreadable, got {:?}", other),
        }
        let _ = live;
        Ok(())
    }

    #[test]
    fn read_state_refuses_a_newer_schema() -> Result<()> {
        let (_dir, git) = scratch();
        let live = git.rev_parse("refs/heads/dev")?;
        write_state_ref(
            &git,
            "dev",
            format!(r#"{{"schema_version":999,"environment":"dev","result_sha":"{live}"}}"#)
                .as_bytes(),
        );

        match read_state(&git, "dev")? {
            EnvironmentBuildState::Unreadable { reason } => {
                assert!(
                    reason.contains("999"),
                    "the reason should name the version it refused, got: {}",
                    reason
                );
            }
            other => panic!("expected Unreadable, got {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn resolve_metadata_sha_prefers_the_local_metadata_branch() -> Result<()> {
        let (_dir, git) = scratch();
        let local = git.rev_parse("refs/heads/hitch-metadata")?;

        // No origin/hitch-metadata exists yet; the local answer must stand.
        assert_eq!(resolve_metadata_sha(&git)?, local);

        // Now point origin/hitch-metadata at a *different* commit and confirm
        // the local branch still wins — matching access_metadata_read_only's
        // read order.
        let other = git.rev_parse("refs/heads/main")?;
        git.update_ref("refs/remotes/origin/hitch-metadata", &other)?;
        assert_ne!(other, local);
        assert_eq!(resolve_metadata_sha(&git)?, local);
        Ok(())
    }
}
