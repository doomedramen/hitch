//! Crash recovery for the one window that publishing cannot make atomic.
//!
//! Publishing a rebuild or release is two steps that git gives us no way to
//! fuse: move `refs/heads/<branch>` (a CAS `update-ref`), then bring every
//! checkout attached to that branch back in line with it. If the process dies
//! between them — Ctrl-C, a crash, a killed CI job — the obligation to resync
//! existed only in that process's memory, and the user is left with a checkout
//! whose working tree silently disagrees with its own branch, forever. That is
//! precisely the failure this whole area exists to eliminate, so it cannot be
//! left to chance.
//!
//! So the intent is written down first. Before the ref moves, a record lands
//! at `refs/hitch/pending-resync/<branch>` naming the branch, the tip it is
//! moving from, the tip it is moving to, and the checkouts that will need
//! updating. It is deleted once the resync has actually happened. A record
//! found later therefore means "someone died mid-publish here".
//!
//! **Recovery never guesses.** It repairs a checkout only when that checkout's
//! working tree and index are provably *exactly* the old tip — clean against
//! `from_sha`, nothing staged, nothing modified. That is a fact about the disk
//! right now, not a claim inherited from the dead process, so a user who has
//! since started editing is never reset out from under. Anything else is
//! reported with the command to run and left untouched.

use crate::commands::global_context::GlobalContext;
use crate::utils::git_operations::GitOperations;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const REF_PREFIX: &str = "refs/hitch/pending-resync";

/// A publish that had moved (or was about to move) a branch ref, and the
/// checkouts it still owed an update to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingResync {
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
}

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
pub fn record_blob(context: &GlobalContext, pending: &PendingResync) -> Result<(String, String)> {
    let payload = serde_json::to_vec_pretty(pending)?;
    let blob = context.git().hash_object_bytes(&payload)?;
    Ok((ref_name(&pending.branch), blob))
}

/// Record the intent to move `branch` and resync `checkouts`, before anything
/// observable changes. Blob-backed so the payload is readable with
/// `git cat-file -p` when diagnosing by hand.
pub fn record(context: &GlobalContext, pending: &PendingResync) -> Result<()> {
    let (refname, blob) = record_blob(context, pending)?;
    context.git().update_ref(&refname, &blob)
}

/// Drop the record for `branch` — the resync it described has happened.
pub fn clear(context: &GlobalContext, branch: &str) {
    // Best-effort: a leftover record is recoverable (the next run re-reads it
    // and finds nothing to do), whereas failing the command over it would
    // turn a successful publish into a reported failure.
    let _ = context.git().delete_ref(&ref_name(branch));
}

/// Every pending record currently in the repository.
pub fn list(git: &GitOperations) -> Result<Vec<(String, PendingResync)>> {
    let mut found = Vec::new();

    for refname in git.list_refs_under(REF_PREFIX)? {
        let Ok(payload) = git.cat_file_blob(&refname) else {
            continue;
        };
        match serde_json::from_slice::<PendingResync>(&payload) {
            Ok(pending) => found.push((refname, pending)),
            // A record we can't parse is not something to act on, but it is
            // also not something to delete silently — leave it for `doctor`.
            Err(_) => continue,
        }
    }

    Ok(found)
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

        let _ = context.git().delete_ref(&refname);
    }

    Ok(())
}

fn repair_checkout(context: &GlobalContext, record: &PendingResync, path: &str) {
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

fn warn_manual(context: &GlobalContext, record: &PendingResync, path: &str) {
    context.log_warning(&format!(
        "A previous '{}' publish was interrupted before it could update the working \
         tree at '{}', and that tree has since been modified — it was left alone. \
         To reconcile it once your changes are safe:\n  \
         cd {} && git stash && git reset --hard {} && git stash pop",
        record.branch, path, path, record.branch
    ));
}
