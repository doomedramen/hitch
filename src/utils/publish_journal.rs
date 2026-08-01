//! The record of what a publish still owes, for the effects git cannot make
//! atomic.
//!
//! Publishing a rebuild or release moves `refs/heads/<branch>` with a single
//! CAS `update-ref`, but two effects remain outside that transaction: bring
//! every checkout attached to the branch back in line with it, and push the
//! new tip to origin. If the process dies between the ref move and either of
//! those — Ctrl-C, a crash, a killed CI job — the obligation existed only in
//! that process's memory. For the resync, that leaves a checkout whose
//! working tree silently disagrees with its own branch, forever. For the
//! push, it leaves the local branch ahead of origin with nothing recording
//! why. That is precisely the failure this whole area exists to eliminate, so
//! it cannot be left to chance.
//!
//! So the intent is written down first. Before the ref moves, a record lands
//! at `refs/hitch/publish/<branch>` naming the branch, the tip it is moving
//! from, the tip it is moving to, the checkouts that will need updating, and
//! whether a push is owed. The resync and push obligations are cleared
//! independently, as each completes — a record found later with either still
//! set therefore means "someone died mid-publish here".
//!
//! **Recovery never guesses.** It repairs a checkout only when that checkout's
//! working tree and index are provably *exactly* the old tip — clean against
//! `from_sha`, nothing staged, nothing modified. That is a fact about the disk
//! right now, not a claim inherited from the dead process, so a user who has
//! since started editing is never reset out from under. Anything else is
//! reported with the command to run and left untouched. An owed push is
//! likewise only ever reported, never performed — pushing on someone's behalf
//! during a startup recovery pass is a network side effect nobody asked for.
//!
//! Legacy `refs/hitch/pending-resync/<branch>` records (written before this
//! journal covered the push) are still read, so a hitch upgrade partway
//! through a publish still finishes it rather than stranding it.

use crate::commands::global_context::GlobalContext;
use crate::utils::git_operations::GitOperations;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const REF_PREFIX: &str = "refs/hitch/publish";

/// The namespace this journal used before it covered the push step. Read on
/// recovery so a hitch upgrade partway through a publish still finishes it.
const LEGACY_REF_PREFIX: &str = "refs/hitch/pending-resync";

/// Who/where/when a publish attempt started — attribution for `hitch doctor`
/// so a wedged publish can be traced to the process that made it without
/// correlating logs by hand.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PublishAttribution {
    pub pid: u32,
    pub hostname: String,
    pub started_at: String,
}

/// A publish that had moved (or was about to move) a branch ref, and the
/// checkouts and push it still owed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PublishRecord {
    #[serde(default)]
    pub branch: String,
    /// The branch tip before the publish. A checkout still holding exactly
    /// this content is provably stale rather than edited.
    #[serde(default)]
    pub from_sha: Option<String>,
    #[serde(default)]
    pub to_sha: String,
    /// Checkout paths that had the branch attached when the publish started.
    #[serde(default)]
    pub checkouts: Vec<String>,
    /// Whether this publish still owes a push to origin. Set when the publish
    /// intends to push; cleared by `mark_push_done` once it has. A record found
    /// later with this still set means the process died between moving the ref
    /// and telling the remote.
    #[serde(default)]
    pub push_owed: bool,
    /// Who/where/when this attempt started. Stamped by `record_blob`, not the
    /// caller — set to `..Default::default()` when constructing a literal.
    #[serde(default)]
    pub initiated_by: Option<PublishAttribution>,
    /// How many publish attempts this branch has had since the last time its
    /// record was fully cleared. Stamped by `record_blob`, not the caller.
    /// Lets a second process racing to fix the same wedge detect that
    /// another attempt already changed things underneath it.
    #[serde(default)]
    pub generation: u64,
    /// The command `recover()` should suggest if this publish's push never
    /// resolves. `rebuild`/`resolve` use force-with-lease (`hitch push <b>
    /// -f`); `release` does not — its push is a plain fast-forward/merge,
    /// not a rewrite, and suggesting a force-push of a release target
    /// (typically `main`/`production`, exactly what `hitch setup`'s
    /// branch-protection ruleset guards) would be actively dangerous.
    /// Empty for a record written before this field existed; `recover()`
    /// falls back to the old hardcoded `-f` remedy only in that case.
    #[serde(default)]
    pub push_remedy: String,
}

