# Migrate Read-Only Git Primitives to git2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the subprocess-and-parse-stdout implementation of four pure-read git primitives (`rev_parse`, `rev_parse_opt`, `cat_file_blob`/`read_blob`, `get_merge_base`) with direct calls into the `git2` (libgit2) crate `GitOperations` already depends on and already holds an open `Repository` handle for — without changing any caller-visible signature, and without touching anything involved in the ORT merge engine (`merge_tree_compose` and its siblings stay on the git-CLI path indefinitely; see Global Constraints).

**Architecture:** `GitOperations` (`src/utils/git_operations.rs:56-71`) already stores a `git2::Repository` in `self.repo`, opened in both constructors — currently marked `#[allow(dead_code)]` because nothing uses it beyond `.workdir()`/`.path()`. This plan puts it to work for the four primitives that are pure reads with no stderr-substring control flow (verified per-primitive in each task), migrating them one at a time behind their existing method signatures so no caller anywhere in the codebase changes. Each migration ships with a differential test that runs the new git2-backed method against a real `git` CLI invocation on the same scenario and asserts they agree — the same discipline `test_merge_tree_compose_matches_real_merge_across_scenarios` already established for the merge engine, applied here to a much lower-risk surface.

**Tech Stack:** Rust, `git2 = "0.20"` (already a dependency), `anyhow`, the existing `HitchTestFramework` / `tests/unit/git_operations_tests.rs` pattern.

## Global Constraints

