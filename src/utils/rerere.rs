use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{check_metadata_health, switch_to};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const METADATA_RR_ROOT: &str = "hitch/rr-cache";
const METADATA_RR_ENTRIES: &str = "hitch/rr-cache/entries";
const METADATA_RR_INDEX: &str = "hitch/rr-cache/index.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RerereIndex {
    #[serde(default)]
    pub entries: BTreeMap<String, RerereEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerereEntry {
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub exported_at: DateTime<Utc>,
    #[serde(default)]
    pub contexts: Vec<RerereContext>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerereContext {
    pub env_name: String,
    pub base_branch: BranchRef,
    #[serde(default)]
    pub promoted_branches: Vec<BranchRef>,
    #[serde(default)]
    pub conflict_branch: Option<BranchRef>,
    #[serde(default)]
    pub metadata_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRef {
    pub name: String,
    #[serde(default)]
    pub sha: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    pub imported_files: usize,
    pub imported_entries: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ExportSummary {
    pub exported_files: usize,
    pub exported_entries: usize,
    pub committed: bool,
}

pub fn rr_cache_dir(context: &GlobalContext) -> PathBuf {
    PathBuf::from(context.git().get_git_dir()).join("rr-cache")
}

pub fn import_shared_rerere_cache(context: &GlobalContext) -> Result<Option<ImportSummary>> {
    check_metadata_health(context)?;

    let source_branch = best_metadata_read_ref(context);

    let files = match context
        .git()
        .list_files_in_branch(&source_branch, METADATA_RR_ENTRIES)
    {
        Ok(files) => files,
        Err(_) if source_branch.starts_with("origin/") => context
            .git()
            .list_files_in_branch("hitch-metadata", METADATA_RR_ENTRIES)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    if files.is_empty() {
        return Ok(None);
    }

    let base = PathBuf::from(context.git().get_git_dir()).join("rr-cache");
    fs::create_dir_all(&base).ok();

    let mut imported_files = 0usize;
    let mut entry_ids: HashSet<String> = HashSet::new();

    for full_path in files {
        let rel = match full_path.strip_prefix(&format!("{}/", METADATA_RR_ENTRIES)) {
            Some(r) => r,
            None => continue,
        };
        let mut parts = rel.splitn(2, '/');
        let entry_id = parts.next().unwrap_or("").to_string();
        let remainder = parts.next().unwrap_or("");
        if entry_id.is_empty() || remainder.is_empty() {
            continue;
        }

        let bytes = context
            .git()
            .read_blob_from_branch(&source_branch, &full_path)
            .or_else(|_| {
                if source_branch.starts_with("origin/") {
                    context
                        .git()
                        .read_blob_from_branch("hitch-metadata", &full_path)
                } else {
                    Err(anyhow::anyhow!("Failed to read {}", full_path))
                }
            })?;

        let dst = base.join(entry_id.clone()).join(remainder);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create rr-cache destination directory {:?}",
                    parent
                )
            })?;
        }
        fs::write(&dst, bytes)
            .with_context(|| format!("Failed to write rr-cache file {:?}", dst))?;
        imported_files += 1;
        entry_ids.insert(entry_id);
    }

    Ok(Some(ImportSummary {
        imported_files,
        imported_entries: entry_ids.len(),
    }))
}

