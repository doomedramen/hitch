# Unify Publish Atomicity Across rebuild/release/resolve Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `hitch release` and `hitch resolve` the same crash-safe, single-ref-transaction publish path `hitch rebuild` already has, and add self-describing attribution + a per-branch generation counter to every publish record so a wedged environment is diagnosable without log spelunking.

**Architecture:** Extract the transactional core of `publish_environment_build` (`src/utils/prelude.rs`) into a reusable `publish_branch` function. Retrofit `release` and `resolve` to call it instead of their current two-step `publish_journal::record` + `update_ref_cas` sequence, which is documented in `AGENTS.md` as a real, un-recovered-from crash window (the journal is cleared unconditionally right after resync, before the push is even attempted, rather than after the push resolves). Add crash-fuzz differential tests for both, mirroring `tests/integration/crash_recovery_tests.rs`'s existing pattern for `rebuild`. Separately, extend `PublishRecord` with attribution (pid/hostname/timestamp) and a monotonic per-branch generation counter, surfaced through `hitch doctor`.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, the existing `GitOperations` plumbing layer, `HitchTestFramework` integration-test harness.

## Global Constraints

- Every new git/gh subprocess spawn point must go through `GitOperations::git_command` (or an already-blessed builder); a new raw `Command::new` requires `#[allow(clippy::disallowed_methods)]` with a one-line reason, matching `clippy.toml`'s existing policy.
- `just format`, `just format-check && just lint`, and `just test` must all pass clean before any task is considered done. `just lint` runs with `-D warnings` — a new clippy warning is a hard failure.
- Do not change the `expected_old: Some(String::new())` vs `None` semantics on `RefEdit::Update` — that CAS distinction is load-bearing (see `AGENTS.md`'s Conventions section and the regression test `publish_survives_leftover_publish_journal_ref` in `prelude.rs`).
- Existing crash-fuzz tests (`tests/integration/crash_recovery_tests.rs`) must continue passing unchanged after Task 2's refactor — they are the regression oracle proving the extraction preserved `rebuild`'s behavior exactly.
- User-facing error strings should keep a concrete next-step instruction (`hitch rebuild <env>`, `hitch push <branch> -f`, ...), matching the existing error-message convention documented in `AGENTS.md`.

---

### Task 1: Attribution and generation counter on `PublishRecord`

**Files:**
- Modify: `src/utils/publish_journal.rs:35-153` (struct, `record_blob`, `record`, `list`, plus new helper)
- Modify: `src/utils/prelude.rs` (the `publish_environment_build` call site that builds a `PublishRecord` literal)
- Modify: `src/commands/release.rs:331-345` (its `PublishRecord` literal)
- Modify: `src/commands/resolve.rs:403-417` (its `PublishRecord` literal)
- Modify: `src/commands/doctor.rs` (new check surfacing pending records)
- Test: `src/utils/publish_journal.rs` (in-file `#[cfg(test)] mod tests`, matching the existing pattern in `src/utils/rebuild_lock.rs` and `src/commands/doctor.rs`)
- Modify: `Cargo.toml` (new dependency)

**Interfaces:**
- Produces: `PublishAttribution { pid: u32, hostname: String, started_at: String }`, and `PublishRecord.generation: u64` / `PublishRecord.initiated_by: Option<PublishAttribution>`. `PublishRecord` gains `#[derive(Default)]`.
- Produces: `publish_journal::record_blob(git: &GitOperations, pending: &PublishRecord) -> Result<(String, String)>` — **signature changes** from `&GlobalContext` to `&GitOperations` (it never used anything else from `GlobalContext`), which is what makes it unit-testable without a full hitch environment. `publish_journal::record(context: &GlobalContext, ...)` keeps its existing signature and now calls `record_blob(context.git(), pending)`.
- Consumes: nothing new from earlier tasks (this task is independent of Tasks 2-6 and can land first or in parallel).

- [ ] **Step 1: Add the `hostname` dependency**

Edit `Cargo.toml`, in the `[dependencies]` block, add a new line after the `git2 = "0.20"` entry:

```toml
# Machine name for self-describing publish-journal records (`hitch doctor`
# attribution), so a wedged publish can be traced to the box that made it
# without correlating logs by hand.
hostname = "0.4"
```

- [ ] **Step 2: Write the failing test**

Add to the bottom of `src/utils/publish_journal.rs`:

```rust
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
        let git = GitOperations::new_at_path(dir.path()).unwrap();
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
        assert_eq!(second.len(), 1, "same branch, same ref name — overwrites in place");
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
            assert_eq!(record.generation, 1, "each branch starts its own count at 1");
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p hitch publish_journal::tests -- --nocapture`
Expected: compile error — `PublishAttribution` doesn't exist, `PublishRecord` has no `generation`/`initiated_by` fields, `record_blob` still takes `&GlobalContext`.

- [ ] **Step 4: Add the struct and fields**

In `src/utils/publish_journal.rs`, replace the `PublishRecord` struct (lines 46-67) with:

```rust
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
}
```

- [ ] **Step 5: Change `record_blob`'s signature and implement generation lookup**

Replace lines 101-111 (the doc comment and `record_blob` function) with:

```rust
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
```

- [ ] **Step 6: Update `record` and `list`'s callers to the new `record_blob` signature**

In `src/utils/publish_journal.rs`, replace the `record` function (lines 116-119) with:

```rust
/// Record the intent to move `branch` and resync `checkouts`, before anything
/// observable changes. Blob-backed so the payload is readable with
/// `git cat-file -p` when diagnosing by hand.
pub fn record(context: &GlobalContext, pending: &PublishRecord) -> Result<()> {
    let (refname, blob) = record_blob(context.git(), pending)?;
    context.git().update_ref(&refname, &blob)
}
```

`list`'s own signature (`pub fn list(git: &GitOperations) -> Result<Vec<(String, PublishRecord)>>`) is unchanged — `next_generation` above just calls it directly with the `git` parameter it already has.

- [ ] **Step 7: Update the three external call sites**

In `src/utils/prelude.rs`, inside `publish_environment_build`, find:

```rust
    let (resync_ref, resync_blob) = crate::utils::publish_journal::record_blob(
        context,
        &crate::utils::publish_journal::PublishRecord {
            branch: env_name.to_string(),
            from_sha: old_env_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
            push_owed: context.should_push(),
        },
    )?;
```

Replace with:

```rust
    let (resync_ref, resync_blob) = crate::utils::publish_journal::record_blob(
        context.git(),
        &crate::utils::publish_journal::PublishRecord {
            branch: env_name.to_string(),
            from_sha: old_env_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
            push_owed: context.should_push(),
            ..Default::default()
        },
    )?;
```

In `src/commands/release.rs:331-345`, apply the same two changes: `context` → `context.git()` as the first argument, and add `..Default::default()` as the literal's last field before the closing brace.

In `src/commands/resolve.rs:403-417`, apply the same two changes.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p hitch publish_journal::tests -- --nocapture`
Expected: PASS, 3 tests.

Run: `cargo build -p hitch` and `cargo test -p hitch` to confirm the three call-site edits compile and nothing else broke.
Expected: clean build, full suite green.

- [ ] **Step 9: Surface attribution and generation in `hitch doctor`**

In `src/commands/doctor.rs`, add a new function and wire it into `run`:

```rust
use crate::utils::{gh, publish_journal, resolutions};
```

(replace the existing `use crate::utils::{gh, resolutions};` import line with the one above)

Add, after `check_resolution_debt` in the function body (in `run`, right after the `check_resolution_debt(context, args.max_resolution_age_days)?;` line):

```rust
    check_pending_publishes(context)?;
```

Add the new function itself, near `check_resolution_debt`:

```rust
/// Report every unresolved publish-journal record — a publish that moved a
/// branch but never finished resyncing or pushing. Each is attributed to the
/// process that made it, so a wedge is traceable without correlating logs.
fn check_pending_publishes(context: &GlobalContext) -> Result<()> {
    let pending = publish_journal::list(context.git())?;
    if pending.is_empty() {
        context.log_verbose("No pending publish-journal records.");
        return Ok(());
    }

    context.log_warning(&format!(
        "{} unresolved publish record(s) — a previous publish did not finish:",
        pending.len()
    ));
    for (_, record) in &pending {
        let who = record
            .initiated_by
            .as_ref()
            .map(|a| format!("pid {} on {} (started {})", a.pid, a.hostname, a.started_at))
            .unwrap_or_else(|| "unknown process".to_string());
        context.log_warning(&format!(
            "  '{}' generation {} — {} — from {} to {}{}",
            record.branch,
            record.generation,
            who,
            record.from_sha.as_deref().unwrap_or("(none)"),
            record.to_sha,
            if record.push_owed { ", push still owed" } else { "" }
        ));
    }
    context.log_info(
        "Recovery for these runs automatically on the next mutating hitch command. If one \
         keeps reappearing, the process attributed above is worth checking.",
    );

    Ok(())
}
```

- [ ] **Step 10: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 11: Commit**

```bash
git add Cargo.toml Cargo.lock src/utils/publish_journal.rs src/utils/prelude.rs src/commands/release.rs src/commands/resolve.rs src/commands/doctor.rs
git commit -m "feat: attribute publish-journal records and track a per-branch generation counter"
```

---

### Task 2: Extract `publish_branch`, refactor `rebuild`'s publish onto it

**Files:**
- Modify: `src/utils/prelude.rs:1022-1233` (`publish_environment_build`)
- Test: `tests/integration/crash_recovery_tests.rs` (no changes expected — this is the regression oracle)

**Interfaces:**
- Consumes: `publish_journal::record_blob(git: &GitOperations, ...)` from Task 1.
- Produces: `pub(crate) fn publish_branch(context: &GlobalContext, branch: &str, new_sha: &str, backup_timestamp: Option<&str>, retry_hint: &str, push: impl FnOnce() -> Result<()>) -> Result<()>` — the shared transactional publish core Tasks 3 and 5 will call.

- [ ] **Step 1: Read `publish_environment_build` in full before editing**

Open `src/utils/prelude.rs:1022-1233` and confirm it still matches the version quoted in this plan's research (the function signature, the `ref_transaction` call, the `maybe_abort_for_test` call sites, and the push confirmation block). If it has drifted, adapt the following steps to the current code rather than blindly applying the diff.

- [ ] **Step 2: Write the new `publish_branch` function**

Add this new function to `src/utils/prelude.rs`, near `publish_environment_build` (e.g. immediately before it):

```rust
/// Land `new_sha` on `branch` in one atomic ref transaction, resync every
/// attached checkout, then push if configured — recoverable at every step
/// via `publish_journal`.
///
/// This is the transactional core `hitch rebuild` already used (previously
/// inlined in `publish_environment_build`). `hitch release` and `hitch
/// resolve` retrofit onto it instead of hand-rolling their own
/// record-then-CAS-then-clear sequence, which is what left them with a real,
/// undefended crash window between the CAS landing and the eventual push —
/// see the "Only `publish_environment_build`..." note in `AGENTS.md`.
///
/// `backup_timestamp`, when `Some`, also archives the pre-publish tip under
/// `refs/hitch/prev/<branch>/<timestamp>` and
/// `refs/hitch/backup/<branch>/<timestamp>` — pass `None` for a caller that
/// doesn't want that (e.g. one that already writes its own tag as the
/// rollback anchor).
///
/// `push` runs only after the ref has moved and checkouts have resynced. Its
/// error is not propagated as this function's error — a push failure is
/// reported to the user, not fatal to an already-successful local publish —
/// but it does gate whether the journal record survives this call: a failed
/// or undone push leaves the record in place so the next mutating command's
/// `publish_journal::recover()` finds it and reports it again.
pub(crate) fn publish_branch(
    context: &GlobalContext,
    branch: &str,
    new_sha: &str,
    backup_timestamp: Option<&str>,
    retry_hint: &str,
    push: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let branch_ref = format!("refs/heads/{}", branch);
    let old_sha = context.git().rev_parse_opt(&branch_ref)?;

    let checkouts = scan_checkouts_on_branch(context, branch)?;

    let (resync_ref, resync_blob) = crate::utils::publish_journal::record_blob(
        context.git(),
        &crate::utils::publish_journal::PublishRecord {
            branch: branch.to_string(),
            from_sha: old_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
            push_owed: context.should_push(),
            ..Default::default()
        },
    )?;
    crate::utils::publish_journal::maybe_abort_for_test("journal-written");

    let mut edits = vec![
        crate::utils::git_operations::RefEdit::Update {
            refname: resync_ref,
            new_oid: resync_blob,
            expected_old: Some(String::new()),
        },
        crate::utils::git_operations::RefEdit::Update {
            refname: branch_ref.clone(),
            new_oid: new_sha.to_string(),
            expected_old: old_sha.clone(),
        },
    ];

    if let (Some(ref old), Some(timestamp)) = (&old_sha, backup_timestamp) {
        edits.push(crate::utils::git_operations::RefEdit::Update {
            refname: format!("refs/hitch/prev/{}/{}", branch, timestamp),
            new_oid: old.clone(),
            expected_old: Some(String::new()),
        });
        edits.push(crate::utils::git_operations::RefEdit::Update {
            refname: format!("refs/hitch/backup/{}/{}", branch, timestamp),
            new_oid: old.clone(),
            expected_old: Some(String::new()),
        });
    }

    if let Err(e) = context
        .git()
        .ref_transaction(&edits, &format!("hitch: publish {}", branch))
    {
        return Err(anyhow::anyhow!(
            "Failed to publish '{}': {}. Most commonly another publish landed first and \
             moved '{}' out from under this one's compare-and-swap. Fetch and re-run \
             '{}'.",
            branch,
            e,
            branch,
            retry_hint
        ));
    }
    crate::utils::publish_journal::maybe_abort_for_test("ref-moved");

    if let Some(ref old_sha) = old_sha {
        context.log_verbose(&format!(
            "✓ Published '{}' ({} -> {})",
            branch, old_sha, new_sha
        ));
    } else {
        context.log_verbose(&format!("✓ Published '{}' ({})", branch, new_sha));
    }

    resync_checkouts(context, branch, new_sha, &checkouts);
    crate::utils::publish_journal::maybe_abort_for_test("resync-done");

    if !context.should_push() {
        crate::utils::publish_journal::clear(context, branch);
        return Ok(());
    }

    match push() {
        Ok(()) => {
            crate::utils::publish_journal::maybe_abort_for_test("push-succeeded");
            let _ = crate::utils::publish_journal::mark_push_done(context, branch);
            crate::utils::publish_journal::clear(context, branch);
        }
        Err(e) => {
            context.log_warning(&format!(
                "'{}' was published locally, but pushing to origin failed: {}",
                branch, e
            ));
            context.log_warning(&format!(
                "The publish record was left in place; the next mutating hitch command \
                 will report it again until it's resolved. Push manually with: hitch push \
                 {} -f",
                branch
            ));
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Refactor `publish_environment_build` to call `publish_branch`**

Replace the body of `publish_environment_build` (everything from `let env_ref = format!(...)` through the closing `Ok(())` before `update_rebuilt_timestamp_for_rebuild`) with a call to `publish_branch`, keeping the confirmation-prompt UI and force-push-with-lease behavior as the `push` closure:

```rust
pub(crate) fn publish_environment_build(
    context: &GlobalContext,
    env_name: &str,
    new_sha: &str,
    backup_timestamp: &str,
    remote_sha_before: &Option<String>,
) -> Result<()> {
    let retry_hint = format!("hitch rebuild {}", env_name);

    publish_branch(
        context,
        env_name,
        new_sha,
        Some(backup_timestamp),
        &retry_hint,
        || {
            if !context.confirm(&format!(
                "Ready to force push the rebuilt '{}' branch to 'origin/{}'.\n\
                 This will OVERWRITE the remote '{}' branch with the new rebuilt version.\n\
                 This action cannot be undone.\n\
                 Do you want to proceed?",
                env_name, env_name, env_name
            ))? {
                context.log_info(&format!(
                    "Skipping remote replacement for '{}' branch. The local '{}' branch \
                     has been rebuilt. To push manually, run: hitch push {} -f",
                    env_name, env_name, env_name
                ));
                // A declined push is not a failure — the local publish already
                // succeeded. Report it as success to `publish_branch` so the
                // journal record clears; there is nothing left to retry.
                return Ok(());
            }

            context.log_info(&format!(
                "Force pushing rebuilt '{}' branch to replace remote",
                env_name
            ));
            force_push_with_deploy_key_if_configured(context, env_name, remote_sha_before).map_err(
                |e| {
                    anyhow::anyhow!(
                        "Failed to force push rebuilt '{}' branch: {}. Someone may have \
                         pushed to '{}' while this rebuild ran, or the deploy key may be \
                         missing/outdated. Fetch and re-run 'hitch rebuild {}', or push once \
                         you've confirmed it's safe to overwrite: hitch push {} -f",
                        env_name, e, env_name, env_name, env_name
                    )
                },
            )
        },
    )?;

    update_rebuilt_timestamp_for_rebuild(context, env_name)?;
    Ok(())
}
```

Note the behavior change this introduces on purpose: previously, declining the push prompt still logged success at the `force_push_with_deploy_key_if_configured` level; now a decline is treated as `Ok(())` inside the closure so `publish_branch` clears the journal record (a declined push isn't an error to retry — the user explicitly chose not to push). Confirm this matches `publish_branch`'s doc comment intent before moving on.

- [ ] **Step 4: Run the existing crash-fuzz suite as the regression oracle**

Run: `cargo test -p hitch --test integration crash_recovery`
Expected: PASS — `test_publish_converges_after_abort_at_each_step`, `test_publish_converges_after_abort_at_push_succeeded`, `test_publish_journal_persists_owed_push_until_resolved` all still green. If any fails, the refactor changed rebuild's observable behavior — fix `publish_branch`/its call site until they pass again before proceeding; do not weaken the tests.

- [ ] **Step 5: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 6: Commit**

```bash
git add src/utils/prelude.rs
git commit -m "refactor: extract publish_branch as a reusable atomic-publish core"
```

---

### Task 3: Retrofit `hitch release` onto `publish_branch`

**Files:**
- Modify: `src/commands/release.rs:328-397` (`perform_release_core`'s publish sequence)

**Interfaces:**
- Consumes: `crate::utils::prelude::publish_branch` from Task 2.

- [ ] **Step 1: Read the current `perform_release_core` publish/push block before editing**

Confirm `src/commands/release.rs:328-397` still matches this plan's research (the `record`/`update_ref_cas`/`resync_checkouts`/`clear` sequence, followed by the tag push and `cleanup()` call). Adapt if it has drifted.

- [ ] **Step 2: Replace the publish/push block**

Replace lines 328-397 (from the `// Publish the target branch atomically with CAS.` comment through the closing `}` of the `if context.should_push()` block, but *not* the trailing `cleanup();` call, which stays) with:

```rust
    // Publish the target branch atomically — see `prelude::publish_branch`.
    // Not versioned into `refs/hitch/prev`/`backup`: the release tag created
    // above already serves as this publish's rollback anchor.
    let retry_hint = format!("hitch release {}", env_name);
    crate::utils::prelude::publish_branch(context, target_branch, &new_sha, None, &retry_hint, || {
        push_branch_with_deploy_key_if_configured(context, target_branch)?;
        context.git().push_tag(&tag_name)
    })?;
```

- [ ] **Step 3: Check the imports still resolve**

`src/commands/release.rs`'s existing `use` block already imports `push_branch_with_deploy_key_if_configured` (line 6) — confirm it's still referenced (it now is, inside the closure). `crate::utils::prelude::scan_checkouts_on_branch`/`checkout_paths`/`resync_checkouts` and the direct `publish_journal::record`/`update_ref_cas`/`clear` calls this block used to make are no longer needed here — leave their imports alone if other functions in this file still use them, remove only if `cargo build` flags them as now-unused.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p hitch --test integration release`
Expected: PASS, all existing `hitch release` integration tests unchanged in behavior. If a test asserted on the old two-step publish's exact log wording, update the assertion to match the new (functionally equivalent) log line from `publish_branch`.

- [ ] **Step 5: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 6: Commit**

```bash
git add src/commands/release.rs
git commit -m "fix: retrofit hitch release onto the atomic publish_branch core"
```

---

### Task 4: Crash-fuzz test for `hitch release`

**Files:**
- Create: `tests/integration/release_crash_recovery_tests.rs`
- Modify: `tests/integration/mod.rs` (or wherever integration test modules are registered — check how `crash_recovery_tests` is registered and mirror it)

**Interfaces:**
- Consumes: `HITCH_TEST_ABORT_AFTER` env var support from `publish_journal::maybe_abort_for_test`, now exercised by `release` via Task 2/3's `publish_branch`.

- [ ] **Step 1: Find how `crash_recovery_tests` is registered as a module**

Run: `grep -rn "crash_recovery_tests" tests/` and identify the `mod crash_recovery_tests;` declaration (likely in `tests/integration/mod.rs` or a top-level `tests/integration.rs`). Note the exact pattern to mirror for the new file.

- [ ] **Step 2: Write the failing test**

Create `tests/integration/release_crash_recovery_tests.rs`:

```rust
//! Crash-fuzz for `hitch release`, mirroring `crash_recovery_tests.rs`'s
//! differential pattern now that `release` publishes through the same
//! `publish_branch` core as `rebuild`.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// One environment with a promoted branch, ready to release to `main`.
    fn setup(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.git
            .run(&["checkout", "-b", "feature-1"])?
            .assert_success();
        env.fs.write_file("1.txt", "one")?;
        env.git.run(&["add", "."])?.assert_success();
        env.git
            .run(&["commit", "-m", "feature 1"])?
            .assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();

        env.hitch
            .run()
            .args(&["promote", "feature-1", "dev"])
            .execute()?
            .assert_success();

        // Stand on 'main' before releasing, so the resync path has real work.
        env.git.run(&["checkout", "main"])?.assert_success();
        Ok(())
    }

    fn tree_oid(env: &TestEnvironment, branch: &str) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&["rev-parse", &format!("refs/heads/{}^{{tree}}", branch)])?
            .stdout()
            .trim()
            .to_string())
    }

    #[test]
    fn test_release_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;
                env.hitch
                    .run()
                    .args(&["release", "dev"])
                    .execute()?
                    .assert_success();
                tree = tree_oid(env, "main")?;
                Ok::<(), anyhow::Error>(())
            });
            tree
        };
        assert!(!expected_tree.is_empty(), "the oracle run produced no tree");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;

                let interrupted = env
                    .hitch
                    .run()
                    .args(&["release", "dev"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute()?;
                assert!(
                    !interrupted.success(),
                    "aborting after '{}' should have crashed hitch release, but it exited \
                     successfully.\nstdout: {}\nstderr: {}",
                    abort_after,
                    interrupted.stdout(),
                    interrupted.stderr()
                );

                // Recovery needs --force: the crash always skips with_locked_env's
                // unlock-on-exit (see AGENTS.md's crash-recovery gotcha).
                env.hitch
                    .run()
                    .args(&["unlock", "dev"])
                    .execute()?
                    .assert_success();

                env.hitch
                    .run()
                    .args(&["release", "dev"])
                    .execute()?
                    .assert_success();

                let actual_tree = tree_oid(env, "main")?;
                assert_eq!(
                    actual_tree, expected_tree,
                    "release aborted after '{}' did not converge to the oracle's tree",
                    abort_after
                );

                let journal =
                    env.git
                        .run(&["for-each-ref", "refs/hitch/publish/main"])?
                        .stdout()
                        .trim()
                        .to_string();
                assert!(
                    journal.is_empty(),
                    "a publish-journal record for 'main' survived recovery after abort \
                     at '{}'",
                    abort_after
                );

                Ok::<(), anyhow::Error>(())
            });
        }

        Ok(())
    }
}
```

- [ ] **Step 3: Register the module**

Add `mod release_crash_recovery_tests;` to the same registration point found in Step 1, next to `mod crash_recovery_tests;`.

- [ ] **Step 4: Run the test to verify it fails (or clarify what breaks)**

Run: `cargo test -p hitch --test integration test_release_converges_after_abort_at_each_step -- --nocapture`
Expected: at this point Tasks 2-3 are already done, so this should largely pass on the first try; if it fails, read the failure carefully — it may reveal `release`'s `--force`/lock handling doesn't line up with `rebuild`'s the way this test assumes (e.g. `hitch release` may not require a manual `unlock` the same way `rebuild --force` does — check `src/commands/release.rs`'s `args.force` handling from the earlier research and adjust the test's recovery step to call `hitch release dev --force` instead of `unlock` + plain `release`, whichever this codebase's actual lock-recovery convention turns out to be for `release`).

- [ ] **Step 5: Fix forward until it passes**

Adjust the test (not `publish_branch`, unless Step 4 revealed a genuine gap in Task 2/3's implementation) until it passes.

- [ ] **Step 6: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 7: Commit**

```bash
git add tests/integration/release_crash_recovery_tests.rs tests/integration/mod.rs
git commit -m "test: add crash-fuzz coverage for hitch release"
```

---

### Task 5: Retrofit `hitch resolve`'s `finish_mode_a` onto `publish_branch`

**Files:**
- Modify: `src/commands/resolve.rs:379-474` (`finish_mode_a`)

**Interfaces:**
- Consumes: `crate::utils::prelude::publish_branch` from Task 2.

- [ ] **Step 1: Read the current `finish_mode_a` publish block before editing**

Confirm `src/commands/resolve.rs:403-440` still matches this plan's research (the `record`/`update_ref_cas`-with-error-mapper-that-also-clears/`resync_checkouts`/`clear` sequence). Adapt if it has drifted.

- [ ] **Step 2: Replace the publish block**

Replace lines 403-440 with:

```rust
    let retry_hint = format!("hitch resolve {} --branch {} --continue", env_name, branch);
    crate::utils::prelude::publish_branch(context, branch, new_sha, None, &retry_hint, || {
        if context.should_push() {
            crate::utils::prelude::push_branch_with_deploy_key_if_configured(context, branch)
        } else {
            Ok(())
        }
    })?;