- Out of scope, explicitly: `merge_tree_compose` and every primitive touching the ORT merge engine. `test_merge_tree_compose_matches_real_merge_across_scenarios` is the load-bearing correctness guarantee for that path (see `AGENTS.md`); swapping its engine would require proving three-way agreement (git-CLI vs `merge-tree` vs git2) instead of the current two-way differential, and is not attempted here.
- Out of scope, explicitly: anything touching a remote (`push`, `fetch`, `gh` calls) or SSH signing (`sign_bytes_ssh`/`verify_signature_ssh`) — `git2` does not obviously replace credential-helper-backed network operations or SSH commit signing safely, and this plan does not attempt it.
- Out of scope, explicitly: `list_refs`/`list_refs_under` — `for-each-ref`'s prefix-matching semantics and `git2::Repository::references_glob`'s fnmatch semantics are close but not obviously identical at ref-path-segment boundaries, and getting that boundary case wrong silently under-or-over-matches a ref namespace. Left for a follow-up plan with its own differential test once that boundary behavior is nailed down, rather than risked here.
- Before migrating a primitive, grep its current callers for any place that inspects the *text* of an `Err` this primitive can return (not just whether it's `Ok`/`Err`) — a caller matching git-CLI-specific stderr wording would break silently under git2's differently-worded errors. Each task below includes this grep as an explicit step.
- Every migrated primitive keeps its exact existing signature (`pub fn rev_parse(&self, reference: &str) -> Result<String>`, etc.) — this is an implementation swap behind a stable interface, not an API change.
- `just format`, `just format-check && just lint`, and `just test` must all pass clean before any task is considered done.

---

### Task 1: Migrate `rev_parse` and `rev_parse_opt`

**Files:**
- Modify: `src/utils/git_operations.rs:494-504` (`rev_parse`), `:1925-1933` (`rev_parse_opt`), `:56-71` (remove `#[allow(dead_code)]` from `repo`)
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Produces: `rev_parse(&self, reference: &str) -> Result<String>` and `rev_parse_opt(&self, reference: &str) -> Result<Option<String>>` — signatures unchanged, implementation now uses `self.repo.revparse_single`.

- [ ] **Step 1: Grep for callers that inspect these methods' error text**

Run: `grep -rn "rev_parse(" src/ tests/ | grep -v "fn rev_parse"` and `grep -rn "rev_parse_opt(" src/ tests/ | grep -v "fn rev_parse_opt"`, then check each hit's surrounding code for `.to_string().contains(...)` or similar on the `Err` case. Per this plan's research, no call site does this (both are used purely for their `Ok` value or as a plain `?`/`.unwrap_or`), but confirm before proceeding rather than trusting that research blindly.

- [ ] **Step 2: Write the failing differential tests**

Add to `tests/unit/git_operations_tests.rs`:

```rust
    #[test]
    fn rev_parse_opt_agrees_with_git_cli_across_revspec_forms() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;

            env.fs.write_file("f.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "one"])?.assert_success();
            env.git
                .run(&["tag", "-a", "v1", "-m", "release v1"])?
                .assert_success();

            let branch = env
                .git
                .run(&["branch", "--show-current"])?
                .stdout()
                .trim()
                .to_string();
            let commit_sha = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();
            let tree_sha = env
                .git
                .run(&["rev-parse", "HEAD^{tree}"])?
                .stdout()
                .trim()
                .to_string();

            let specs = [
                format!("refs/heads/{}", branch),
                "v1".to_string(),
                commit_sha.clone(),
                commit_sha[..10].to_string(),
                "HEAD^{tree}".to_string(),
                format!("{}:f.txt", commit_sha),
                "does-not-exist".to_string(),
            ];

            for spec in &specs {
                let expected = env
                    .git
                    .run(&["rev-parse", "--verify", "--quiet", spec])
                    .ok()
                    .filter(|r| r.success())
                    .map(|r| r.stdout().trim().to_string())
                    .filter(|s| !s.is_empty());

                let actual = git_ops.rev_parse_opt(spec)?;
                assert_eq!(
                    actual, expected,
                    "rev_parse_opt('{}') disagreed with git CLI: got {:?}, expected {:?}",
                    spec, actual, expected
                );
            }

            // The tree-peel spec's value must equal the tree we captured directly.
            assert_eq!(
                git_ops.rev_parse_opt("HEAD^{tree}")?,
                Some(tree_sha),
                "tree-peel spec did not resolve to the expected tree"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn rev_parse_errors_on_a_reference_that_does_not_resolve() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            env.fs.write_file("f.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "one"])?.assert_success();

            let sha = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();
            assert_eq!(git_ops.rev_parse("HEAD")?, sha);

            assert!(
                git_ops.rev_parse("does-not-exist").is_err(),
                "rev_parse must error, not silently return an empty string, for an \
                 unresolvable reference"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
```

- [ ] **Step 3: Run the tests to verify they fail or pass trivially against the old implementation**

Run: `cargo test -p hitch rev_parse -- --nocapture`
Expected: both tests PASS against the *current* subprocess-based implementation (this is a differential test of behavior, not of code path — it should already hold true before the migration; if it doesn't, the migration's target behavior is already wrong and needs re-deriving before Step 4, not after).

- [ ] **Step 4: Implement the git2-backed versions**

In `src/utils/git_operations.rs`, replace `rev_parse` (lines 494-504):

```rust
    pub fn rev_parse(&self, reference: &str) -> Result<String> {
        let obj = self
            .repo
            .revparse_single(reference)
            .with_context(|| format!("git rev-parse {} failed", reference))?;
        Ok(obj.id().to_string())
    }
```

Replace `rev_parse_opt` (lines 1925-1933):

```rust
    pub fn rev_parse_opt(&self, reference: &str) -> Result<Option<String>> {
        // Mirrors `git rev-parse --verify --quiet`: any resolution failure —
        // not found, ambiguous, malformed spec — is a soft `None`, never an
        // `Err`. Discarding the git2::Error here is deliberate, not lazy:
        // that's the exact behavior this replaces.
        Ok(self
            .repo
            .revparse_single(reference)
            .ok()
            .map(|obj| obj.id().to_string()))
    }
```

- [ ] **Step 5: Remove the now-justified `#[allow(dead_code)]`**

In `src/utils/git_operations.rs:56-58`, remove the `#[allow(dead_code)]` attribute directly above `repo: Repository,` — it is no longer dead code.

- [ ] **Step 6: Run the tests to verify they pass against the new implementation**

Run: `cargo test -p hitch rev_parse -- --nocapture`
Expected: PASS, both tests, now genuinely exercising the git2 path.

- [ ] **Step 7: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean. Pay attention to anything that asserted on the exact wording of a `rev_parse` failure message (grep `rev-parse.*failed` across `tests/` if the suite surfaces a break here) — git2's error text differs from the old `String::from_utf8_lossy(&output.stderr)` wording; update any such assertion to match the new (still informative) message rather than trying to preserve the old wording verbatim.

- [ ] **Step 8: Commit**

```bash
git add src/utils/git_operations.rs tests/unit/git_operations_tests.rs
git commit -m "refactor: migrate rev_parse/rev_parse_opt to git2, off subprocess+parse"
```

---

### Task 2: Migrate `cat_file_blob` and `read_blob`

**Files:**
- Modify: `src/utils/git_operations.rs:1849-1859` (`cat_file_blob`), `:3305-3315` (`read_blob`)
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Consumes: nothing from Task 1 (independent — can land before or after it).
- Produces: `cat_file_blob(&self, reference: &str) -> Result<Vec<u8>>` and `read_blob(&self, oid: &str) -> Result<Vec<u8>>` — signatures unchanged. `read_blob` becomes a thin call into `cat_file_blob` (both accept any git2 revspec resolving to a blob, including a bare OID), consolidating what `AGENTS.md`'s research already flagged as near-duplicate implementations.

- [ ] **Step 1: Grep for callers that inspect these methods' error text**

Run: `grep -rn "cat_file_blob(\|read_blob(" src/ tests/ | grep -v "fn cat_file_blob\|fn read_blob"`, check each hit for stderr-substring matching on the error path. Confirm none exists before proceeding.

- [ ] **Step 2: Write the failing test**

Add to `tests/unit/git_operations_tests.rs`:

```rust
    #[test]
    fn cat_file_blob_and_read_blob_agree_with_git_cli() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;

            env.fs.write_file("f.txt", "hello world\n")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "add f.txt"])?.assert_success();

            let commit_sha = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();
            let blob_oid = env
                .git
                .run(&["rev-parse", &format!("{}:f.txt", commit_sha)])?
                .stdout()
                .trim()
                .to_string();

            let expected = env
                .git
                .run(&["cat-file", "blob", &blob_oid])?
                .stdout()
                .into_bytes();

            assert_eq!(
                git_ops.cat_file_blob(&format!("{}:f.txt", commit_sha))?,
                expected,
                "cat_file_blob via a <commit>:<path> spec disagreed with git CLI"
            );
            assert_eq!(
                git_ops.cat_file_blob(&blob_oid)?,
                expected,
                "cat_file_blob via a raw blob OID disagreed with git CLI"
            );
            assert_eq!(
                git_ops.read_blob(&blob_oid)?,
                expected,
                "read_blob disagreed with git CLI"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
```

- [ ] **Step 3: Run the test to verify it passes against the old implementation**

Run: `cargo test -p hitch cat_file_blob_and_read_blob_agree -- --nocapture`
Expected: PASS (this asserts behavior true of the current subprocess implementation too).

- [ ] **Step 4: Implement the git2-backed versions**

Replace `cat_file_blob` (`src/utils/git_operations.rs:1849-1859`):

```rust
    pub fn cat_file_blob(&self, reference: &str) -> Result<Vec<u8>> {
        let obj = self
            .repo
            .revparse_single(reference)
            .with_context(|| format!("Failed to resolve '{}' to a blob", reference))?;
        let blob = obj
            .peel_to_blob()
            .with_context(|| format!("Failed to read blob '{}'", reference))?;
        Ok(blob.content().to_vec())
    }
```

Replace `read_blob` (`src/utils/git_operations.rs:3305-3315`):

```rust
    /// A raw OID is itself a valid revspec, so this is `cat_file_blob` under
    /// a name some callers use for clarity when they already have an OID
    /// rather than a `<rev>:<path>`-style spec.
    pub fn read_blob(&self, oid: &str) -> Result<Vec<u8>> {
        self.cat_file_blob(oid)
    }
```

- [ ] **Step 5: Run the test to verify it passes against the new implementation**

Run: `cargo test -p hitch cat_file_blob_and_read_blob_agree -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 7: Commit**

```bash
git add src/utils/git_operations.rs tests/unit/git_operations_tests.rs
git commit -m "refactor: migrate cat_file_blob/read_blob to git2, consolidate the duplicate"
```

---

### Task 3: Migrate `get_merge_base`

**Files:**
- Modify: `src/utils/git_operations.rs:2800-2814` (`get_merge_base`)
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Consumes: `rev_parse_opt` from Task 1 if it has already landed (reuses it to resolve `branch1`/`branch2` to OIDs); if Task 1 hasn't landed yet, this task resolves them directly via `self.repo.revparse_single` inline instead — either is fine, note which one this execution actually used in the commit.
- Produces: `get_merge_base(&self, branch1: &str, branch2: &str) -> Result<Option<String>>` — signature unchanged.

- [ ] **Step 1: Grep for callers that inspect this method's error text**

Run: `grep -rn "get_merge_base(" src/ tests/ | grep -v "fn get_merge_base"`, confirm no caller matches on error text (this method already returns `Ok(None)` rather than an `Err` for the "no common ancestor" case, so there is little surface for this concern here, but check the genuine-`Err` path — a resolution failure on `branch1`/`branch2` themselves — is not text-matched anywhere either).

- [ ] **Step 2: Write the failing test**

Add to `tests/unit/git_operations_tests.rs`:

```rust
    #[test]
    fn get_merge_base_agrees_with_git_cli() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;

            env.fs.write_file("f.txt", "base")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "base"])?.assert_success();
            let base_sha = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();

            env.git.run(&["checkout", "-b", "branch-a"])?.assert_success();
            env.fs.write_file("a.txt", "a")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "a"])?.assert_success();

            env.git.run(&["checkout", "-b", "branch-b", "main"])?.assert_success();
            env.fs.write_file("b.txt", "b")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "b"])?.assert_success();

            let expected = env
                .git
                .run(&["merge-base", "branch-a", "branch-b"])?
                .stdout()
                .trim()
                .to_string();
            assert_eq!(expected, base_sha, "test setup sanity check");

            let actual = git_ops.get_merge_base("branch-a", "branch-b")?;
            assert_eq!(actual, Some(base_sha));

            // Unrelated histories (no common ancestor) must be None, not an error.
            env.git.run(&["checkout", "--orphan", "unrelated"])?.assert_success();
            env.fs.write_file("u.txt", "u")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "unrelated root"])?.assert_success();

            let none_result = git_ops.get_merge_base("branch-a", "unrelated")?;
            assert_eq!(none_result, None, "unrelated histories must yield None, not an error");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