pub fn export_rerere_cache_to_metadata(
    context: &GlobalContext,
    state: &crate::utils::resolve_state::ResolveState,
) -> Result<ExportSummary> {
    check_metadata_health(context)?;

    let rr_cache = rr_cache_dir(context);
    if !rr_cache.exists() {
        return Ok(ExportSummary::default());
    }

    let now = Utc::now();
    let metadata_sha = context
        .git()
        .rev_parse_fallback(&["origin/hitch-metadata", "hitch-metadata"])
        .ok();

    let promoted = {
        let mut out = Vec::new();
        out.extend(state.merged_so_far.clone());
        out.extend(state.remaining_branches.clone());
        out
    };

    let ctx = RerereContext {
        env_name: state.env_name.clone(),
        base_branch: BranchRef {
            name: state.base_branch.clone(),
            sha: branch_sha_opt(context, &state.base_branch),
        },
        promoted_branches: promoted
            .iter()
            .map(|b| BranchRef {
                name: b.clone(),
                sha: branch_sha_opt(context, b),
            })
            .collect(),
        conflict_branch: Some(BranchRef {
            name: state.conflict_branch.clone(),
            sha: branch_sha_opt(context, &state.conflict_branch),
        }),
        metadata_sha,
    };

    let mut exported_entries = 0usize;
    let mut exported_files = 0usize;

    let committed = switch_to(context, "hitch-metadata", || -> Result<bool> {
        fs::create_dir_all(METADATA_RR_ENTRIES)?;

        // Copy rr-cache/<id>/** -> hitch/rr-cache/entries/<id>/**
        let entry_dirs = list_entry_dirs(&rr_cache)?;
        for entry_id in &entry_dirs {
            exported_entries += 1;
            let src_dir = rr_cache.join(entry_id);
            let dst_dir = PathBuf::from(METADATA_RR_ENTRIES).join(entry_id);
            exported_files += copy_dir_recursive(&src_dir, &dst_dir)?;
        }

        // Update manifest index.json
        let mut index = load_index_from_disk(Path::new(METADATA_RR_INDEX)).unwrap_or_default();
        for entry_id in entry_dirs {
            let dst_dir = PathBuf::from(METADATA_RR_ENTRIES).join(&entry_id);
            let size = dir_size_bytes(&dst_dir).unwrap_or(0);
            index
                .entries
                .entry(entry_id)
                .and_modify(|e| {
                    e.size_bytes = size;
                    e.exported_at = now;
                    e.contexts.push(ctx.clone());
                })
                .or_insert_with(|| RerereEntry {
                    size_bytes: size,
                    created_at: now,
                    exported_at: now,
                    contexts: vec![ctx.clone()],
                    last_used_at: None,
                });
        }

        fs::create_dir_all(METADATA_RR_ROOT)?;
        let json = serde_json::to_string_pretty(&index)?;
        fs::write(METADATA_RR_INDEX, json)?;

        // Commit changes (noop is ok). rr-cache paths may be ignored by hitch-metadata's
        // .gitignore in older repos, so force-add to ensure they are committed.
        let _ = context
            .git()
            .run_git_command(&["add", "-f", METADATA_RR_ROOT])?;
        let msg = "Hitch: update shared conflict resolutions (rerere)";
        let out = context
            .git()
            .run_git_command(&["commit", "--no-verify", "-m", msg])?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{}\n{}", stdout, stderr);
        let committed = out.status.success()
            || combined.contains("nothing to commit")
            || combined.contains("nothing added to commit");

        if committed && context.should_push() {
            let _ = context.git().push_branch("hitch-metadata");
        }

        Ok(out.status.success())
    })?;

    Ok(ExportSummary {
        exported_files,
        exported_entries,
        committed,
    })
}

pub fn load_shared_rerere_index(context: &GlobalContext) -> Result<Option<RerereIndex>> {
    check_metadata_health(context)?;

    for candidate in ["origin/hitch-metadata", "hitch-metadata"] {
        match context
            .git()
            .read_file_from_branch(candidate, METADATA_RR_INDEX)
        {
            Ok(s) => {
                let idx: RerereIndex =
                    serde_json::from_str(&s).context("Failed to parse rerere index.json")?;
                return Ok(Some(idx));
            }
            Err(_) => continue,
        }
    }

    Ok(None)
}

#[derive(Debug, Clone)]
pub struct PruneCandidate {
    pub entry_id: String,
    pub size_bytes: u64,
    pub contexts: usize,
    pub all_context_branches_missing: bool,
}