```

Adjust `retry_hint`'s exact flag names to whatever `finish_mode_a`'s actual CLI surface is (check `src/commands/resolve.rs`'s `clap::Args` struct for the real flag names before finalizing this string — the plan's research did not capture the full `ResolveCommand` struct, only `finish_mode_a`'s internal signature).

- [ ] **Step 3: Confirm `push_branch_with_deploy_key_if_configured` is reachable from `resolve.rs`**

It's `pub(crate)` in `src/utils/prelude.rs` per Task 2's research — check whether `resolve.rs` already imports it (it may not, since the old code didn't push at all in this path, or pushed via a different helper). Add the import if missing.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p hitch --test integration resolve`
Expected: PASS, all existing `hitch resolve` Mode A integration tests unchanged in behavior.

- [ ] **Step 5: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 6: Commit**

```bash
git add src/commands/resolve.rs
git commit -m "fix: retrofit hitch resolve's Mode A publish onto the atomic publish_branch core"
```

---

### Task 6: Crash-fuzz test for `hitch resolve`

**Files:**
- Create: `tests/integration/resolve_crash_recovery_tests.rs`
- Modify: `tests/integration/mod.rs`

**Interfaces:**
- Consumes: `HITCH_TEST_ABORT_AFTER` support in `resolve`'s Mode A path, now exercised via Task 5's `publish_branch` call.

