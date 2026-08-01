# Lineage Verification for Content-Addressed Trust Decisions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gap between "content matches" and "this is genuinely the same history" in hitch's two content-addressed trust decisions — recorded conflict-resolution replay and crash-recovery checkout repair — without weakening either mechanism's fast path or turning routine git housekeeping (reflog expiry) into false alarms.

**Architecture:** `src/utils/resolutions.rs`'s `ResolutionMeta.source_branch_head` already records the branch head a resolution was made against, but is documented as "audit only" and never checked at replay time — only the exact merge-stage blob-OID match (`res.meta.key == key`) gates replay. Enforce lineage there: `try_replay_resolution` (`src/utils/prelude.rs`) must additionally confirm the branch's current tip descends from `source_branch_head` before splicing recorded content into a build. Separately, `src/utils/publish_journal.rs`'s `repair_checkout` proves a checkout is safe to reset purely by tree-content equality (`matches_commit_exactly`) — add a positive-evidence-only reflog check (fails open when the reflog is empty/inconclusive, fails closed only on an actual contradiction) as defense in depth.

**Tech Stack:** Rust, `anyhow`, existing `GitOperations` plumbing, the existing `try_replay_resolution_tests` in-file test module in `src/utils/prelude.rs`.

## Global Constraints

- `just format`, `just format-check && just lint`, and `just test` must all pass clean before any task is considered done.
- The lineage check in `try_replay_resolution` must run *before* the `require_signed` gate (both are structural/identity checks that should short-circuit before the trust/signing question, matching the existing `res.meta.key != key` check's placement and rationale).
- The reflog check in `repair_checkout` must fail **open** (proceed with recovery) when the reflog is empty or missing — reflog expiry (`gc.reflogexpire`, default 90 days) and `core.logAllRefUpdates=false` are both routine, non-adversarial conditions. It fails **closed** (warn instead of reset) only on a positive contradiction: a non-empty reflog that does not contain `from_sha`. Do not invert this — a stricter design would turn ordinary git housekeeping into manual-intervention cases and regress a currently-working recovery path.
- Do not touch `resolutions::verify_resolution_signature`'s existing blob-OID re-verification (the `f0`/`f1`/... tree-entry-name-vs-content-hash check) — that is a separate, already-correct guard against a different attack (repointing a tree entry after signing), out of scope here.

---

### Task 1: Enforce `source_branch_head` lineage at resolution-replay time

**Files:**
- Modify: `src/utils/resolutions.rs:80-102` (`ResolutionMeta` doc comment on `source_branch_head`)
- Modify: `src/utils/prelude.rs:1271-1413` (`try_replay_resolution`)
- Test: `src/utils/prelude.rs:1415-1592` (existing `try_replay_resolution_tests` module)

**Interfaces:**
- Consumes: `GitOperations::get_merge_base(&self, branch1: &str, branch2: &str) -> Result<Option<String>>` and `GitOperations::rev_parse_opt(&self, reference: &str) -> Result<Option<String>>` (both already exist, unchanged).
- Produces: no new public interface — `try_replay_resolution`'s signature is unchanged; this task only adds an internal check.

- [ ] **Step 1: Update `source_branch_head`'s doc comment to describe its new, enforced role**

In `src/utils/resolutions.rs`, in the `ResolutionMeta` struct (lines 80-102), replace:

```rust
    /// The head SHA of `branch` at record time — audit only; correctness
    /// comes from the exact stage-OID match, not this.
    pub source_branch_head: String,
```

with:

```rust
    /// The head SHA of `branch` at record time. Enforced at replay time in
    /// `try_replay_resolution` (`src/utils/prelude.rs`): replay requires the
    /// branch's current tip to be this commit or a descendant of it, so a
    /// coincidental exact stage-OID match against an unrelated history (e.g.
    /// two branches independently making the same one-line edit to the same
    /// file) can't silently splice one branch's recorded resolution onto
    /// another's build.
    pub source_branch_head: String,
```

- [ ] **Step 2: Write the failing test**

In `src/utils/prelude.rs`, inside the existing `try_replay_resolution_tests` module (after `require_signed_check_fails_closed_on_metadata_health_error`, before the module's closing `}`), add:

```rust
    /// A recorded resolution whose stage OIDs happen to match the current
    /// conflict, but whose `source_branch_head` is not an ancestor of the
    /// branch's current tip, must be held rather than replayed — the exact
    /// stage-OID match alone isn't enough to prove this is the same conflict
    /// in the same history.
    #[test]
    fn replay_holds_when_source_branch_head_is_not_an_ancestor() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path();

        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.name", "Test User"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("README.md"), "hi\n")?;
        git(repo, &["add", "README.md"]);
        git(repo, &["commit", "-q", "-m", "init"]);
        let root = git(repo, &["rev-parse", "HEAD"]);

        // The commit the resolution claims to have been recorded against.
        git(repo, &["checkout", "-q", "-b", "recorded-against"]);
        std::fs::write(repo.join("other.txt"), "a\n")?;
        git(repo, &["add", "other.txt"]);
        git(repo, &["commit", "-q", "-m", "recorded-against tip"]);
        let source_branch_head = git(repo, &["rev-parse", "HEAD"]);

        // An unrelated commit off the same root, sharing no ancestry with
        // `recorded-against` beyond the root itself — this stands in for
        // `branch-b`'s *current* tip, which the resolution was never
        // recorded against.
        git(repo, &["checkout", "-q", root.as_str()]);
        git(repo, &["checkout", "-q", "-b", "branch-b"]);
        std::fs::write(repo.join("unrelated.txt"), "b\n")?;
        git(repo, &["add", "unrelated.txt"]);
        git(repo, &["commit", "-q", "-m", "branch-b's real tip"]);

        let logger = Arc::new(Logger::for_command("test", false));
        let context =
            GlobalContext::new_at_path(&repo.to_string_lossy(), false, true, true, logger)
                .expect("failed to build test GlobalContext");

        let stages: Vec<MergeStages> = vec![(
            "shared.txt".to_string(),
            Some("base-oid".to_string()),
            Some("ours-oid".to_string()),
            Some("theirs-oid".to_string()),
        )];
        let resolved_dir = tempfile::tempdir()?;
        std::fs::write(resolved_dir.path().join("shared.txt"), "resolved\n")?;
        let pending = PendingConflict {
            env: "dev".to_string(),
            branch: "branch-b".to_string(),
            conflicts_with: "branch-a".to_string(),
            source_branch_head: source_branch_head.clone(),
            stages: stages.clone(),
        };
        resolutions::record_resolution(
            context.git(),
            &pending,
            resolved_dir.path(),
            "tester@example.com",
            "2026-01-01T00:00:00Z",
        )?;

        let outcome = MergeTreeCompose {
            tree_oid: root,
            conflicted_stages: stages,
        };

        let mut confirmed = HashSet::new();
        let replay_result = try_replay_resolution(
            &context,
            "composed-placeholder",
            "branch-b",
            "merge message",
            &outcome,
            &mut confirmed,
            false,
        )?;
        assert!(
            replay_result.is_none(),
            "a resolution recorded against a commit that isn't an ancestor of branch-b's \
             current tip must be held, not replayed"
        );

        Ok(())
    }

    /// The ordinary case must still work: a resolution recorded against a
    /// branch's actual current tip (the common case — nothing moved since
    /// recording) replays normally.
    #[test]
    fn replay_proceeds_when_source_branch_head_is_the_current_tip() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path();

        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.name", "Test User"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("README.md"), "hi\n")?;
        git(repo, &["add", "README.md"]);
        git(repo, &["commit", "-q", "-m", "init"]);

        git(repo, &["checkout", "-q", "-b", "branch-b"]);
        std::fs::write(repo.join("other.txt"), "a\n")?;
        git(repo, &["add", "other.txt"]);
        git(repo, &["commit", "-q", "-m", "branch-b tip"]);
        let branch_b_head = git(repo, &["rev-parse", "HEAD"]);
        let tree = git(repo, &["rev-parse", "HEAD^{tree}"]);

        let logger = Arc::new(Logger::for_command("test", false));
        let context =
            GlobalContext::new_at_path(&repo.to_string_lossy(), false, true, true, logger)
                .expect("failed to build test GlobalContext");

        let stages: Vec<MergeStages> = vec![(
            "shared.txt".to_string(),
            Some("base-oid".to_string()),
            Some("ours-oid".to_string()),
            Some("theirs-oid".to_string()),
        )];
        let resolved_dir = tempfile::tempdir()?;
        std::fs::write(resolved_dir.path().join("shared.txt"), "resolved\n")?;
        let pending = PendingConflict {
            env: "dev".to_string(),
            branch: "branch-b".to_string(),
            conflicts_with: "branch-a".to_string(),
            source_branch_head: branch_b_head,
            stages: stages.clone(),
        };
        resolutions::record_resolution(
            context.git(),
            &pending,
            resolved_dir.path(),
            "tester@example.com",
            "2026-01-01T00:00:00Z",
        )?;

        let outcome = MergeTreeCompose {
            tree_oid: tree,
            conflicted_stages: stages,
        };

        let mut confirmed = HashSet::new();
        let replay_result = try_replay_resolution(
            &context,
            "composed-placeholder",
            "branch-b",
            "merge message",
            &outcome,
            &mut confirmed,
            false,
        )?;
        assert!(
            replay_result.is_some(),
            "a resolution recorded against branch-b's actual current tip must replay"
        );

        Ok(())
    }
```

- [ ] **Step 3: Run the tests to verify the first one fails**

Run: `cargo test -p hitch --lib try_replay_resolution_tests -- --nocapture`
Expected: `replay_holds_when_source_branch_head_is_not_an_ancestor` FAILS (replay currently succeeds because nothing checks lineage yet); `replay_proceeds_when_source_branch_head_is_the_current_tip` PASSES already (no regression risk in the no-op case).

- [ ] **Step 4: Add the lineage check to `try_replay_resolution`**

In `src/utils/prelude.rs`, in `try_replay_resolution`, insert this block immediately after the `res.meta.key != key` structural check (after its closing `}` at what was originally line 1310, before the `require_signed` comment block):

```rust
    // `res.meta.key` proves the merge-stage inputs match exactly, but not
    // that this resolution was ever recorded against a state `branch`
    // actually descends from — two branches can independently produce
    // byte-identical merge inputs (e.g. the same one-line fix to the same
    // file) without being the same conflict in any meaningful sense.
    // `source_branch_head` is git's own record of what `branch` looked like
    // at record time; require the branch's current tip to be that commit or
    // a descendant of it before treating this as "the same conflict".
    if let Some(current_head) = context.git().rev_parse_opt(&format!("refs/heads/{}", branch))? {
        let descends_from_recorded_head = current_head == res.meta.source_branch_head
            || context
                .git()
                .get_merge_base(&current_head, &res.meta.source_branch_head)?
                .as_deref()
                == Some(res.meta.source_branch_head.as_str());
        if !descends_from_recorded_head {
            context.log_warning(&format!(
                "Recorded resolution {} for '{}' was recorded against a different point in \
                 '{}'s history ({}) than its current tip ({}) — holding instead, in case the \
                 matching stage OIDs are coincidental rather than the same conflict. To \
                 inspect it:\n  hitch resolutions",
                &key[..12.min(key.len())],
                branch,
                branch,
                &res.meta.source_branch_head[..12.min(res.meta.source_branch_head.len())],
                &current_head[..12.min(current_head.len())]
            ));
            return Ok(None);
        }
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p hitch --lib try_replay_resolution_tests -- --nocapture`
Expected: PASS, all tests in the module including the two new ones and the pre-existing `require_signed_check_fails_closed_on_metadata_health_error`.

- [ ] **Step 6: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean. Pay particular attention to `tests/integration/resolve_tests.rs`'s existing replay tests (`test_replay_composes_recorded_resolution`, `test_replay_is_a_miss_after_branch_moves`) — the second one's name suggests it may already be testing a related "branch moved" scenario; confirm it still passes and still tests what its name claims (if its "branch moved" scenario happens to also move `branch`'s tip away from `source_branch_head`, the new lineage check should now be part of why it's a miss, which is a strengthening, not a regression — but verify the test's assertions don't assume a *different* reason for the miss that this change invalidates).

- [ ] **Step 7: Commit**

```bash
git add src/utils/resolutions.rs src/utils/prelude.rs
git commit -m "fix: enforce source_branch_head lineage before replaying a recorded resolution"
```

---

### Task 2: Reflog-backed lineage check for crash-recovery checkout repair

**Files:**
- Modify: `src/utils/git_operations.rs` (new `reflog_values` method, placed near `matches_commit_exactly`/`is_working_directory_clean`)
- Modify: `src/utils/publish_journal.rs:251-295` (`repair_checkout`)
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Produces: `pub fn reflog_values(&self, refname: &str, limit: usize) -> Result<Vec<String>>` on `GitOperations`.
- Consumes: nothing from Task 1 — independent.

**Grounding:** `src/utils/git_operations.rs` has no in-file `#[cfg(test)] mod tests` — this module's tests live entirely in `tests/unit/git_operations_tests.rs`, which uses `HitchTestFramework::new()` + `framework.with_test_environment(TestSetup::GitOnly, |env| { ... })`, giving `env.temp_dir` (repo path), `env.git` (raw git command runner), and `GitOperations::new_at_path(&env.temp_dir.to_string_lossy())` — this exact pattern repeats at nearly every existing test in that file (e.g. lines 87-107).

- [ ] **Step 1: Write the failing test**

Add to `tests/unit/git_operations_tests.rs`, following the file's existing pattern:

```rust
    #[test]
    fn reflog_values_returns_prior_ref_values_most_recent_first() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;

            env.fs.write_file("f.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "one"])?.assert_success();
            let sha_one = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();

            env.fs.write_file("f.txt", "two")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "two"])?.assert_success();
            let sha_two = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();

            let branch = env.git.run(&["branch", "--show-current"])?.stdout().trim().to_string();
            let values = git_ops.reflog_values(&format!("refs/heads/{}", branch), 10)?;
            assert_eq!(values.first(), Some(&sha_two));
            assert!(values.contains(&sha_one));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn reflog_values_is_empty_not_an_error_for_a_ref_with_no_reflog() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            let values = git_ops.reflog_values("refs/heads/does-not-exist", 10)?;
            assert!(values.is_empty());
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p hitch reflog_values -- --nocapture`
Expected: compile error — `reflog_values` doesn't exist yet.

- [ ] **Step 3: Implement `reflog_values`**

In `src/utils/git_operations.rs`, add near `matches_commit_exactly` (around line 1861):

```rust
    /// The values `refname` has had, most recent first, up to `limit`
    /// entries, as recorded in its own reflog.
    ///
    /// This is what distinguishes "this ref's tip really was `from_sha`
    /// before moving to its current value" from "some checkout's working
    /// tree happens to have the same tree content as `from_sha`" — content
    /// equality alone (`matches_commit_exactly`) can't tell those apart; the
    /// reflog is git's own record of what this ref's value actually was.
    ///
    /// Returns an empty `Vec`, not an error, when the ref has no reflog
    /// (freshly created, `core.logAllRefUpdates=false`, or entries expired
    /// via `gc.reflogexpire`) — that is routine, not evidence of anything,
    /// and callers must treat it as inconclusive rather than as a
    /// contradiction.
    pub fn reflog_values(&self, refname: &str, limit: usize) -> Result<Vec<String>> {
        let output = self.run_git_command(&[
            "reflog",
            "show",
            "--format=%H",
            "-n",
            &limit.to_string(),
            refname,
        ])?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p hitch reflog_values -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Wire the check into `repair_checkout`**

In `src/utils/publish_journal.rs`, in `repair_checkout` (lines 251-295), replace:

```rust
    if !git.matches_commit_exactly(from_sha).unwrap_or(false) {
        warn_manual(context, record, path);
        return;
    }

    match git.reset_hard_to(&record.to_sha) {
```

with:

```rust
    if !git.matches_commit_exactly(from_sha).unwrap_or(false) {
        warn_manual(context, record, path);
        return;
    }

    // Defense in depth: `matches_commit_exactly` proves tree content matches
    // `from_sha`, but not that this checkout's branch ref genuinely held
    // that value. Check the ref's own reflog for corroboration. An empty
    // reflog (expiry, `core.logAllRefUpdates=false`) is inconclusive, not
    // suspicious — proceed as before in that case. Only a non-empty reflog
    // that *doesn't* contain `from_sha` is a real contradiction.
    let branch_ref = format!("refs/heads/{}", record.branch);
    let reflog = context.git().reflog_values(&branch_ref, 50).unwrap_or_default();
    if !reflog.is_empty() && !reflog.iter().any(|sha| sha == from_sha) {
        warn_manual(context, record, path);
        return;
    }

    match git.reset_hard_to(&record.to_sha) {
```

- [ ] **Step 6: Run the existing crash-recovery suite as a regression check**

Run: `cargo test -p hitch --test integration crash_recovery`
Expected: PASS unchanged — every existing recovery scenario's `from_sha` is genuinely in `refs/heads/<branch>`'s reflog (the ref transaction that moved it writes that reflog entry itself), so the new check should never trigger for any currently-passing case.

- [ ] **Step 7: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 8: Commit**

```bash
git add src/utils/git_operations.rs src/utils/publish_journal.rs tests/unit/git_operations_tests.rs
git commit -m "fix: corroborate crash-recovery's tree-match with the branch ref's own reflog"
```

---

### Task 3: Adversarial test proving the reflog check actually blocks a fabricated match

**Files:**
- Modify: `src/utils/publish_journal.rs` (in-file `#[cfg(test)] mod tests` — add one if none exists yet; check whether an earlier plan already added one before creating a duplicate)

**Interfaces:**
- Consumes: `reflog_values` from Task 2, `repair_checkout` (private `fn`, reachable via `super::*` from an in-file test module).

- [ ] **Step 1: Write the failing test**

Add to `src/utils/publish_journal.rs` (creating `#[cfg(test)] mod tests` at the bottom of the file if one doesn't already exist — check first, since a separately-applied plan may have added one for a different reason):

```rust
#[cfg(test)]
mod repair_checkout_tests {
    use super::*;
    use crate::commands::global_context::GlobalContext;
    use crate::utils::logging::Logger;
    use std::sync::Arc;

    #[allow(clippy::disallowed_methods)] // test-only scratch repo bootstrap, mirrors prelude.rs's raw-git test helpers
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A checkout whose working tree happens to match `from_sha`'s tree
    /// content, but whose branch ref never actually held `from_sha` — a
    /// fabricated record, standing in for either an adversarial checkout or
    /// a benign coincidental collision. `repair_checkout` must NOT silently
    /// reset it; it should warn instead, exactly as it already does for a
    /// genuinely dirty checkout.
    #[test]
    fn repair_checkout_declines_a_tree_match_the_reflog_contradicts() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path();

        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.name", "Test User"]);
        git(repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("f.txt"), "one\n")?;
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "one"]);

        // dev's real history: one -> two. Its reflog will contain both.
        git(repo, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(repo.join("f.txt"), "two\n")?;
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "two"]);
        let real_to_sha = git(repo, &["rev-parse", "dev"]);

        // A commit with IDENTICAL tree content to the real "one" state, but
        // built on a different, unrelated branch — dev's reflog never held
        // this SHA, even though its tree matches "one" byte for byte.
        git(repo, &["checkout", "-q", "-b", "unrelated", "dev~1"]);
        std::fs::write(repo.join("g.txt"), "filler\n")?;
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "filler, then revert to match dev~1's tree"]);
        git(repo, &["rm", "-q", "g.txt"]);
        git(repo, &["commit", "-q", "-m", "back to dev~1's tree content"]);
        let fabricated_from_sha = git(repo, &["rev-parse", "HEAD"]);
        // Confirm the fabrication actually has matching tree content before
        // relying on it below.
        let dev_root_tree = git(repo, &["rev-parse", "dev~1^{tree}"]);
        let fabricated_tree = git(repo, &["rev-parse", &format!("{fabricated_from_sha}^{{tree}}")]);
        assert_eq!(
            dev_root_tree, fabricated_tree,
            "test setup bug: fabricated commit's tree must match dev~1's tree exactly"
        );

        // The checkout under test: standing on 'dev', reset to the
        // fabricated commit's content directly (simulating tree content that
        // matches from_sha without the branch ref ever having held it).
        git(repo, &["checkout", "-q", "dev"]);
        git(repo, &["reset", "--hard", &fabricated_from_sha]);

        let logger = Arc::new(Logger::for_command("test", false));
        let context =
            GlobalContext::new_at_path(&repo.to_string_lossy(), false, true, true, logger)
                .expect("failed to build test GlobalContext");

        // dev's ref must still read as `real_to_sha` for repair_checkout's
        // caller (`recover`) to even consider this checkout — restore that
        // after the working-tree reset above, exactly as an interrupted
        // publish would leave it (ref moved, working tree not yet resynced).
        git(repo, &["update-ref", "refs/heads/dev", &real_to_sha]);

        let record = PublishRecord {
            branch: "dev".to_string(),
            from_sha: Some(dev_root_tree_commit(repo)), // see helper below
            to_sha: real_to_sha,
            checkouts: vec![repo.to_string_lossy().to_string()],
            ..Default::default()
        };

        repair_checkout(&context, &record, &repo.to_string_lossy());

        // If repair_checkout wrongly trusted the tree match, HEAD would now
        // be at real_to_sha (it reset). Since the reflog contradicts the
        // fabricated from_sha, it must have declined and left the working
        // tree exactly where the fabrication put it.
        let head_after = git(repo, &["rev-parse", "HEAD"]);
        assert_eq!(
            head_after, fabricated_from_sha,
            "repair_checkout applied a reset despite the reflog contradicting from_sha"
        );

        Ok(())
    }

    /// `dev~1`'s actual commit SHA (not the fabricated one) — the record's
    /// `from_sha` must name a real commit for the `Option<String>` field to
    /// be meaningful, even though what's under test is the *tree content*
    /// coincidence, not this SHA's own reflog presence.
    fn dev_root_tree_commit(repo: &std::path::Path) -> String {
        git(repo, &["rev-parse", "dev~1"])
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p hitch repair_checkout_declines_a_tree_match -- --nocapture`
Expected: FAILS before Task 2's reflog check exists (or if Task 2 hasn't landed yet in execution order, this task depends on it — confirm Task 2 is complete first). If Task 2 is already applied and this still fails, the assertion setup likely has a bug — check that `record.from_sha` (which must be `dev~1`'s SHA, per the field's meaning) is genuinely different from `fabricated_from_sha` while sharing its tree, and that `matches_commit_exactly(from_sha)` in `repair_checkout` is being evaluated against `dev~1`'s SHA (whose tree the fabrication was built to match), not against the fabrication's own SHA.

- [ ] **Step 3: Fix forward until it passes**

This task should require no production-code changes if Task 2 is correctly implemented — only test-setup fixes. If it reveals a genuine gap in Task 2's implementation, fix `repair_checkout`/`reflog_values` there, not here.

- [ ] **Step 4: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 5: Commit**

```bash
git add src/utils/publish_journal.rs
git commit -m "test: prove crash-recovery declines a tree match the branch's reflog contradicts"
```

---

### Task 4: Surface lineage-check outcomes in `hitch doctor`

**Files:**
- Modify: `src/commands/doctor.rs`

**Interfaces:**
- Consumes: `resolutions::list_resolutions` (already used by `check_resolution_debt`) and each resolution's `meta.source_branch_head`.

- [ ] **Step 1: Write the failing test**

Add to `src/commands/doctor.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn source_branch_head_report_line_names_the_recorded_commit() {
        // format_source_branch_head_note is a pure formatting function —
        // test its output shape directly rather than standing up a full
        // repo + GlobalContext for a one-line report addition.
        let note = format_source_branch_head_note("abcdef1234567890");
        assert!(note.contains("abcdef123456"));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p hitch --lib doctor::tests -- --nocapture`
Expected: compile error — `format_source_branch_head_note` doesn't exist yet.

- [ ] **Step 3: Add the helper and wire it into `check_resolution_debt`'s per-resolution report line**

In `src/commands/doctor.rs`, add:

```rust
/// A short note naming the commit a resolution was recorded against, for
/// the per-resolution report line in `check_resolution_debt` — makes the
/// lineage check Task 1 of this plan added to replay visible at a glance,
/// without requiring a separate doctor pass.
fn format_source_branch_head_note(source_branch_head: &str) -> String {
    format!("recorded against {}", &source_branch_head[..12.min(source_branch_head.len())])
}
```

In `check_resolution_debt`, find the per-resolution `context.log_info` call (around line 123-131):

```rust
        context.log_info(&format!(
            "  {} {} vs {} ({}, env {}) — {}",
            &r.meta.key[..12.min(r.meta.key.len())],
            r.meta.branch,
            r.meta.conflicts_with,
            age_str,
            r.meta.env,
            r.meta.recorded_by
        ));
```

Replace with:

```rust
        context.log_info(&format!(
            "  {} {} vs {} ({}, env {}) — {}, {}",
            &r.meta.key[..12.min(r.meta.key.len())],
            r.meta.branch,
            r.meta.conflicts_with,
            age_str,
            r.meta.env,
            r.meta.recorded_by,
            format_source_branch_head_note(&r.meta.source_branch_head)
        ));
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p hitch --lib doctor::tests -- --nocapture`
Expected: PASS.

Run: `cargo test -p hitch --test integration doctor` (if such a test file exists — check first with `grep -rln "hitch doctor\|\"doctor\"" tests/integration/`) to confirm no existing test asserts on the old, now-changed log line format; update any that do.

- [ ] **Step 5: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 6: Commit**

```bash
git add src/commands/doctor.rs
git commit -m "feat: show each recorded resolution's source commit in hitch doctor"
```