pub fn compute_prune_candidates(
    context: &GlobalContext,
    index: &RerereIndex,
) -> Vec<PruneCandidate> {
    let mut out: Vec<PruneCandidate> = index
        .entries
        .iter()
        .map(|(id, entry)| {
            let missing = entry
                .contexts
                .iter()
                .all(|c| context_all_branches_missing(context, c));
            PruneCandidate {
                entry_id: id.clone(),
                size_bytes: entry.size_bytes,
                contexts: entry.contexts.len(),
                all_context_branches_missing: missing,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.all_context_branches_missing
            .cmp(&a.all_context_branches_missing)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
            .then_with(|| a.entry_id.cmp(&b.entry_id))
    });

    out
}

pub fn prune_shared_rerere_cache(context: &GlobalContext, max_size_mb: u64) -> Result<()> {
    check_metadata_health(context)?;

    let max_bytes = max_size_mb.saturating_mul(1024 * 1024);

    let mut index = match load_shared_rerere_index(context)? {
        Some(i) => i,
        None => {
            context.log_info("No shared rerere cache found in hitch-metadata.");
            return Ok(());
        }
    };

    let total_bytes: u64 = index.entries.values().map(|e| e.size_bytes).sum();
    if total_bytes <= max_bytes {
        context.log_info(&format!(
            "Shared rerere cache size is already under cap ({} bytes ≤ {} bytes).",
            total_bytes, max_bytes
        ));
        return Ok(());
    }

    let candidates = compute_prune_candidates(context, &index);

    let mut running = total_bytes;
    let mut removed: Vec<String> = Vec::new();
    for c in candidates {
        if running <= max_bytes {
            break;
        }
        running = running.saturating_sub(c.size_bytes);
        index.entries.remove(&c.entry_id);
        removed.push(c.entry_id);
    }

    if removed.is_empty() {
        context.log_info("No entries eligible for pruning under current rules.");
        return Ok(());
    }

    switch_to(context, "hitch-metadata", || -> Result<()> {
        for id in &removed {
            let dir = PathBuf::from(METADATA_RR_ENTRIES).join(id);
            if dir.exists() {
                let _ = fs::remove_dir_all(&dir);
            }
        }
        let json = serde_json::to_string_pretty(&index)?;
        fs::create_dir_all(METADATA_RR_ROOT)?;
        fs::write(METADATA_RR_INDEX, json)?;

        let _ = context
            .git()
            .run_git_command(&["add", "-f", METADATA_RR_ROOT])?;
        let msg = format!(
            "Hitch: prune shared conflict resolutions (cap {}MB)",
            max_size_mb
        );
        let _ = context
            .git()
            .run_git_command(&["commit", "--no-verify", "-m", &msg])?;
        if context.should_push() {
            let _ = context.git().push_branch("hitch-metadata");
        }
        Ok(())
    })?;

    context.log_success(&format!(
        "Pruned {} rerere entr(y/ies); new manifest total ≈ {} bytes.",
        removed.len(),
        index.entries.values().map(|e| e.size_bytes).sum::<u64>()
    ));
    Ok(())
}

fn best_metadata_read_ref(context: &GlobalContext) -> String {
    if context.git().rev_parse("origin/hitch-metadata").is_ok() {
        "origin/hitch-metadata".to_string()
    } else {
        "hitch-metadata".to_string()
    }
}

fn branch_sha_opt(context: &GlobalContext, branch: &str) -> Option<String> {
    context.git().get_branch_commit_sha(branch).ok()
}

fn context_all_branches_missing(context: &GlobalContext, ctx: &RerereContext) -> bool {
    let mut branches = Vec::new();
    branches.push(ctx.base_branch.name.as_str());
    for b in &ctx.promoted_branches {
        branches.push(b.name.as_str());
    }
    if let Some(cb) = &ctx.conflict_branch {
        branches.push(cb.name.as_str());
    }

    branches
        .into_iter()
        .all(|b| !branch_exists_local_or_remote_tracking(context, b))
}

fn branch_exists_local_or_remote_tracking(context: &GlobalContext, branch: &str) -> bool {
    context.git().branch_exists(branch).ok().unwrap_or(false)
        || context
            .git()
            .rev_parse(&format!("refs/remotes/origin/{}", branch))
            .is_ok()
}

fn load_index_from_disk(path: &Path) -> Result<RerereIndex> {
    let s = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

fn list_entry_dirs(rr_cache: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(rr_cache).with_context(|| format!("read_dir {:?}", rr_cache))? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        out.push(name);
    }
    out.sort();
    Ok(out)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<usize> {
    let mut files = 0usize;
    if !src.exists() {
        return Ok(0);
    }
    fs::create_dir_all(dst)?;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let ft = ent.file_type()?;
        let name = ent.file_name();
        let src_path = ent.path();
        let dst_path = dst.join(name);
        if ft.is_dir() {
            files += copy_dir_recursive(&src_path, &dst_path)?;
        } else if ft.is_file() {
            let bytes = fs::read(&src_path)?;
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dst_path, bytes)?;
            files += 1;
        }
    }
    Ok(files)
}

fn dir_size_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0u64;
    if !dir.exists() {
        return Ok(0);
    }
    for ent in fs::read_dir(dir)? {
        let ent = ent?;
        let ft = ent.file_type()?;
        let p = ent.path();
        if ft.is_dir() {
            total += dir_size_bytes(&p)?;
        } else if ft.is_file() {
            total += ent.metadata()?.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrip() {
        let mut idx = RerereIndex::default();
        idx.entries.insert(
            "abc".to_string(),
            RerereEntry {
                size_bytes: 12,
                created_at: Utc::now(),
                exported_at: Utc::now(),
                contexts: vec![RerereContext {
                    env_name: "dev".to_string(),
                    base_branch: BranchRef {
                        name: "main".to_string(),
                        sha: Some("deadbeef".to_string()),
                    },
                    promoted_branches: vec![],
                    conflict_branch: None,
                    metadata_sha: None,
                }],
                last_used_at: None,
            },
        );

        let s = serde_json::to_string(&idx).unwrap();
        let back: RerereIndex = serde_json::from_str(&s).unwrap();
        assert!(back.entries.contains_key("abc"));
    }
}