/// Abort the process at a named point in the publish sequence, if the
/// environment asks for it.
///
/// This exists solely so the crash-fuzz tests can interrupt a publish at each
/// step and assert that recovery converges. `std::process::abort` rather than a
/// normal exit, because the whole point is to skip every destructor and
/// cleanup path exactly as a `kill -9` would.
///
/// Gated on `debug_assertions` so a release build compiles this out entirely
/// (see the `not(debug_assertions)` twin below) — otherwise anyone able to
/// set an environment variable on a production `hitch` process (a shared CI
/// runner, anyone with filesystem/environment access to where hitch runs)
/// could abort a real publish mid-flight. `just test` builds and runs
/// against the debug profile (`CARGO_BIN_EXE_hitch`), so `debug_assertions`
/// is true for the whole crash-fuzz suite.
#[cfg(debug_assertions)]
pub fn maybe_abort_for_test(point: &str) {
    if std::env::var("HITCH_TEST_ABORT_AFTER").as_deref() == Ok(point) {
        eprintln!("hitch: aborting after '{}' (HITCH_TEST_ABORT_AFTER)", point);
        std::process::abort();
    }
}

#[cfg(not(debug_assertions))]
pub fn maybe_abort_for_test(_point: &str) {}

fn ref_name(branch: &str) -> String {
    // Branch names contain '/'; the ref path nests accordingly, which is fine
    // for a ref hierarchy and keeps the branch recoverable from the ref name.
    format!("{}/{}", REF_PREFIX, branch)
}

/// Hash the payload for `pending` and return `(refname, blob_oid)` without
/// writing the ref.
///
/// Splitting the write lets the caller put the ref update inside a larger
/// atomic transaction alongside the branch move it describes, instead of doing
/// it as a separate step with a crash window in between.
///
/// Stamps `generation` and `initiated_by` itself, overwriting whatever the
/// caller passed for those two fields — they describe *this* call, not
/// something the caller can meaningfully set.
pub fn record_blob(git: &GitOperations, pending: &PublishRecord) -> Result<(String, String)> {
    let mut pending = pending.clone();
    pending.generation = next_generation(git, &pending.branch)?;
    pending.initiated_by = Some(PublishAttribution {
        pid: std::process::id(),
        hostname: hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        started_at: chrono::Utc::now().to_rfc3339(),
    });
    let payload = serde_json::to_vec_pretty(&pending)?;
    let blob = git.hash_object_bytes(&payload)?;
    Ok((ref_name(&pending.branch), blob))
}

/// One more than the generation of the most recent still-pending record for
/// `branch`, or 1 if none exists. A record survives here only when a prior
/// publish attempt never resolved — a fresh attempt over that unresolved
/// state is a new generation, not a repeat of the same one.
fn next_generation(git: &GitOperations, branch: &str) -> Result<u64> {
    let prior = list(git)?
        .into_iter()
        .find(|(_, r)| r.branch == branch)
        .map(|(_, r)| r.generation);
    Ok(prior.unwrap_or(0) + 1)
}

/// Record the intent to move `branch` and resync `checkouts`, before anything
/// observable changes. Blob-backed so the payload is readable with
/// `git cat-file -p` when diagnosing by hand.
pub fn record(context: &GlobalContext, pending: &PublishRecord) -> Result<()> {
    let (refname, blob) = record_blob(context.git(), pending)?;
    context.git().update_ref(&refname, &blob)
}

/// Drop the record for `branch` — the resync it described has happened.
pub fn clear(context: &GlobalContext, branch: &str) {
    // Best-effort: a leftover record is recoverable (the next run re-reads it
    // and finds nothing to do), whereas failing the command over it would
    // turn a successful publish into a reported failure.
    let _ = context.git().delete_ref(&ref_name(branch));
}

/// Every pending record currently in the repository, from both the current
/// namespace and the legacy one an in-flight upgrade may have left behind.
pub fn list(git: &GitOperations) -> Result<Vec<(String, PublishRecord)>> {
    let mut found = Vec::new();

    for prefix in [REF_PREFIX, LEGACY_REF_PREFIX] {
        for refname in git.list_refs_under(prefix)? {
            let Ok(payload) = git.cat_file_blob(&refname) else {
                continue;
            };
            match serde_json::from_slice::<PublishRecord>(&payload) {
                Ok(record) => found.push((refname, record)),
                // A record we can't parse is not something to act on, but it
                // is also not something to delete silently. `hitch doctor`'s
                // `check_pending_publishes` surfaces parseable records; an
                // unparseable one still isn't reported anywhere — it's left
                // in place for a human to find by hand
                // (`git for-each-ref refs/hitch/publish`) rather than
                // deleted, in case that ever changes.
                Err(_) => continue,
            }
        }
    }

    Ok(found)
}

