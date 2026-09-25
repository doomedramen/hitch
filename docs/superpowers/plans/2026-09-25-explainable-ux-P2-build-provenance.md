# P2 — Build Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make "Actual" state reliable, so Desired / Actual / Proposed stop being one real concept plus a guess. Today hitch infers what an environment branch contains from commit *timestamps* and human commit messages (`src/core/status.rs:103`, `determine_rebuild_state`); this phase replaces the guessing substrate with a record hitch wrote itself, in the same atomic transaction that published the branch.

**Architecture:** A new `src/utils/build_record.rs` module holding a versioned `EnvironmentBuildRecord`, stored as a JSON blob at `refs/hitch/state/<environment>`. The record is written by `rebuild_environment_opts` and carried into `publish_branch` as one extra `RefEdit` in the *same* `ref_transaction` that moves `refs/heads/<env>` — so a record describing a tip the branch does not have is not representable, rather than merely unlikely. This is derived state and never lives in `hitch.json` (source §6, §34).

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, `chrono` (already a dependency with the `serde` feature, `Cargo.toml:46`), the existing `GitOperations` plumbing, `HitchTestFramework`.

**Parent plan:** `docs/superpowers/plans/2026-09-25-explainable-ux-program.md` (P2). Read its Global Constraints before starting.

---

## Why the record has to ride the publish transaction

The obvious implementation — write the branch, then write the record — has a crash window, and it is the *same class* of bug this repository has already been bitten by twice:

- `release` and `resolve`'s old publish path did `publish_journal::record()` then a separate `update_ref_cas`; the WAL entry and the effect it described were two operations, and the fix (`docs/superpowers/plans/2026-08-01-unify-publish-atomicity.md`) was to fold them into one `ref_transaction`.
- The wrong-merge-base gotcha in `AGENTS.md` is the same shape one layer down: a cheap approximation standing in for the real computation, disagreeing in exactly the cases nobody tested.

So: **one `RefEdit` list, one `ref_transaction`, no window.** A reader that finds `record.result_sha != rev-parse(refs/heads/<env>)` is therefore looking at a *deliberate* inconsistency — a publish that carried no record (`hitch resolve` Mode B, `hitch release`), or a human's `git update-ref` — and it must report that explicitly rather than trust the record.

### `expected_old` for the state ref: unconditional, and why that is safe

`RefEdit::Update { expected_old: Some(String::new()) }` — unconditional overwrite. `Create` would break the second rebuild (the ref already exists → the whole transaction fails → publishing is wedged), and `Update { expected_old: Some(<current>) }` would be a second CAS on a ref whose transaction is already CAS'd on the branch tip, adding a failure mode that buys nothing. Unconditional is safe *because* the batch is all-or-nothing: if the branch CAS fails, the state ref is not written either, so it can never clobber another publisher's record.

