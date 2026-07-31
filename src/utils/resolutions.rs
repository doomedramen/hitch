//! Shared, content-addressed conflict resolutions stored as git refs.
//!
//! See docs/merge-conflict-handling-plan.md (phase 5). When `hitch resolve`
//! resolves a peer-vs-peer conflict (Mode B), the fix can optionally be
//! *recorded* so a byte-identical conflict recurring on a later rebuild can
//! be replayed instead of re-resolved by hand.
//!
//! ## Why content-addressed OID tuples, not git-rerere
//!
//! The design doc originally said "rerere-backed". The red-team pass on that
//! design surfaced two critical problems with rerere here, and its own
//! mandated mitigations pointed at a different primitive:
//!
//! - rerere keys on *normalized conflict-hunk text*, so a force-push that
//!   changes a branch's meaning while leaving the conflicted hunk textually
//!   identical replays the old (now wrong) resolution silently. Keying on
//!   the exact stage-1/2/3 blob OIDs instead makes any change to any side a
//!   cache *miss*, never a wrong replay — staleness is structural, not a
//!   check that can be forgotten.
//! - rerere's cache is `.git/rr-cache`, global to the repo and not
//!   relocatable, so hydrating teammates' resolutions into it changes the
//!   behavior of the user's *own* plain-git merges outside hitch. Storing
//!   resolutions as their own refs and applying them only inside hitch's
//!   worktree operations never touches rr-cache at all.
//!
//! Since Mode B already owns the whole conflict, hitch reads the three merge
//! stages directly (`git ls-files --unmerged`) and needs neither rerere nor
//! any hand-rolled hunk normalization.
//!
//! ## Storage
//!
//! One ref per resolution at `refs/hitch/resolutions/<key>`, where `<key>`
//! is a git hash over the sorted `(path, base_oid, ours_oid, theirs_oid)`
//! tuples of the conflict. The ref points at a parentless commit whose tree
//! holds `meta.json` plus one blob per resolved file (`f0`, `f1`, ...) — so
//! everything transports via ordinary `git push`/`fetch` and can never be
//! GC'd out from under the ref.

use crate::utils::git_operations::GitOperations;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const RESOLUTIONS_REF_PREFIX: &str = "refs/hitch/resolutions/";
pub const RESOLUTIONS_REFSPEC: &str = "+refs/hitch/resolutions/*:refs/hitch/resolutions/*";

/// One conflicted path's raw merge stages before a blob name is assigned:
/// `(path, base_oid, ours_oid, theirs_oid)`, each side optional.
pub type StageTuple = (String, Option<String>, Option<String>, Option<String>);

/// One conflicted file's three merge stages — the content-addressed key
/// inputs. Any side may be absent (add/add, delete/modify).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictStage {
    pub path: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    /// Name of the resolved-content blob in the ref's tree (`f0`, `f1`...).
    pub blob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionMeta {
    pub env: String,
    pub branch: String,
    pub conflicts_with: String,
    /// The head SHA of `branch` at record time — audit only; correctness
    /// comes from the exact stage-OID match, not this.
    pub source_branch_head: String,
    pub recorded_by: String,
    pub recorded_at: String,
    pub hitch_version: String,
    /// The content-addressed key (also the ref's basename).
    pub key: String,
    pub files: Vec<ConflictStage>,
    /// Armored SSH signature (`ssh-keygen -Y sign`) over this struct
    /// serialized with `signature` set to `None` — the canonical unsigned
    /// payload, so verification can reconstruct exactly these bytes.
    /// `None` for resolutions recorded before signing existed, or recorded on
    /// a machine with no SSH signing key configured (`sign_bytes_ssh`
    /// returns `None` rather than erroring in that case).
    #[serde(default)]
    pub signature: Option<String>,
}

/// A recorded resolution, loaded from its ref.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub meta: ResolutionMeta,
    /// Resolved content per path, in `files` order.
    pub resolved: Vec<(String, Vec<u8>)>,
    pub commit_oid: String,
}

/// The captured conflict at the moment a Mode B resolve session starts,
/// persisted to the worktree's private git dir so `--continue` can record
/// the resolution against the exact stages that conflicted (the resolved
/// index no longer carries them once the user `git add`s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConflict {
    pub env: String,
    pub branch: String,
    pub conflicts_with: String,
    pub source_branch_head: String,
    /// `(path, base, ours, theirs)` for each conflicted file, no blob name
    /// yet (assigned at record time).
    pub stages: Vec<StageTuple>,
}