```

- [ ] **Step 3: Run the test to verify it passes against the old implementation**

Run: `cargo test -p hitch get_merge_base_agrees_with_git_cli -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Implement the git2-backed version**

Replace `get_merge_base` (`src/utils/git_operations.rs:2800-2814`):

```rust
    pub fn get_merge_base(&self, branch1: &str, branch2: &str) -> Result<Option<String>> {
        let oid1 = match self.rev_parse_opt(branch1)? {
            Some(sha) => git2::Oid::from_str(&sha)
                .with_context(|| format!("'{}' resolved to an invalid oid '{}'", branch1, sha))?,
            None => return Ok(None),
        };
        let oid2 = match self.rev_parse_opt(branch2)? {
            Some(sha) => git2::Oid::from_str(&sha)
                .with_context(|| format!("'{}' resolved to an invalid oid '{}'", branch2, sha))?,
            None => return Ok(None),
        };

        match self.repo.merge_base(oid1, oid2) {
            Ok(oid) => Ok(Some(oid.to_string())),
            // No common ancestor (unrelated histories) — matches the original
            // subprocess path's behavior on a nonzero `git merge-base` exit.
            Err(_) => Ok(None),
        }
    }
```

If Task 1 has not yet landed when this task executes, replace the two `self.rev_parse_opt(...)` calls with direct `self.repo.revparse_single(...).ok().map(|o| o.id())` inline instead, since `rev_parse_opt` won't exist as a git2-backed method yet — either produces the same result.

