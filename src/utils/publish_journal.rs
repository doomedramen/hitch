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

/// A publish that had moved (or was about to move) a branch ref, and the
/// checkouts and push it still owed.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub fn record_blob(context: &GlobalContext, pending: &PublishRecord) -> Result<(String, String)> {
    let payload = serde_json::to_vec_pretty(pending)?;
    let blob = context.git().hash_object_bytes(&payload)?;
    Ok((ref_name(&pending.branch), blob))
}

/// Record the intent to move `branch` and resync `checkouts`, before anything
/// observable changes. Blob-backed so the payload is readable with
/// `git cat-file -p` when diagnosing by hand.
pub fn record(context: &GlobalContext, pending: &PublishRecord) -> Result<()> {
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
                // A record we can't parse is not something to act on, but it is
                // also not something to delete silently — leave it for `doctor`.
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
            context.log_warning(&format!(
                "A previous '{}' publish moved the branch but was interrupted before it \
                 could push. The local branch is ahead of origin. To finish it:\n  \
                 hitch push {} -f",
                record.branch, record.branch
            ));
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