/// Compute the stable resolution key from the conflict stages: a git hash
/// over the sorted `(path, base, ours, theirs)` tuples. Uses `git
/// hash-object` so there's no extra hashing dependency and the value is a
/// normal 40-char object id usable as a ref basename.
pub fn resolution_key(git: &GitOperations, stages: &[StageTuple]) -> Result<String> {
    let mut sorted: Vec<_> = stages.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut canonical = String::new();
    for (path, base, ours, theirs) in &sorted {
        canonical.push_str(&format!(
            "{}\0{}\0{}\0{}\n",
            path,
            base.as_deref().unwrap_or("-"),
            ours.as_deref().unwrap_or("-"),
            theirs.as_deref().unwrap_or("-"),
        ));
    }
    git.hash_object_bytes(canonical.as_bytes())
}

pub enum RecordOutcome {
    /// Newly recorded (or an identical recording already existed).
    Recorded { key: String, files: usize },
    /// A resolution for this exact conflict already exists with *different*
    /// resolved content — not overwritten; the caller must decide.
    Conflict {
        key: String,
        existing_commit: String,
    },
}

/// Record a resolution for `pending`, reading each conflicted file's
/// resolved content from `resolved_root` (the resolve worktree). Idempotent
/// for an identical re-record; refuses to silently overwrite a differing
/// existing resolution (returns `RecordOutcome::Conflict`).
pub fn record_resolution(
    git: &GitOperations,
    pending: &PendingConflict,
    resolved_root: &std::path::Path,
    recorded_by: &str,
    recorded_at: &str,
) -> Result<RecordOutcome> {
    let key = resolution_key(git, &pending.stages)?;
    let refname = format!("{}{}", RESOLUTIONS_REF_PREFIX, key);

    // Build tree entries: one resolved blob per file, plus meta.json.
    let mut files = Vec::new();
    let mut tree_entries = Vec::new();
    for (i, (path, base, ours, theirs)) in pending.stages.iter().enumerate() {
        let content = std::fs::read(resolved_root.join(path)).map_err(|e| {
            anyhow::anyhow!(
                "Cannot read resolved '{}' to record the resolution: {}",
                path,
                e
            )
        })?;
        let blob = format!("f{}", i);
        let blob_oid = git.hash_object_bytes(&content)?;
        tree_entries.push((blob.clone(), blob_oid));
        files.push(ConflictStage {
            path: path.clone(),
            base: base.clone(),
            ours: ours.clone(),
            theirs: theirs.clone(),
            blob,
        });
    }

    let mut meta = ResolutionMeta {
        env: pending.env.clone(),
        branch: pending.branch.clone(),
        conflicts_with: pending.conflicts_with.clone(),
        source_branch_head: pending.source_branch_head.clone(),
        recorded_by: recorded_by.to_string(),
        recorded_at: recorded_at.to_string(),
        hitch_version: env!("CARGO_PKG_VERSION").to_string(),
        key: key.clone(),
        files,
        signature: None,
    };
    // Sign the meta payload as it stands *without* the signature field, so
    // verification can reconstruct exactly these bytes. `sign_bytes_ssh`
    // returns `None` (not an error) when this machine has no SSH signing key
    // configured, so recording still succeeds unsigned.
    let unsigned = serde_json::to_vec_pretty(&meta)?;
    meta.signature = git.sign_bytes_ssh(&unsigned)?;
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    let meta_oid = git.hash_object_bytes(&meta_json)?;
    tree_entries.push(("meta.json".to_string(), meta_oid));

    let tree = git.make_blob_tree(&tree_entries)?;

    // If a resolution already exists for this exact conflict, only replace it
    // when the resolved trees actually differ — and even then, don't clobber:
    // report the clash so the caller can surface both.
    if let Some(existing_commit) = git.rev_parse_opt(&refname)? {
        let existing_tree = git.rev_parse_opt(&format!("{}^{{tree}}", existing_commit))?;
        if existing_tree.as_deref() == Some(tree.as_str()) {
            return Ok(RecordOutcome::Recorded {
                key,
                files: pending.stages.len(),
            });
        }
        return Ok(RecordOutcome::Conflict {
            key,
            existing_commit,
        });
    }

    let commit = git.commit_tree_parentless(
        &tree,
        &format!(
            "hitch resolution: {} vs {} in {}",
            pending.branch, pending.conflicts_with, pending.env
        ),
    )?;
    git.update_ref(&refname, &commit)?;

    Ok(RecordOutcome::Recorded {
        key,
        files: pending.stages.len(),
    })
}

/// Load the resolution for `key`, if a ref exists.
pub fn load_resolution(git: &GitOperations, key: &str) -> Result<Option<Resolution>> {
    let refname = format!("{}{}", RESOLUTIONS_REF_PREFIX, key);
    let Some(commit_oid) = git.rev_parse_opt(&refname)? else {
        return Ok(None);
    };
    load_from_commit(git, &commit_oid)
}