/// Clear the push obligation on `branch`'s record, leaving the resync
/// obligation intact.
///
/// The two obligations are cleared separately because they complete at
/// different times: the resync happens locally and immediately, the push
/// happens afterwards and can be declined or fail.
pub fn mark_push_done(context: &GlobalContext, branch: &str) -> Result<()> {
    let refname = ref_name(branch);
    let Ok(payload) = context.git().cat_file_blob(&refname) else {
        return Ok(());
    };
    let Ok(mut record) = serde_json::from_slice::<PublishRecord>(&payload) else {
        return Ok(());
    };
    if !record.push_owed {
        return Ok(());
    }
    record.push_owed = false;
    let updated = serde_json::to_vec_pretty(&record)?;
    let blob = context.git().hash_object_bytes(&updated)?;
    context.git().update_ref(&refname, &blob)
}

/// Finish any publish that died before it could resync, then drop its record.
///
/// Runs at startup for mutating commands only, so it is always covered by the
/// repo-wide lock and can never race a publish that is still in flight.
pub fn recover(context: &GlobalContext) -> Result<()> {
    let pending = match list(context.git()) {
        Ok(p) => p,
        // Recovery is a courtesy on the way to the real command; a repository
        // that can't even be queried will fail loudly enough on its own.
        Err(_) => return Ok(()),
    };

    for (refname, record) in pending {
        // If the branch never actually moved, the process died before the CAS
        // and there is nothing to repair.
        let current = context
            .git()
            .rev_parse_opt(&format!("refs/heads/{}", record.branch))?;
        if current.as_deref() != Some(record.to_sha.as_str()) {
            let _ = context.git().delete_ref(&refname);
            continue;
        }

        for path in &record.checkouts {
            repair_checkout(context, &record, path);
        }

        if record.push_owed {
            // Unlike the resync obligation, a push is never actually
            // discharged by anything that runs after the crash — it either
            // already landed (the process died between the push succeeding
            // and `mark_push_done`/`clear` running; see
            // `force_push_with_deploy_key_if_configured` /
            // `record_pushed_tip` in `prelude.rs`, which update
            // `refs/remotes/origin/<branch>` as part of a successful push,
            // before either of those) or it genuinely never happened. Tell
            // those apart before saying anything, and only drop the record
            // once the obligation is actually gone — warning once and then
            // deleting the record regardless would either lie (claiming a
            // push is owed when it already landed) or permanently forget a
            // real one (deleting the only place recording that it didn't).
            let remote_tip = context
                .git()
                .rev_parse_opt(&format!("refs/remotes/origin/{}", record.branch))
                .ok()
                .flatten();
            if remote_tip.as_deref() == Some(record.to_sha.as_str()) {
                context.log_info(&format!(
                    "A previous '{}' publish was interrupted, but its push had already \
                     completed before the process died. Nothing to do.",
                    record.branch
                ));
            } else {
                let remedy = if record.push_remedy.is_empty() {
                    format!("hitch push {} -f", record.branch)
                } else {
                    record.push_remedy.clone()
                };
                context.log_warning(&format!(
                    "A previous '{}' publish moved the branch but was interrupted before it \
                     could push. The local branch is ahead of origin. To finish it:\n  {}",
                    record.branch, remedy
                ));
                // The push obligation is still genuinely owed — leave the
                // record in place so the next mutating command's `recover()`
                // finds it and warns again, instead of silently forgetting
                // it after this one report.
                continue;
            }
        }

        let _ = context.git().delete_ref(&refname);
    }

    Ok(())
}

fn repair_checkout(context: &GlobalContext, record: &PublishRecord, path: &str) {
    // Only act on a checkout that still has the branch attached — the user may
    // have switched away since, in which case nothing is out of sync.
    let attached = context
        .git()
        .checkouts_on_branch(&record.branch)
        .map(|checkouts| {
            checkouts
                .iter()
                .any(|c| GitOperations::same_checkout_path(&c.path, path))
        })
        .unwrap_or(false);
    if !attached {
        return;
    }

    let Ok(git) = GitOperations::new_at_path(path) else {
        return;
    };

    // Already consistent — the resync ran before the process died, or the user
    // fixed it themselves.
    if git.is_working_directory_clean().unwrap_or(false) {
        return;
    }

    // The proof: is this working tree *exactly* the old tip? If so it is stale,
    // not edited, and advancing it cannot lose anything.
    let Some(from_sha) = record.from_sha.as_deref() else {
        warn_manual(context, record, path);
        return;
    };
    if !git.matches_commit_exactly(from_sha).unwrap_or(false) {
        warn_manual(context, record, path);
        return;
    }

    match git.reset_hard_to(&record.to_sha) {
        Ok(()) => context.log_info(&format!(
            "Finished an interrupted '{}' publish: updated the working tree at '{}'.",
            record.branch, path
        )),
        Err(_) => warn_manual(context, record, path),
    }
}

