//! Persistent state for in-progress conflict resolution
//!
//! When a rebuild merge conflicts, hitch writes a small JSON file to
//! `.git/hitch-resolve-state.json` — analogous to how git itself stores
//! transient state in `.git/` (MERGE_HEAD, CHERRY_PICK_HEAD, etc.).
//! This is intentionally NOT stored in the `hitch-metadata` branch because
//! it is local, machine-specific, and temporary.
//!
//! `hitch resolve` / `--continue` / `--abort` read this file to resume
//! exactly where the interrupted rebuild left off.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// File name written inside `.git/` when a rebuild conflict is in progress
const RESOLVE_STATE_FILE: &str = "hitch-resolve-state.json";

/// State saved when a rebuild is interrupted by a merge conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveState {
    /// The environment being rebuilt
    pub env_name: String,
    /// The temporary branch that is currently checked out with the conflict
    pub temp_branch: String,
    /// The branch the user was on before the rebuild started
    pub original_branch: String,
    /// The base branch for the environment
    pub base_branch: String,
    /// Promoted branches already squash-merged successfully before the conflict
    pub merged_so_far: Vec<String>,
    /// Branches still waiting to be merged (conflict branch is first in the list)
    pub remaining_branches: Vec<String>,
    /// The branch whose squash merge produced the conflict
    pub conflict_branch: String,
}

/// Return the path to `.git/hitch-resolve-state.json` for a given git dir
pub fn resolve_state_path(git_dir: &str) -> PathBuf {
    PathBuf::from(git_dir).join(RESOLVE_STATE_FILE)
}

/// Write resolve state to `.git/hitch-resolve-state.json`
pub fn write_resolve_state(git_dir: &str, state: &ResolveState) -> Result<()> {
    let path = resolve_state_path(git_dir);
    let json = serde_json::to_string_pretty(state).context("Failed to serialize resolve state")?;
    std::fs::write(&path, &json)
        .with_context(|| format!("Failed to write resolve state to {:?}", path))?;
    Ok(())
}

/// Read resolve state from `.git/hitch-resolve-state.json`
pub fn read_resolve_state(git_dir: &str) -> Result<ResolveState> {
    let path = resolve_state_path(git_dir);
    let json = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "No conflict in progress (could not read {:?}). \
             Run 'hitch rebuild <env>' to start a rebuild.",
            path
        )
    })?;
    serde_json::from_str(&json).context("Failed to parse resolve state — the file may be corrupted")
}

/// Remove resolve state file (called after --continue success or --abort)
pub fn remove_resolve_state(git_dir: &str) -> Result<()> {
    let path = resolve_state_path(git_dir);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove resolve state file {:?}", path))?;
    }
    Ok(())
}

/// Return true if a resolve state file exists in the given git dir
pub fn resolve_state_exists(git_dir: &str) -> bool {
    resolve_state_path(git_dir).exists()
}