fn load_from_commit(git: &GitOperations, commit_oid: &str) -> Result<Option<Resolution>> {
    let meta_bytes = git.read_blob(&format!("{}:meta.json", commit_oid))?;
    let meta: ResolutionMeta = serde_json::from_slice(&meta_bytes)?;

    let mut resolved = Vec::new();
    for f in &meta.files {
        let content = git.read_blob(&format!("{}:{}", commit_oid, f.blob))?;
        resolved.push((f.path.clone(), content));
    }

    Ok(Some(Resolution {
        meta,
        resolved,
        commit_oid: commit_oid.to_string(),
    }))
}

/// List every recorded resolution reachable locally.
pub fn list_resolutions(git: &GitOperations) -> Result<Vec<Resolution>> {
    let mut out = Vec::new();
    for (_refname, oid) in git.list_refs(RESOLUTIONS_REF_PREFIX)? {
        if let Some(res) = load_from_commit(git, &oid)? {
            out.push(res);
        }
    }
    out.sort_by(|a, b| a.meta.recorded_at.cmp(&b.meta.recorded_at));
    Ok(out)
}

/// The private git directory of a linked worktree — where the pending
/// conflict state is stashed between resolve start and `--continue` (outside
/// the working tree, so `git add` can't sweep it in, and auto-removed with
/// the worktree).
pub fn pending_state_path(worktree_git: &GitOperations) -> std::path::PathBuf {
    std::path::PathBuf::from(worktree_git.get_git_dir()).join("hitch-resolve.json")
}

pub fn write_pending(worktree_git: &GitOperations, pending: &PendingConflict) -> Result<()> {
    let path = pending_state_path(worktree_git);
    std::fs::write(&path, serde_json::to_vec_pretty(pending)?)?;
    Ok(())
}

pub fn read_pending(worktree_git: &GitOperations) -> Result<Option<PendingConflict>> {
    let path = pending_state_path(worktree_git);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Verify a loaded resolution's signature.
///
/// Reconstructs the exact payload that was signed (the meta with `signature`
/// cleared) and checks it against the repository's allowed-signers file, with
/// `recorded_by` as the claimed signer identity. Any failure — no signature,
/// no allowed-signers file, unknown signer, altered content — is `false`;
/// this must fail closed, since it gates whether unreviewed content gets
/// spliced into a build.
pub fn verify_resolution_signature(git: &GitOperations, res: &Resolution) -> Result<bool> {
    let Some(signature) = res.meta.signature.as_deref() else {
        return Ok(false);
    };
    let mut unsigned = res.meta.clone();
    unsigned.signature = None;
    let payload = serde_json::to_vec_pretty(&unsigned)?;
    git.verify_signature_ssh(&payload, signature, &res.meta.recorded_by)
}

#[cfg(test)]
mod tests {
    use super::StageTuple;

    #[test]
    fn key_is_order_independent_and_content_sensitive() {
        // Same tuples in different order → same canonical string (sorted by
        // path), so the same key; a changed blob oid → a different key.
        // We can't call git here, so test the canonicalization directly by
        // replicating what resolution_key hashes.
        let canon = |stages: &[StageTuple]| {
            let mut sorted: Vec<_> = stages.to_vec();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut s = String::new();
            for (p, b, o, t) in &sorted {
                s.push_str(&format!(
                    "{}\0{}\0{}\0{}\n",
                    p,
                    b.as_deref().unwrap_or("-"),
                    o.as_deref().unwrap_or("-"),
                    t.as_deref().unwrap_or("-"),
                ));
            }
            s
        };

        let a = vec![
            (
                "b.txt".to_string(),
                Some("b1".into()),
                Some("b2".into()),
                Some("b3".into()),
            ),
            (
                "a.txt".to_string(),
                Some("a1".into()),
                Some("a2".into()),
                Some("a3".into()),
            ),
        ];
        let b = vec![
            (
                "a.txt".to_string(),
                Some("a1".into()),
                Some("a2".into()),
                Some("a3".into()),
            ),
            (
                "b.txt".to_string(),
                Some("b1".into()),
                Some("b2".into()),
                Some("b3".into()),
            ),
        ];
        assert_eq!(canon(&a), canon(&b), "order must not change the key input");

        let mut c = b.clone();
        c[0].2 = Some("DIFFERENT".into());
        assert_ne!(
            canon(&b),
            canon(&c),
            "a changed stage oid must change the key input"
        );
    }
}