- [ ] **Step 5: Run the test to verify it passes against the new implementation**

Run: `cargo test -p hitch get_merge_base_agrees_with_git_cli -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Run the full suite and lint**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean.

- [ ] **Step 7: Commit**

```bash
git add src/utils/git_operations.rs tests/unit/git_operations_tests.rs
git commit -m "refactor: migrate get_merge_base to git2"
```

---

### Task 4: Full regression sweep and scope note

**Files:**
- Modify: `AGENTS.md` (architecture-map entry for `git_operations.rs`)

**Interfaces:**
- Consumes: Tasks 1-3.

- [ ] **Step 1: Run the entire suite once, deliberately, as a single pass**

Run: `just format && just format-check && just lint && just test`
Expected: all four clean, with all three migrated primitives now in a single consistent state (this is a final sweep after all three tasks, catching any interaction between them that a per-task run might have missed — e.g. Task 3's fallback branch from Step 4 if execution order differed from the plan's numbering).

- [ ] **Step 2: Confirm the merge-engine boundary was not crossed**

Run: `grep -n "merge_tree_compose\|merge_tree_write_tree_name_only" src/utils/git_operations.rs` and confirm both still call `run_git_plumbing_command`/`run_git_command` (i.e. still subprocess-based) — this plan must not have touched them. This is a deliberate sanity check, not busywork: it is the one thing this plan must never have done, per Global Constraints.

- [ ] **Step 3: Update `AGENTS.md`'s architecture-map entry**

In `AGENTS.md`, in the `src/utils/git_operations.rs` bullet, add a note after the existing "The `git2` dependency is present but effectively dead..." sentence (search for that exact phrase) — replace it with:

```
`git2` is used for the read-only plumbing primitives (`rev_parse`,
`rev_parse_opt`, `cat_file_blob`, `read_blob`, `get_merge_base`) via the
`Repository` handle `GitOperations` already opens in both constructors —
no subprocess for those five. Everything else, including every primitive
touching the ORT merge engine (`merge_tree_compose` and siblings) and
anything hitting a remote, still shells out to the real `git`/`gh` binaries;
see the differential tests in `tests/unit/git_operations_tests.rs`
(`*_agrees_with_git_cli`) for why that boundary is where it is.
```

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md
git commit -m "docs: record the git2 read-primitive migration and its boundary"
```