fn warn_manual(context: &GlobalContext, record: &PublishRecord, path: &str) {
    context.log_warning(&format!(
        "A previous '{}' publish was interrupted before it could update the working \
         tree at '{}', and that tree has since been modified — it was left alone. \
         To reconcile it once your changes are safe:\n  \
         cd {} && git stash && git reset --hard {} && git stash pop",
        record.branch, path, path, record.branch
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::git_operations::GitOperations;

    /// A scratch git repo for tests that need real plumbing (`record_blob`
    /// hashes a blob, `list` reads refs back) but nothing hitch-specific —
    /// no `hitch.json`, no environments. Mirrors the raw-git test helpers at
    /// `src/utils/prelude.rs:1430` and `:1989`, which exist for the same
    /// reason: `GitOperations` alone is enough here, a full
    /// `HitchTestFramework` repo is not needed.
    #[allow(clippy::disallowed_methods)] // test-only scratch repo bootstrap, same rationale as prelude.rs's raw-git test helpers
    fn init_scratch_repo() -> (tempfile::TempDir, GitOperations) {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@test"]);
        run(&["config", "user.name", "test"]);
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        let git = GitOperations::new_at_path(&dir.path().to_string_lossy()).unwrap();
        (dir, git)
    }

    #[test]
    fn record_blob_stamps_attribution() {
        let (_dir, git) = init_scratch_repo();
        let record = PublishRecord {
            branch: "dev".to_string(),
            to_sha: "deadbeef".to_string(),
            ..Default::default()
        };

        let (refname, blob) = record_blob(&git, &record).unwrap();
        git.update_ref(&refname, &blob).unwrap();

        let found = list(&git).unwrap();
        assert_eq!(found.len(), 1);
        let attribution = found[0]
            .1
            .initiated_by
            .as_ref()
            .expect("record_blob should always stamp attribution");
        assert_eq!(attribution.pid, std::process::id());
        assert!(!attribution.hostname.is_empty());
        assert!(!attribution.started_at.is_empty());
    }

    #[test]
    fn generation_increments_across_unresolved_records_for_same_branch() {
        let (_dir, git) = init_scratch_repo();
        let record = PublishRecord {
            branch: "dev".to_string(),
            to_sha: "deadbeef".to_string(),
            ..Default::default()
        };

        let (refname, blob) = record_blob(&git, &record).unwrap();
        git.update_ref(&refname, &blob).unwrap();
        let first = list(&git).unwrap();
        assert_eq!(first[0].1.generation, 1);

        // A second attempt over the still-unresolved first record — e.g. a
        // second process racing to fix the same wedge — must see a higher
        // generation, not repeat 1.
        let (refname2, blob2) = record_blob(&git, &record).unwrap();
        git.update_ref(&refname2, &blob2).unwrap();
        let second = list(&git).unwrap();
        assert_eq!(
            second.len(),
            1,
            "same branch, same ref name — overwrites in place"
        );
        assert_eq!(second[0].1.generation, 2);
    }

    #[test]
    fn generation_is_independent_per_branch() {
        let (_dir, git) = init_scratch_repo();
        let dev = PublishRecord {
            branch: "dev".to_string(),
            to_sha: "deadbeef".to_string(),
            ..Default::default()
        };
        let qa = PublishRecord {
            branch: "qa".to_string(),
            to_sha: "cafef00d".to_string(),
            ..Default::default()
        };

        let (dev_ref, dev_blob) = record_blob(&git, &dev).unwrap();
        git.update_ref(&dev_ref, &dev_blob).unwrap();
        let (qa_ref, qa_blob) = record_blob(&git, &qa).unwrap();
        git.update_ref(&qa_ref, &qa_blob).unwrap();

        let found = list(&git).unwrap();
        for (_, record) in &found {
            assert_eq!(
                record.generation, 1,
                "each branch starts its own count at 1"
            );
        }
    }
}