**Grounding:** `tests/integration/resolve_tests.rs:13-120` (`test_resolve_mode_a_rebases_without_touching_the_users_checkout`) is the real Mode A pattern this task reuses: create a `shared.txt` conflict between `branch-a` and `main`, promote `branch-a` to `dev` with `--no-rebuild`, run `hitch resolve dev` (pauses in a disposable worktree), fetch the worktree path with `hitch resolve dev --branch branch-a --path`, resolve the conflict there with plain git + `git rebase --continue`, then land it with `hitch resolve dev --branch branch-a --continue` — that last command is what runs `finish_mode_a` and is what this task interrupts.

- [ ] **Step 1: Write the failing test**

Create `tests/integration/resolve_crash_recovery_tests.rs`:

```rust
//! Crash-fuzz for hitch resolve's Mode A publish, mirroring
//! crash_recovery_tests.rs's differential pattern now that resolve publishes
//! through the same publish_branch core as rebuild/release.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Produce a real Mode A conflict (branch-a vs main on shared.txt),
    /// promote branch-a to dev, pause the guided rebase in its worktree, and
    /// resolve the conflict there with plain git — everything up to but not
    /// including the final 'hitch resolve dev --branch branch-a --continue'
    /// that lands it. Mirrors
    /// resolve_tests.rs::test_resolve_mode_a_rebases_without_touching_the_users_checkout.
    fn setup_paused_rebase(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.fs.write_file("shared.txt", "v1\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "base v1"])?;

        env.git.run(&["checkout", "-b", "branch-a"])?;
        env.fs.write_file("shared.txt", "from-branch-a\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git
            .run(&["commit", "-m", "branch-a: update shared.txt"])?;
        env.git.run(&["checkout", "main"])?;

        env.fs.write_file("shared.txt", "from-main-later\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "main: update shared.txt"])?;

        env.hitch
            .run()
            .args(&["promote", "branch-a", "dev", "--no-rebuild"])
            .execute()?
            .assert_success();

        env.hitch
            .run()
            .args(&["resolve", "dev"])
            .execute()?
            .assert_success();

        let session_path = env
            .hitch
            .run()
            .args(&["resolve", "dev", "--branch", "branch-a", "--path"])
            .execute()?
            .assert_success()
            .stdout()
            .trim()
            .to_string();
        let session = std::path::PathBuf::from(&session_path);
        let session_git = GitCommandRunner::new(&session)?;

        std::fs::write(session.join("shared.txt"), "resolved\n")?;
        session_git.run(&["add", "shared.txt"])?.assert_success();
        session_git
            .run(&["-c", "core.editor=true", "rebase", "--continue"])?
            .assert_success();

        Ok(())
    }

    fn tree_oid(env: &TestEnvironment, branch: &str) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&["rev-parse", &format!("refs/heads/{}^{{tree}}", branch)])?
            .stdout()
            .trim()
            .to_string())
    }

    #[test]
    fn test_resolve_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup_paused_rebase(env)?;
                env.hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .execute()?
                    .assert_success();
                tree = tree_oid(env, "branch-a")?;
                Ok::<(), anyhow::Error>(())
            });
            tree
        };
        assert!(!expected_tree.is_empty(), "the oracle run produced no tree");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup_paused_rebase(env)?;

                let interrupted = env
                    .hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute()?;
                assert!(
                    !interrupted.success(),
                    "aborting after '{}' should have crashed hitch resolve --continue, but \
                     it exited successfully.\nstdout: {}\nstderr: {}",
                    abort_after,
                    interrupted.stdout(),
                    interrupted.stderr()
                );

                // Re-running --continue is the documented recovery path: the
                // worktree's rebase already finished (git rebase --continue
                // succeeded before hitch's own publish step ran), so hitch
                // only needs to redo the publish half. If this assumption is
                // wrong for this codebase's actual resolve re-entry logic,
                // this step will fail loudly rather than silently pass —
                // adjust to the real recovery command src/commands/resolve.rs
                // expects here.
                env.hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .execute()?
                    .assert_success();

                let actual_tree = tree_oid(env, "branch-a")?;
                assert_eq!(
                    actual_tree, expected_tree,
                    "resolve aborted after '{}' did not converge to the oracle's tree",
                    abort_after
                );

                let journal = env
                    .git
                    .run(&["for-each-ref", "refs/hitch/publish/branch-a"])?
                    .stdout()
                    .trim()
                    .to_string();
                assert!(
                    journal.is_empty(),
                    "a publish-journal record for 'branch-a' survived recovery after abort \
                     at '{}'",
                    abort_after
                );

                Ok::<(), anyhow::Error>(())
            });
        }

        Ok(())
    }
}
```

- [ ] **Step 2: Register the module**

Find the exact registration pattern used for `crash_recovery_tests` (run `grep -rn "mod crash_recovery_tests" tests/` to locate it — likely `tests/integration/mod.rs`) and add `mod resolve_crash_recovery_tests;` next to it.

- [ ] **Step 3: Run the test to verify it fails, then fix forward**

Run: `cargo test -p hitch --test integration test_resolve_converges_after_abort_at_each_step -- --nocapture`

If the re-`--continue` recovery step in Step 1's test doesn't work as written (e.g. `finish_mode_a` expects different state after a crash than after a normal first call), read `src/commands/resolve.rs`'s Mode A entry logic to find the actual re-entry behavior and adjust the test's recovery call accordingly — this is a real discovery step, not just a mechanical retry.

- [ ] **Step 4: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 5: Commit**

```bash
git add tests/integration/resolve_crash_recovery_tests.rs tests/integration/mod.rs
git commit -m "test: add crash-fuzz coverage for hitch resolve's Mode A publish"
```