This is a **new** edit, not a change to the publish-journal edit. Global Constraint 4 (do not touch the journal's `expected_old`) and its regression test `publish_survives_leftover_publish_journal_ref` are untouched.

---

## Global Constraints

- `just format`, `just format-check && just lint`, `just test` clean before any task is done. **All four are `-p hitch`-scoped — never run a bare workspace-wide cargo command**, because `hitch-desktop` does not compile and is not ours.
- **`refs/hitch/state/*` must never enter `src/commands/cleanup.rs:174`'s prunable set** (`for namespace in ["backup", "prev"]`). It is a live pointer, not an archive. Task 6 adds a non-vacuous test: 11 rebuilds (enough for a prune to actually fire), `hitch cleanup --apply`, then assert `state` survived *and* `prev/` shrank to 10.
- **The two differential/crash-fuzz oracles must pass unchanged**, except for the additive assertions Task 6 makes inside `crash_recovery_tests.rs`'s existing abort loop. `test_merge_tree_compose_matches_real_merge_across_scenarios` is not to be edited at all.
- **Never sort a promoted branch list.** `PinnedInputs.branches` order is semantic, and `desired_branches` records that order verbatim.
- `PinnedInputs.branches` is `Vec<(String, String)>`. Converting to `Vec<PinnedBranch>` is a mechanical mapping in one direction only; do not change `PinnedInputs` itself — P3 and P4 depend on its shape.
- Comments explain *why*, not *what*. In particular: do not add comments restating that the record is written in the transaction. The reason is in the transaction's existing comment block at `src/utils/prelude.rs:1257-1262`.
- No user-visible output changes in this phase. `hitch rebuild` prints exactly what it printed before. If you find yourself editing `src/commands/rebuild.rs`'s strings, stop.

---

### Task 1: The `build_record` module — types, writer, reader

**Files:**
- Create: `src/utils/build_record.rs`
- Modify: `src/utils/mod.rs:4` (insert `pub mod build_record;` between `authorization` and `command_helpers`)

**Interfaces:**
- Produces: `pub struct PinnedBranch { pub branch: String, pub sha: String }` — `Serialize, Deserialize, Debug, Clone, PartialEq, Eq`.
- Produces: `pub struct ResolutionUse { pub branch: String, pub resolution_key: String }` — same derives.
- Produces: `pub struct EnvironmentBuildRecord { schema_version, environment, metadata_sha, base_name, base_sha, desired_branches: Vec<PinnedBranch>, included_branches: Vec<PinnedBranch>, held: Vec<CompatibilityConflict>, replayed_resolutions: Vec<ResolutionUse>, result_sha, built_at: DateTime<Utc>, hitch_version }` — field names taken verbatim from source §6 so the on-disk JSON matches what the eventual desktop consumer expects.
- Produces: `pub enum EnvironmentBuildState { LegacyUnknown, Known(Box<EnvironmentBuildRecord>), ResultMismatch { record: Box<EnvironmentBuildRecord>, live_tip: Option<String> }, Unreadable { reason: String } }` — `Debug`.
- Produces: `pub fn state_ref(env_name: &str) -> String`.
- Produces: `pub fn record_blob(git: &GitOperations, record: &EnvironmentBuildRecord) -> Result<(String, String)>` — returns `(refname, blob_oid)`, writes no ref. Mirrors `publish_journal::record_blob` (`src/utils/publish_journal.rs:147`) exactly, for the same reason.
- Produces: `pub fn read_state(git: &GitOperations, env_name: &str) -> Result<EnvironmentBuildState>`.
- Produces: `pub fn resolve_metadata_sha(git: &GitOperations) -> Result<String>`.
- Consumes: `GitOperations::{hash_object_bytes, cat_file_blob, rev_parse_opt}` (`src/utils/git_operations.rs:3160`, `:1844`, `rev_parse_opt` via git2) and `CompatibilityConflict` (`src/utils/prelude.rs:2254`).
- Modify: `src/utils/prelude.rs:2252-2264` — add `Serialize, Deserialize, PartialEq, Eq` to `CompatibilityConflict`'s derives (line 2253). Note there is an unrelated `#[derive(Debug, Clone)]` at `:2186`; do not edit that one.

**Decisions recorded here so they are not re-litigated:**

1. **`held: Vec<CompatibilityConflict>`, not a new `HoldRecord` type.** Source §6 says `HoldRecord` in a *"Suggested record"* block, not as a mandate. `CompatibilityConflict` already carries exactly those three fields, is already what `CompositionResult.held` and `RebuildOutcome.held` contain, and duplicating it would create a third conflict shape. The spec's name is noted in the field's doc comment instead. The only change to the existing type is additive derives.
2. **`built_at` is `chrono::DateTime<Utc>`,** per source §6, not the `String` RFC3339 that `publish_journal` and `resolutions` use. `chrono`'s `serde` feature is already enabled (`Cargo.toml:46`), so this costs no new dependency, and a typed timestamp is what P3 needs when it stops treating time as a correctness input.
3. **`metadata_sha` is a recorded fact and a fast path, not the authoritative staleness signal.** P3's real staleness check compares the *per-branch pinned SHAs* in `desired_branches` against freshly pinned ones, because that catches the cases a single metadata-hash comparison cannot (metadata changed and changed back; metadata SHA moved because an unrelated environment was promoted). `metadata_sha` is still worth recording — it names the config the build was declared from, and it makes "did the declaration change at all" a one-ref answer.
   `resolve_metadata_sha` reads `refs/heads/hitch-metadata`, falling back to `refs/remotes/origin/hitch-metadata`, and errors if neither exists. `access_metadata_read_only` has the same fallback order (`src/utils/prelude.rs:277-296`) and `check_metadata_health` guarantees the local branch is not behind remote, so in the healthy case the local tip is the right answer. In the already-degraded case (local unreadable) the recorded SHA may not be the one the config was actually read from — say so in the function's doc comment rather than pretending otherwise, because the consequence is bounded: P3 does not gate on this field.
4. **No `deny_unknown_fields`.** `schema_version` is the compatibility gate; if a future hitch adds a field it bumps the version, and this hitch's reader refuses a version it does not know (`Unreadable`) rather than misreading it.
5. **`read_state` never guesses.** Missing ref → `LegacyUnknown`. Present but `result_sha` disagreeing with the live tip (including the tip being *absent*, e.g. the environment branch was deleted) → `ResultMismatch`. Present but unparseable, or `schema_version` greater than `SCHEMA_VERSION` → `Unreadable`. `SCHEMA_VERSION` is `1` and is a `pub const`.

- [x] **Step 1: Write the failing unit tests first**

Add to the new `src/utils/build_record.rs`, at the bottom, a `#[cfg(test)] mod tests` using the git-backed unit-test pattern from `tests/unit/git_operations_tests.rs:20-30` (`GitOperations::new_at_path(&env.temp_dir.to_string_lossy())` against a `TestEnvironment`):

- `record_blob_then_read_state_round_trips`: write a record via `record_blob`, point a ref at the returned oid with `update_ref`, `read_state` returns `Known` with every field equal to what was written — including that `desired_branches` came back in the *same order* it went in as.
- `read_state_on_a_fresh_environment_is_legacy_unknown`: no ref → `LegacyUnknown`. Not `Err`.
- `read_state_flags_a_result_mismatch`: valid record whose `result_sha` is some other SHA, with `refs/heads/dev` present at a different SHA → `ResultMismatch` carrying the live tip.
- `read_state_flags_a_missing_tip`: valid record, no `refs/heads/dev` at all → `ResultMismatch { live_tip: None }`.
- `read_state_flags_a_corrupt_blob`: ref pointing at bytes that are not JSON → `Unreadable`, not `Err`, not `Known`.
- `read_state_refuses_a_newer_schema`: hand-written JSON with `schema_version: 999` → `Unreadable`, and the reason string mentions the version.
- `resolve_metadata_sha_prefers_the_local_metadata_branch`: with both `refs/heads/hitch-metadata` and `refs/remotes/origin/hitch-metadata` present at different SHAs, returns the local one.

- [x] **Step 2: Add the types and the two serde derives**

In `src/utils/prelude.rs:2253`, change `#[derive(Debug, Clone)]` on `CompatibilityConflict` to `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]` and add `use serde::{Deserialize, Serialize};` to the module's imports (`src/utils/prelude.rs:1-5`) — it has none today. The desktop crate already compiles `prelude`, so widening its imports cannot break it.

While in the doc comment above it (`:2252`), fix a line that is already wrong: it reads *"One branch's conflict, as found by `preflight_compatibility_report`"* — but since P1, `compose_environment` produces them too, and that is now the producer that matters. Per `AGENTS.md`'s own rule about fixing stale docs on sight, correct it in this change rather than leaving a comment that points the next agent at the wrong function.

Then create `src/utils/build_record.rs` with the module doc explaining what the record is for and, critically, that it is *derived state* — never in `hitch.json`, never user-authored — and the three types. Give `EnvironmentBuildRecord` a `#[derive(Default)]`-free explicit construction and a `new(...)` taking exactly the fields; do not add defaults, because a record that silently defaults a missing `result_sha` is precisely the fabricated-Actual failure this phase exists to prevent.

- [x] **Step 3: Implement the writer**

`state_ref(env_name)` → `format!("refs/hitch/state/{}", env_name)`. `record_blob` → `serde_json::to_vec_pretty` then `git.hash_object_bytes`, returning `(state_ref(env_name), oid)`. Document that it writes no ref, and that this split is what lets the caller place the ref update inside `publish_branch`'s existing transaction.

`resolve_metadata_sha` per decision 3 above, with the degraded-case caveat in its doc comment.

- [x] **Step 4: Implement the reader**

`read_state` does `git.rev_parse_opt(&state_ref(env_name))`; on `Ok(None)` return `LegacyUnknown` without touching anything else. Otherwise `cat_file_blob` the oid, `serde_json::from_slice`, and classify. Check `schema_version` *before* trusting any other field. Then compare `record.result_sha` against `git.rev_parse_opt("refs/heads/<env>")` and return `ResultMismatch` on disagreement.

Every failure mode returns an `EnvironmentBuildState` value. `read_state` returns `Err` only for genuine I/O failure (a git invocation that failed), never for "the record is not usable" — that distinction is what lets P3's `status` render "unknown" instead of erroring out on a repo with one bad blob.

- [x] **Step 5: Gate**

`just format && just format-check && just lint && just test`. All seven unit tests green.

---

### Task 2: `extras` on `publish_branch`

**Files:**
- Modify: `src/utils/prelude.rs:1242-1250` (`publish_branch` signature)
- Modify: `src/utils/prelude.rs:1341-1355` (append extras to `edits` immediately before the `ref_transaction` call)
- Modify: `src/utils/prelude.rs:1430-1437` (`publish_environment_build` signature + forwarding)
- Modify: `src/commands/release.rs:334-341` and `:355` (pass `&[]`)
- Modify: `src/commands/resolve.rs:435` (pass `&[]` — Mode A's rebase landing)
- Modify: `src/commands/resolve.rs:735` (`publish_environment_build` call — pass `&[]`, see Task 5 note)

**Interfaces:**
- Change: `publish_branch(context, branch, new_sha, backup_timestamp, retry_hint, push_remedy, push)` gains `extras: &[RefEdit]`, placed immediately after `new_sha` so the ref edits sit next to the thing they describe. `RefEdit` is already in `crate::utils::git_operations` (`src/utils/git_operations.rs:117`); source §6's `PublishExtraRef` is a field-for-field duplicate of it, so use `RefEdit` and note the substitution.
- Change: `publish_environment_build` gains the same `extras: &[RefEdit]` and forwards it.
- Change: `release.rs` and `resolve.rs`'s `finish_mode_a` pass `&[]`.

**Scope note, recorded so it is not mistaken for an oversight:** `hitch release` publishes a branch too, so it *could* carry a record. It does not in this phase. A release's result is not an environment composition — it is a tag-and-branch landing of a specific source commit — so the `EnvironmentBuildRecord` shape does not describe it truthfully, and writing one would be the fabricated-Actual failure in a new costume. Source §16/§34 give release its own history model; that is P5's work. Same reasoning for `resolve`'s Mode B (`src/commands/resolve.rs:735`): it produces an environment build, but from a hand-resolved worktree whose included-branch list hitch does not currently track, so a record built from a `CompositionResult` it never had would be a guess. Task 5 Step 4 turns that gap into an explicit test.

- [x] **Step 1: Add the parameter and forward it**

Insert `extras: &[RefEdit],` after `new_sha` in both signatures. In `publish_branch`, after the two archival `RefEdit::Update` pushes at `src/utils/prelude.rs:1341-1351` and before the `ref_transaction` call at `:1353`, add `edits.extend_from_slice(extras);`.

Extend the existing comment block at `:1257-1262` by one sentence: it currently explains that the journal ref and the replaced tip ride with the branch move. Add that caller-supplied `extras` ride there too, for the same reason — so a caller never has a ref update that is not part of the same atomic batch. Do not reword the rest of that comment; it is load-bearing history.

- [x] **Step 2: Update the three `&[]` callers**

`release.rs` has two `publish_branch` calls (`:334` branch push, `:355` tag push). `resolve.rs:435` is `finish_mode_a`. `resolve.rs:735` is the Mode B `publish_environment_build` call. All four get `&[]`.

- [x] **Step 3: Gate**

`just format && just format-check && just lint && just test`. Nothing in the existing suite may need editing — if one does, the plumbing is wrong. In particular `crash_recovery_tests.rs`, `release_crash_recovery_tests.rs`, and `resolve_crash_recovery_tests.rs` must all still pass untouched at this point.

---

### Task 3: `ResolutionUse` — record *which* resolution was replayed

**Files:**
- Modify: `src/utils/prelude.rs:1530-1539` (`try_replay_resolution` — return the key)
- Modify: `src/utils/prelude.rs:686-693` (the `replayed.push` site in `compose_environment`)
- Modify: `src/utils/prelude.rs:576` and `:602` (`RebuildOutcome.replayed` and `CompositionResult.replayed` field types)
- Modify: `src/commands/rebuild.rs:105-110` and `:182-191` (the two display sites)

**Interfaces:**
- Change: `try_replay_resolution` returns `Result<Option<(String, String)>>` — `(new_composed_sha, resolution_key)` instead of `Option<String>`. The key is already computed internally at `src/utils/prelude.rs:1546`; returning it costs nothing.
- Change: `CompositionResult.replayed` and `RebuildOutcome.replayed` become `Vec<ResolutionUse>`.
- Consumes: `build_record::ResolutionUse` (Task 1).

**Why this is in P2 and not deferred:** `CompositionResult.replayed` is currently `Vec<String>` — just branch names. Source §6 asks for `replayed_resolutions: Vec<ResolutionUse>`, and the whole point of the record is being able to say *which* recorded resolution produced this build. A branch name alone cannot, because the key is content-addressed: the same branch replayed against a later conflict resolves under a *different* key. Recording the name without the key would be the field's value silently dropped.

The ripple is five call sites, all display-only. Do not change what is printed.

- [x] **Step 1: Change the return type and the push site**

`try_replay_resolution`: return `Ok(None)` in every early-return as today, and at the success path return `Ok(Some((resolved_commit, key)))`.

`compose_environment`'s replay branch (`:686-693`) becomes:

```rust
if let Some((resolved, key)) = try_replay_resolution(/* unchanged args */)? {
    composed = resolved;
    replayed.push(ResolutionUse {
        branch: branch.clone(),
        resolution_key: key,
    });
    included.push(branch.clone());
    last_composed = branch.clone();
    continue;
}
```

- [x] **Step 2: Update the two display sites**

`src/commands/rebuild.rs:105-110` and `:182-191` each do `result.replayed.join(", ")`. That still compiles only if `ResolutionUse: Display`, which it should not be. Map explicitly:

```rust
let names: Vec<&str> = result.replayed.iter().map(|r| r.branch.as_str()).collect();
```

and join `names`. Keep the surrounding pluralization logic byte-identical — the `""`/`"es"` handling there was a P1 bug fix and must not be disturbed. If `clippy` suggests `itertools::join`, it is not a dependency; use the explicit `Vec<&str>`.

- [x] **Step 3: Fix the unit test that compared the two vectors**

`src/utils/prelude.rs:2870` asserts `first.replayed == second.replayed` in `compose_environment`'s purity test. `ResolutionUse` derives `PartialEq`, so this still compiles and still means the right thing (the whole `ResolutionUse`, key included — which is a *stronger* determinism assertion than the one it replaces). Confirm that is what it now checks and update the surrounding comment if it describes the old comparison.

- [x] **Step 4: Gate**

`just format && just format-check && just lint && just test`.

---

### Task 4: `rebuild_environment_opts` writes the record

**Files:**
- Modify: `tests/integration/rebuild_tests.rs` (add the test first)
- Modify: `src/utils/prelude.rs:985-1010` (build the record, pass it as `extras`)

**Interfaces:**
- Consumes: `PinnedInputs` (`pinned`), `CompositionResult` (`composition`), `new_sha`, `timestamp` — all already in scope at `:956-1005`.
- Consumes: `build_record::{EnvironmentBuildRecord, PinnedBranch, record_blob, resolve_metadata_sha}` and `GitOperations::ref_transaction`'s `RefEdit`.

**Ordering of construction matters.** The record's `result_sha` must be the SHA the transaction actually publishes, and its `held`/`included` must come from the composition that produced it. Both are already in scope, but `composition.result_sha` is currently *moved* into `new_sha` at `:985` — read it before that, or bind `new_sha` by reference from `composition`. The ref edit must be built **before** `publish_environment_build` is called (it is hashed into the object database, and the object must exist before a transaction can point a ref at it), and the `RefEdit` must be `Update { expected_old: Some(String::new()) }` per the header's rationale.

**Put this in `rebuild_environment_opts`, not in `src/commands/rebuild.rs`.** Five other call sites route through it — `promote.rs:286`, `demote.rs:299`, `approvals/approve.rs:338`, and `release.rs:709`/`:712` — so all of them get a correct, current record for free, and none of them can end up publishing a branch whose record is stale. Building the record in the command instead would mean the record is only ever as fresh as the one command someone remembered to instrument, which is the timestamp-inference bug this phase is here to kill. This is worth a test: assert that `hitch promote` also leaves a record whose `result_sha` is the new tip.

- [x] **Step 1: Write the failing test**

In `tests/integration/rebuild_tests.rs`, add `test_rebuild_writes_a_build_record_matching_the_published_branch`. It uses the file's existing `TestSetup::HitchInit` scaffolding and the `inject_branches_into_metadata` helper (`:48`) to promote two branches, runs `hitch rebuild dev`, then:

- `for-each-ref --format=%(objectname) refs/hitch/state/dev` resolves to exactly one OID.
- `cat-file -p` that OID, parse as JSON, and assert: `schema_version == 1`, `environment == "dev"`, `base_name == "main"`, `base_sha == rev-parse main`, `metadata_sha` equals `rev-parse hitch-metadata`, `hitch_version` is non-empty, `result_sha == rev-parse dev`, `held` is empty, `replayed_resolutions` is empty.
- `desired_branches` is `[{branch: "feature-a", sha: rev-parse feature-a}, {branch: "feature-b", sha: rev-parse feature-b}]` — **as a sequence**, so order is asserted, not just membership.
- `included_branches` equals `desired_branches` in this all-clean case.
- `built_at` parses as an RFC 3339 timestamp. Parse the raw JSON string with `chrono::DateTime::parse_from_rfc3339` rather than asserting a shape, so the test survives a switch to a different serialization.

Run it. It must fail with "no such ref: refs/hitch/state/dev" — if it fails any *other* way, the test itself is wrong; fix the test before touching `src/`.

- [x] **Step 2: Build the record at the publish site**

After `let new_sha = composition.result_sha;` (`:985`) and before `publish_environment_build` (`:1002`), construct:

```rust
let desired: Vec<PinnedBranch> = pinned
    .branches
    .iter()
    .map(|(branch, sha)| PinnedBranch { branch: branch.clone(), sha: sha.clone() })
    .collect();

// Filter `pinned` by membership rather than looking each `included` name up in
// it. Composition walks `pinned.branches` in order, so `composition.included`
// is a *subsequence* of it — this yields exactly the same list in promotion
// order, and cannot silently drop a name (a `filter_map` over `included`
// would, and a dropped branch is a record that lies about what was built).
let included: Vec<PinnedBranch> = pinned
    .branches
    .iter()
    .filter(|(branch, _)| composition.included.iter().any(|n| n == branch))
    .map(|(branch, sha)| PinnedBranch { branch: branch.clone(), sha: sha.clone() })
    .collect();
```

Then the `EnvironmentBuildRecord` — `held: composition.held.clone()` (`.clone()` is required: `composition.held` is moved into `RebuildOutcome` at `:1019`, and the record only borrows it), `replayed_resolutions: composition.replayed.iter().map(|r| ResolutionUse { branch: r.branch.clone(), resolution_key: r.resolution_key.clone() }).collect()` after Task 3 has made `replayed` a `Vec<ResolutionUse>`, `result_sha: new_sha.clone()`, `built_at: chrono::Utc::now()`, `hitch_version: env!("CARGO_PKG_VERSION").to_string()`. Then `record_blob` and the `RefEdit`; pass `&[state_edit]` as the new `extras` argument to `publish_environment_build`.

`pinned` and `composition` are read by reference throughout — do not move either. P3 needs `pinned` (for staleness comparison) and `rebuild_environment_opts` returns `composition.held` and `composition.replayed` at `:1018-1021`.

- [x] **Step 3: Make the test green, then prove it is not vacuous**

`just test` with the new test. Then verify it is not vacuous by reverting Step 2 temporarily and confirming the test fails; restore. (A test that passes with the feature disabled is worse than no test.)

- [x] **Step 4: Assert the two-ref transaction is real, not sequential**

Extend the same test with a second `hitch rebuild dev` after advancing `feature-a`. Assert: it succeeds (which is what `expected_old: Some(String::new())` buys — `Create` would have failed this batch), and the record's `result_sha` now equals the *new* `rev-parse dev` and its `desired_branches[0].sha` equals the *new* `rev-parse feature-a`. A record left stale from the first build would fail this.

- [x] **Step 5: Assert `--dry-run` writes nothing**

In the same file, `test_dry_run_does_not_write_a_build_record`: promote, `hitch rebuild dev --dry-run`, then assert `for-each-ref refs/hitch/state/dev` is empty. This is the same non-mutation guarantee P1 established for the composition, extended to the new ref — and it is the assertion that would catch someone "helpfully" writing the record from the dry-run path.

- [x] **Step 6: Assert the record follows `promote` too**

`test_promote_refreshes_the_build_record`: promote a second branch and let `promote` run its rebuild, then assert the record's `desired_branches` now lists both branches and its `result_sha` is the new `dev` tip. This is the assertion that catches someone moving the record construction into `src/commands/rebuild.rs` where `promote` cannot reach it — which would look correct in every rebuild test and be silently wrong in production.

- [x] **Step 7: Gate**

`just format && just format-check && just lint && just test`.

---

### Task 5: The record's content under conflict, replay, and no-record publishes

**Files:**
- Modify: `tests/integration/rebuild_tests.rs` (held, and stale-record cases)
- Modify: `tests/integration/resolve_tests.rs` (replayed, and the no-record scope case)

**Interfaces:**
- Consumes: the reader and record from Task 1/4.
- No `src/` changes expected in this task. If a test needs one, that is a finding: write it down in the phase's Implementation status section rather than patching the source to make the test pass.

- [x] **Step 1: `test_rebuild_records_held_branches_separately_from_included`**

Reuse the scaffolding of `test_hitch_rebuild_ejects_conflicting_branch_by_default` (`tests/integration/rebuild_tests.rs:396`) verbatim — the same branch setup, the same conflict. Do not invent a new one; a divergent fixture here is how the "record matches the build" claim quietly stops being tested against a real eject. After that rebuild, assert the record has:

- `desired_branches` containing the held branch (**it was desired** — that is the whole point of the Desired/Actual distinction),
- `included_branches` **not** containing it,
- one `held` entry with the right `branch` and `conflicts_with`,
- `result_sha` equal to the live `dev` tip, so the record is *correct*, not stale — a hold is a successful publish, not a failure.

- [x] **Step 2: `test_replayed_branch_is_recorded_as_included_with_its_resolution_key`**

Put this in `resolve_tests.rs` directly after `test_dry_run_agrees_with_real_build_about_replayed_resolutions` (`:654`) — that test already builds the recorded-resolution fixture, and duplicating that setup into a new module would be worse than one cross-referenced test in the wrong file. Cross-reference it in a doc comment both ways.

Assert: the replayed branch is in `included_branches`, is **not** in `held`, and its `replayed_resolutions` entry has a non-empty `resolution_key` that equals `rev-parse refs/hitch/resolutions/<key>`'s basename — i.e. the recorded key names a resolution ref that actually exists. A key that does not resolve would make the record a claim with nothing behind it.

- [x] **Step 3: `test_a_promotion_after_a_rebuild_leaves_the_record_describing_the_old_build`**

Rebuild, then promote a second branch (no second rebuild). Assert `refs/hitch/state/dev`'s `desired_branches` still lists only the first branch, and still has the old `result_sha` — and that `rev-parse dev` still matches it.

This is the test that proves the record is a *snapshot of a build*, not a live view of intent. The "needs rebuild" conclusion drawn from this is P3's; P2's obligation is only that the record does not lie about what it recorded.

- [x] **Step 4: `test_resolve_mode_b_publishes_without_a_record_and_leaves_no_usable_actual`**

In `resolve_tests.rs`, reusing `test_resolve_mode_b_worktree_continue_publishes_and_cleans_up` (`:256`) — it already drives a Mode B session to a real publish, so the "did it publish" half is settled and this test only has to ask the new question. After that publish, assert `refs/hitch/state/dev` does not exist.

This is the deliberate gap from Task 2's scope note, pinned down as a test so it is visible and gets revisited, rather than discovered later as a mysterious `LegacyUnknown`. The comment must say what the follow-up is: a Mode B build does produce an environment branch, so it *should* record one, and the missing input is the included-branch list hitch does not currently track for a hand-resolved worktree.

Then extend it: run a plain `hitch rebuild` afterwards and assert the record now exists — i.e. the absence is per-publish, not a one-way door.

- [x] **Step 5: Gate**

`just format && just format-check && just lint && just test`.

---

### Task 6: The two invariants that are easy to break later

**Files:**
- Modify: `tests/integration/crash_recovery_tests.rs:165-240` (additive assertions inside the existing abort loop)
- Modify: `tests/integration/cleanup_tests.rs` (new test)

**Interfaces:**
- Consumes: `refs/hitch/state/<env>`, the `HITCH_TEST_ABORT_AFTER` abort points, `hitch cleanup --apply`.
- No `src/` changes expected. `cleanup.rs:174` is already correct; this task proves it and must not "helpfully" edit it.

- [x] **Step 1: Assert the record cannot disagree with the tip across a crash**

`crash_recovery_tests.rs::test_publish_converges_after_abort_at_each_step` already loops `["journal-written", "ref-moved", "resync-done"]` and already has a `match abort_after` that knows which of those fires before versus after the atomic transaction. Add to it:

- In the `journal-written` arm: assert `for-each-ref refs/hitch/state/dev` is empty. The transaction has not run; nothing may have moved. This is the assertion that makes the phase's central claim testable.
- In the `ref-moved` / `resync-done` arm: assert the state ref resolves, and that its `result_sha == dev_after_abort` — the tip the abort left behind. If the record could ever be written in a different transaction, this is the assertion that fails.

Do not add a new abort point. There is deliberately no window between "tip moved" and "record written" — that is the design — so there is nothing to interrupt. An abort point here would imply the two were separable.

- [x] **Step 2: Prove `cleanup` cannot prune `state`**

In `cleanup_tests.rs`, add `test_cleanup_does_not_prune_the_build_record_ref`. Rebuild the same environment 11 times (enough that `ARCHIVE_REF_RETENTION == 10` at `src/commands/cleanup.rs:166` actually has something to prune), then `hitch cleanup --apply`, then assert **both**:

- `refs/hitch/state/dev` still resolves, and
- `refs/hitch/prev/dev/` is now down to at most 10 entries.

The second half is the non-vacuity requirement. A test that only asserts the state ref survives would pass even if cleanup had stopped pruning anything at all.

- [x] **Step 3: Gate**

`just format && just format-check && just lint && just test`. All four crash-fuzz suites still green, with the two new additive assertions in the rebuild one.

---

### Task 7: Documentation and the manual check

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/superpowers/plans/2026-09-25-explainable-ux-program.md` (P2 implementation status, and any deviation this phase revealed)
- Modify: this file (check off steps, add an Implementation status section)

- [x] **Step 1: Update `AGENTS.md`**

- **Architecture map:** add `src/utils/build_record.rs` alongside the `publish_journal.rs` / `resolutions.rs` entries, with one line on what a build record is and that it rides `publish_branch`'s transaction.
- **Gotchas:** the invariants that are *not* self-evident from the code, and that a future agent would plausibly get wrong:
  - `refs/hitch/state/*` is a **live pointer**, not an archive, and must never join `cleanup.rs:174`'s prunable set. Contrast it with `prev/`/`backup/`, which *are* archives — the two live under the same `refs/hitch/` root and look alike.
  - A missing build record is `LegacyUnknown`, never "probably fine" — and a `resolve`-Mode-B or `release` publish legitimately produces one, so `LegacyUnknown` is a normal state today, not a bug. Say that, or P3 will "fix" it by fabricating a record.
  - The `expected_old: Some(String::new())` on the state ref is deliberate and is *not* the publish-journal CAS mistake Global Constraint 4 warns about. Spell out why (the batch is all-or-nothing behind the branch CAS, so unconditional cannot clobber another publisher).
- Keep the entry terse, in the existing style. This file grows expensive to read if the entries become essays.

- [x] **Step 2: Update the master plan**

Replace P2's status line with `COMPLETE`, add the deviations this phase actually made (there will be at least the two recorded above: `RefEdit` instead of source §6's `PublishExtraRef`, and `CompatibilityConflict` instead of `HoldRecord`; plus `ResolutionUse` being pulled forward from "P2 as authored"), and carry forward whatever P3 must now know.

- [x] **Step 3: Manual end-to-end check**

Per Global Constraint 12. Build the debug binary (`cargo build -p hitch`, **not** `just build` — that is a release build and this is not an abort-hook test, but consistency with the documented recipes matters) and drive a throwaway repo in `/tmp`:

1. `hitch init`, promote two branches, `hitch rebuild dev`.
2. `git for-each-ref refs/hitch/state` — confirm exactly one ref, `refs/hitch/state/dev`.
3. `git cat-file -p refs/hitch/state/dev` — read the JSON and confirm the SHAs match what `git rev-parse` says *right now*, that the branch order is the promotion order, and that `result_sha` is `dev`'s tip.
4. `hitch rebuild dev` again — confirm it succeeds (the `Create`-would-break case) and the record updated.
5. Make one branch conflict, `hitch rebuild dev` again — confirm exit code 2, the branch in `held` and *not* in `included_branches`, and the record still matching the live tip.
6. `git push` the state ref away, or `git update-ref -d refs/hitch/state/dev`, and confirm nothing crashes and nothing reports a plausible-but-wrong Actual.

Record the actual outputs in this plan's Implementation status section, the way P1 recorded its manual check.

- [x] **Step 4: Final gate**

`just format`, `just format-check && just lint`, `just test`. All clean. Then stop — do not start P3.

---

## Exit criteria

From the master plan, restated so they are checkable:

- [x] The record's `result_sha` matches the published branch tip, or the record is absent. Never inconsistent — because it rides the same transaction, and the crash-fuzz loop asserts it at all three abort points.
- [x] Held branches are recorded in `held` and absent from `included_branches`; replayed branches are recorded in `replayed_resolutions` with a key that names a real resolution ref, and present in `included_branches`.
- [x] A promotion after a build leaves the record describing the *old* build (snapshot, not live view). P3 turns this into "needs rebuild".
- [x] A corrupt, mismatched, or future-schema record is an explicit `Unreadable`/`ResultMismatch` value, never a guess and never an `Err` that would break `status`.
- [x] `hitch cleanup --apply` cannot prune `refs/hitch/state/*`, proven alongside a prune that does fire.
- [x] `--dry-run` writes no record.
- [x] `hitch release` and `resolve` Mode A / Mode B publish with `extras: &[]` and their crash-fuzz suites pass untouched.
- [x] All four gates green; `AGENTS.md` and the master plan updated in the same change.

---

## Implementation status

**COMPLETE** (2026-09-25). 39 steps, 8/8 exit criteria. 9 unit tests in `src/utils/build_record.rs`, 9 new integration tests across `rebuild_tests.rs` / `resolve_tests.rs` / `crash_recovery_tests.rs` / `cleanup_tests.rs`. Final gate: 68 lib / 365 mod (2 ignored) / 1 no_args_help, format and clippy clean.

### Deviations from the plan as authored

1. **`RefEdit` instead of source §6's `PublishExtraRef`** (Task 2). Field-for-field duplicate of the existing type; no reason to have two.
2. **`held: Vec<CompatibilityConflict>` instead of §6's `HoldRecord`** (Task 1). Same three fields, and already what `CompositionResult.held` holds. A third conflict shape would be strictly worse than reusing the one, so `CompatibilityConflict` gained only additive serde derives plus a stale doc-comment fix (it still said "as found by `preflight_compatibility_report`", untrue since P1).
3. **`ResolutionUse` pulled forward** (Task 3, as scoped in the header). `CompositionResult.replayed` became `Vec<ResolutionUse>`. The display sites in `commands/rebuild.rs` were repointed to map to `.branch` — the CLI has never shown keys and this was not the place to start.
4. **`read_state` probes `schema_version` before the full parse** (Task 1 Step 4, unplanned). See finding 1.
5. **Task 2's "four call sites" is three.** The plan's claim that `release.rs` has two `publish_branch` calls (`:334` branch push, `:355` tag push) was wrong: `:355` is a *comment* mentioning `publish_branch`, and the tag push goes through something else. One call in `release.rs`, two in `resolve.rs`. Three in-crate test call sites in `prelude.rs` also needed the mechanical argument — a signature change cannot avoid that, so the plan's "the existing suite must pass unchanged" is read as "must need no *behavioural* edit", which is what held.
6. **`publish_branch` needed `#[allow(clippy::too_many_arguments)]`** (8 params). The house convention at `prelude.rs:1545` is a bare allow plus a one-line reason, so that is what it got.
7. **Task 6 Step 1's `journal-written` assertion was wrong, and the correct one is stronger.** The plan said to assert `refs/hitch/state/dev` is *empty* at that point. It is not empty: `setup_with_pending_second_promotion` calls `setup`, which promotes — and so rebuilds — so a legitimate record from that build already exists. The assertion actually made is that the interrupted build left the ref **byte-identical** (`state_oid == state_oid_before`), which is the real invariant and strictly stronger: not merely "no new record" but "the record was not touched at all".
8. **Task 6 Step 2 plants archive refs rather than rebuilding 11 times.** Rapid rebuilds collide on the `backup_timestamp` ref name (same second) and would overwrite each other, so they would not reliably produce 11 prunable refs. Planting 15 refs matches the existing `test_cleanup_prunes_old_archive_refs_per_env_per_namespace` directly above, and the test still does a *real* `promote` to produce a *real* record — so what is under test is cleanup sparing it, not a hand-made blob.

### Findings

1. **The `schema_version` gate was unreachable for the case it exists for.** Writing `read_state` as parse-then-check means a record from a newer hitch — which by construction has every field this hitch knows *plus* fields it does not — fails the full parse on the first unknown field and gets reported as a corrupt blob. That is both the wrong diagnosis and the misleading one: the record is perfectly valid, just newer. The fix is a `VersionProbe` deserialized on its own first, and the ordering is now load-bearing and commented as such. Caught by `read_state_refuses_a_newer_schema`, which was written with a deliberately partial JSON body and failed for this reason.

2. **`metadata_sha` is not observable from outside the command, and the plan overclaimed for it.** Documented as "a recorded fact and a fast path" answering "did the declaration change at all". The integration test asserting `metadata_sha == rev-parse hitch-metadata` failed, and the manual check found out why. A rebuild brackets the commit it records on **both** sides with its own metadata writes:

   - `with_locked_env` commits `locked: true` / `locked_at` **before** the declaration is read, so the declaration actually came from the lock commit;
   - publishing then commits `rebuilt_at` and the unlock **after**.

   Observed in `/tmp/hitchdbg/repo`: record says `455f1b6`, branch tip is `e287007`, two commits later, and `git diff` between them shows exactly the lock→unlock + `rebuilt_at: null`→stamp transition. So the recorded SHA is a commit that only ever existed as a transient tip, is unobservable from outside, and is always a strict ancestor of the final tip. The test now asserts what is actually true (full SHA, a real commit, an ancestor, and *not* the tip) and the field's doc comment says plainly that it cannot be a staleness signal. P3 must not read it for correctness.

   The two additional reasons it still fails as a signal — it moves on an unrelated environment's promotion, and it returns to a prior value when a declaration edit is reverted — were already known; this is a third, and the strongest.

3. **Non-vacuity was proven for all three of Task 4's tests** by temporarily passing `&[]` as `extras` and confirming all three fail. The dry-run test fails too, via its built-in second half (a real rebuild *does* write a record), so the "must not write" assertion cannot pass for the wrong reason.

### Manual end-to-end check

Debug binary (`cargo build -p hitch`), throwaway repo in `/tmp/hitchp2/repo`. Raw outputs, not paraphrase.

| # | Step | Observed |
|---|---|---|
| 1 | `hitch init`, promote `feature-a` then `feature-b`, `rebuild dev` | exit 0, `Rebuilding environment 'dev'` |
| 2 | `for-each-ref refs/hitch/state` | exactly one ref: `refs/hitch/state/dev` (a `blob`) |
| 3 | `cat-file -p refs/hitch/state/dev` | `result_sha` = `dev` tip; `base_sha` = `main` tip; `desired_branches` in promotion order `feature-a`, `feature-b`, each SHA equal to that branch's live tip; `included == desired`; `held`/`replayed_resolutions` empty; `built_at` `2026-09-25T18:20:02.361628Z`; `hitch_version` `1.3.8` |
| 4 | advance `feature-a`, `rebuild dev` again | exit 0 — the `Create`-would-break case passes — and the record updated to the new `result_sha` and the new `feature-a` SHA |
| 5 | make `feature-b` collide, `rebuild dev` | exit 2, `Held 'feature-b' — conflicts with 'feature-a' (1 file)`; record: `desired` = both, `included` = `feature-a` only, `held` = `('feature-b', 'feature-a', ['feature-a.txt'])`; `result_sha` still equal to the live tip — a hold is a successful publish |
| 6 | `git update-ref -d refs/hitch/state/dev`, then exercise readers | `status` 0, `rebuild --dry-run` 2, `conflicts` 2, `doctor` 0 — no panics, no crash; a subsequent real rebuild recreated the record |

**Caveat on step 6, stated so nobody over-reads it:** "nothing crashes when the record is missing" is currently *trivially* true, because `build_record::read_state` has no production caller yet — P2 built the writer, P3 wires the reader. The only consumer of the missing-record path today is the unit test. Step 6 confirms the ref's absence does not disturb anything that reads refs; it does **not** yet demonstrate graceful degradation in `hitch status`, because nothing reads the record there. Do not cite it as evidence that P3's reader is safe.

### What P3 inherits

- `read_state` is written, unit-tested against all four states, and **unused**. P3's `RepositoryStateSnapshot` is its first caller.
- `desired_branches` (per-branch SHAs, promotion order) is the only usable staleness input. `metadata_sha` is not.
- `LegacyUnknown` is a normal, expected state for any environment last published by `release`, `resolve` Mode A, or `resolve` Mode B.
- The record is a snapshot of a build. After a `--no-rebuild` promotion it is *correctly* stale, and a test pins that.
- `src/utils/prelude.rs` line numbers moved again (the record construction sits between the compose and the publish, ~55 lines). Re-resolve before citing.

