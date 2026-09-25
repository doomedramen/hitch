# P3 — State model

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. All 43 boxes below are ticked; see the closing sections for what actually landed.

**Goal:** One shared read-only `RepositoryStateSnapshot` that `status`, the desktop workspace index, and every future planner consume, with staleness decided by comparing pinned SHAs instead of timestamps.

**Architecture:** New `src/core/state.rs` holds the types and the one function that builds them (`build_state_snapshot`). It reads `hitch.json` for Desired, reads `refs/hitch/state/<env>` for Actual, and compares **commit SHAs**, never timestamps. Everything downstream — `core/status.rs`, `core/details.rs`, `commands/status.rs` — consumes the snapshot instead of re-deriving staleness.

**Tech Stack:** Rust, `anyhow`, `serde`, `chrono`, `GitOperations` plumbing, `HitchTestFramework`.

**Spec section:** §11.1 (Desired / Actual / Proposed), §11.2 (`EnvironmentHealth`). §12's *renderer* is P7's job; P3 produces the data §12 will draw and no glyphs.

---

## Global Constraints

- **`file:lines` refs below were resolved against `1f5b2dd` + `310de28` on `explainable-ux`.** Re-resolve if the tree has moved.
- **This phase changes no mutation behaviour.** Nothing here writes a ref, takes a lock, fetches, or pushes. It is a pure read path. `git diff --name-only main..explainable-ux -- crates/` must stay empty.
- **Timestamps are for presentation, never for correctness** (spec §11.2). `built_at` and `rebuilt_at` may be *displayed*; no verdict may depend on comparing them to a commit date.
- **`metadata_sha` is not a staleness input.** It is a recorded fact that is always a strict ancestor of the build's final tip and is unobservable from outside the command (see `src/utils/build_record.rs:79-95`). Staleness comes from `desired_branches` / `base_sha` in the record versus live refs.
- **`PinnedInputs.branches` order is load-bearing** (promotion order). Never sort a record's branch lists, and never sort the declaration.
- **The snapshot must not touch the network.** The current code reaches `branch_exists_anywhere`, which shells out to `git ls-remote --heads origin` once *per branch* — an N-round-trip read in a command that is otherwise local. P3 resolves refs with `rev_parse_opt("refs/heads/<b>")` falling back to `rev_parse_opt("refs/remotes/origin/<b>")`: both are object-database reads against already-fetched remote-tracking refs.
- **A display preflight is still a display preflight.** `commands/status.rs` calls `preflight_compatibility_report_local` to show a `⛔` for a branch that *would* be held next rebuild. That is a prediction about the next build, and it stays out of `ActualComposition`, which by spec §11.1 reports what the last build *did*. The two must not be conflated. Re-plumbing `resolve`'s mode selection is P4's job (Global Constraint in the master plan), not this phase's.
- **`RebuildState` and `commands/status.rs`'s private `RebuildStatus` are deleted, not extended.** There are currently **four** copies of the timestamp comparison (`core/status.rs:103`, `commands/status.rs:258` inline per-branch, `commands/status.rs:316` base, `commands/status.rs:406`). Leaving any of them would recreate exactly the two-ways-to-one-answer divergence P1 just removed. A second verdict enum is the same bug in a different shape.

  > **Corrected during execution — there were more than four.** The count above is
  > of *distinct comparison sites*, and it is accurate as such, but it badly
  > understates the blast radius: both summary blocks in `commands/status.rs`
  > also called `determine_rebuild_status` per environment, so a single
  > `hitch status` computed the same verdict once per environment in the top
  > summary, again in the body, and again in the bottom summary. Six call sites
  > reaching three implementations. See Finding 1 below.
- **Non-vacuity rule:** every test asserting a new verdict must be checked to fail when the new code path is bypassed. A test that passes both before and after the change proves nothing.

---

## Deviations from the spec's §11.2 sketch, and why

The spec says "Expose state such as" and gives `NeedsRebuild { changed_inputs: Vec<String> }`. Two refinements, both forced by the spec's own §11.2 example output:

1. **`ChangedInput` carries both SHAs, not a `String`.** §11.2's own illustration of the improvement is `feature/auth changed / 2a42d1c → 7c931af` — "more accurate *and easier to explain*". A bare `Vec<String>` of branch names cannot render that arrow, so the sketch's own stated goal is unreachable with the sketch's own type.
2. **`PartiallyRealised` yields to `NeedsRebuild` when both are true.** Precedence is documented on the enum. Rationale: `PartiallyRealised` is a durable claim about the build that produced the current tip, whereas `NeedsRebuild` means that build is *superseded* — reporting "partially realised" for a build whose inputs have since moved would be a claim about history masquerading as a statement about now.

---

## Tasks

### Task 1 — The state types

**Files:** `src/core/state.rs` (new), `src/core/mod.rs` (register `pub mod state;`)

**Interfaces:** produces, all `pub`:

- `RepositoryStateSnapshot { metadata_sha, current_branch, environments, features, captured_at }`
- `EnvironmentState { name, base, desired, actual, health, locked, approval_policy }`
- `ApprovalPolicy { required, min_approvals, approvers }`
- `DesiredComposition { base, base_sha: Option<String>, branches: Vec<DeclaredBranch> }` where `DeclaredBranch { name, sha: Option<String> }` (`None` = the branch does not resolve locally *or* on the cached remote ref)
- `ActualComposition` — `FromRecord(Box<RecordActual>)` | `LegacyUnknown` | `Unreadable { reason }`
- `RecordActual { tip, base_sha, included, held, replayed_resolutions, built_at, record }`
- `ActualMembership` — `Included` | `Held` | `AlreadyInBase` | `Missing` | `Unknown`
- `EnvironmentHealth` — `Realised` | `PartiallyRealised { held }` | `NeedsRebuild { changed_inputs: Vec<ChangedInput>, added: Vec<String>, removed: Vec<String> }` | `NeverBuilt` | `LegacyUnknown` | `MissingBranch`
- `ChangedInput { branch, previous_sha, current_sha }` with a `short()` helper for display
- `FeatureState { name, memberships: Vec<FeatureMembership> }` and `FeatureMembership { environment, desired, actual }`

- [x] Write the module header. It must state, in the words a future agent needs: the Desired/Actual split is a *fact* boundary not a *style* choice; absence of a record is `LegacyUnknown` and never a licence to guess; and the health verdict is derived from SHAs.
- [x] Declare the enums and structs. `Derive(Debug, Clone, PartialEq, Eq)` throughout so tests can compare whole environments. `RecordActual.held` is `Vec<CompatibilityConflict>` (reused from `prelude`, as P2 did — not spec §6's `HoldRecord`).
- [x] Doc-comment `EnvironmentHealth`'s precedence rule, and `MissingBranch` being checked *before* any record read.
- [x] `impl ChangedInput { pub fn short(&self) -> (&str, &str) }` returning 7-char prefixes for the §11.2 arrow.
- [x] `impl RecordActual { pub fn membership_of(&self, branch: &str) -> ActualMembership }` — included/held lookup, `Unknown` for a branch the record never mentions.
- [x] `impl EnvironmentHealth { pub fn is_actionable(&self) -> bool }` — true for `NeedsRebuild` / `NeverBuilt` / `MissingBranch`. One predicate so summary counting never re-derives the question.
- [x] Add `pub mod state;` to `src/core/mod.rs`.
- [x] `cargo build -p hitch` clean.

---

### Task 2 — `build_state_snapshot`: Desired

**Files:** `src/core/state.rs`

**Interfaces:** `build_state_snapshot(context: &GlobalContext) -> Result<RepositoryStateSnapshot>`

- [x] Test first: a scratch repo with two environments, one feature promoted to both, a remote-only feature, and one promoted branch whose ref does not exist. Assert `desired.branches` preserves *declaration* order, and that a non-resolving branch gets `sha: None` rather than being dropped.
- [x] Implement Desired: read config via `access_metadata_read_only`; resolve each declared branch's sha with the local-then-remote-tracing `rev_parse_opt` pair from Global Constraint 5. Build `features` by inverting the declaration — every branch that appears in any environment's `branches` list, each carrying one `FeatureMembership` per environment that declares it.
- [x] `metadata_sha` from `rev_parse_opt("refs/heads/hitch-metadata")` (not `get_branch_commit_sha`, which falls back and errors). It is reported, never compared.
- [x] Run the test.

---

### Task 3 — `build_state_snapshot`: Actual, and the health verdict

**Files:** `src/core/state.rs`

**Interfaces:** the same `build_state_snapshot`, now complete; plus private `live_sha(git, branch) -> Option<String>`

- [x] Test first — the rebase case. Build, then `git rebase` a feature so its commits are *newer by date but different SHAs*; assert `NeedsRebuild` with `ChangedInput { previous_sha, current_sha }`. This is the bug the whole phase exists to fix and it must fail against the timestamp heuristic.
- [x] Test first — the skewed-clock case. Build, then commit to a feature with `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE` set to **before** the build; assert `NeedsRebuild`. Under the old code this reads as up to date.
- [x] Test first — base moved after rebuild. Assert `NeedsRebuild` naming the base in `changed_inputs`.
- [x] Test first — declaration moved after rebuild (`--no-rebuild` promote). Assert `added` / `removed` are populated, since no branch SHA changed.
- [x] Test first — held branch, nothing else moved. Assert `PartiallyRealised { held: ["branch-b"] }`.
- [x] Test first — no record, `rebuilt_at: None` → `NeverBuilt`; no record, `rebuilt_at: Some` → `LegacyUnknown`; no env branch ref → `MissingBranch`; corrupt record → `LegacyUnknown` with the reason preserved in `ActualComposition::Unreadable`.
- [x] Implement: per environment, in this order — (a) `refs/heads/<env>` missing ⇒ `MissingBranch`, stop; (b) `read_state`; (c) `LegacyUnknown` / `Unreadable` ⇒ `LegacyUnknown`, stop; (d) `ResultMismatch` ⇒ treat the record's membership as stale-but-present and still compute `changed_inputs` from the *record's* pinned SHAs, so the verdict names what changed since that build; (e) `Known` ⇒ compare `record.base_sha` and each `record.desired_branches` entry against live refs, plus the declared-vs-recorded branch-name sets for `added` / `removed`.
- [x] Populate `actual` and `features[].memberships[].actual` from the same derivation — one code path, so the environment view and the feature view cannot disagree.
- [x] `ActualMembership` resolution order, documented on the function: record speaks about this branch ⇒ its answer wins (`Included` / `Held`) even if the branch has since been deleted, because the build demonstrably contained it; otherwise live `is_branch_merged_into(branch, base)` ⇒ `AlreadyInBase`; otherwise branch unresolvable ⇒ `Missing`; otherwise ⇒ `Unknown`.
- [x] Run the tests.

---

### Task 4 — Delete the timestamp verdict from `core/status.rs`

**Files:** `src/core/status.rs`, `src/core/details.rs`

- [x] Change `build_status_model(context)` → `build_status_model(context, snapshot: &RepositoryStateSnapshot)`. It must take the snapshot rather than build its own — two builds of the same snapshot cannot disagree, and building twice is what let the two views drift in the first place.
- [x] `EnvironmentStatusModel` gains `state: EnvironmentState` and **loses** `rebuild_state: RebuildState`. Delete `enum RebuildState` and `fn determine_rebuild_state` outright.
- [x] Replace the two summary counters' `matches!` filters with `health.is_actionable()` for the "needs rebuild" count, and an explicit `matches!(…, NeverBuilt)` for the "never rebuilt" count. The distinction is deliberate: never-built is a subset of actionable, not an alternative to it, and the two counts are labelled differently in the UI.
- [x] `details.rs`: `build_environment_details_model` calls `build_state_snapshot(context)?` and passes it through. `build_env_overview` matches on `env.state.health` and renders a line for every variant — `Realised`, `PartiallyRealised`, `NeedsRebuild`, `NeverBuilt`, `LegacyUnknown`, `MissingBranch`. `LegacyUnknown` must read as a first-class honest state ("actual unknown — no build record; this environment was last built by a hitch that did not record one"), never as "up to date".
- [x] Grep for `RebuildState` and confirm zero hits outside the two files just edited.
- [x] `cargo build -p hitch` clean.

---

### Task 5 — Delete the timestamp verdict from `commands/status.rs`

**Files:** `src/commands/status.rs`

This file holds **three** of the four copies.

- [x] In `display_environment_status`, thread the environment's `EnvironmentState` in (the function already receives `config`; add the snapshot, or pass the one `EnvironmentState` it needs). Derive `is_stale` for a branch from `health.changed_inputs` membership, not from `get_commit_timestamp`; derive `base_is_stale` the same way.
- [x] Replace `determine_rebuild_status` and `enum RebuildStatus` with the snapshot's health. Render `LegacyUnknown` and `MissingBranch` explicitly — today there is no way for `hitch status` to say "I don't know what this environment contains", which is precisely what this phase removes.
- [x] Confirm `get_commit_timestamp` now has **zero** callers in `src/` outside `git_operations.rs` itself. If so, note in `AGENTS.md` that it is retained for the P9 activity log rather than left looking live.
- [x] The `⛔` held glyph keeps its `preflight_compatibility_report_local` source (Global Constraint 7) — but the *line's wording* must not imply the branch is currently held. Distinguish "held in the last build" (from the record) from "would conflict on the next rebuild" (from the preflight).
- [x] `cargo build -p hitch` clean.

---

### Task 6 — Integration tests

**Files:** `tests/integration/status_tests.rs`, new `tests/integration/state_model_tests.rs`

- [x] **Un-ignore `test_hitch_status_detects_base_branch_changes` and `test_hitch_status_multiple_envs_with_changed_base`.** They are `#[ignore = "Timing-sensitive test: relies on git commit timestamps being newer than rebuild timestamp"]` — that dependency is the bug, and this phase is what removes the reason for the attribute. Delete the `sleep(2)` calls too.
- [x] `test_status_shows_per_branch_staleness`: delete the `--date 2099-01-01` future-timestamp hack and its comment. A plain commit must now be enough. That the test needed a fake future date at all is the clearest single statement of what was wrong.
- [x] New tests, one per exit criterion: same branch promoted to two environments (membership differs per environment, consistently); remote-only feature (`DesiredComposition` resolves it from the remote-tracking ref, and it is *not* `Missing`); missing branch (`Missing` membership, and an environment health that does not claim a confident Actual); feature already integrated into base (`AlreadyInBase`); base changed after rebuild; feature changed after rebuild; locked and approval metadata represented.
- [x] **The agreement test**: build the snapshot and the status model from the same repo and assert they report the same verdict per environment. This is the exit criterion "status and the snapshot agree" stated as a test rather than as a hope.
- [x] **The legacy test**: an environment last published by `hitch release` (no record) renders as `LegacyUnknown` and `hitch status` still exits 0 and says so. This is the case P2's plan flagged as *trivially* true and explicitly not demonstrated — the reader finally has a production caller, so it can be.
- [x] **Non-vacuity**: temporarily re-introduce the timestamp comparison in one derivation, confirm the new tests fail, then revert. Record which tests caught it.

---

### Task 7 — Docs

**Files:** `AGENTS.md`, master plan, this file

- [x] `AGENTS.md`: add the `refs/hitch/state/*` reader to the architecture map; add a gotcha recording that the timestamp heuristic is gone and why re-introducing it is a regression, and that `metadata_sha` is not the staleness input. If `get_commit_timestamp` loses its last caller, say so.
- [x] Master plan: mark P3 complete with its exit criteria and deviations.
- [x] This file: fill in "As executed" — deviations, findings, manual-check table — and write "What P4 inherits".
- [x] Manual check against a throwaway repo in `/tmp`, per `AGENTS.md`: build `dev`, then rebase a feature and confirm the SHA-based message renders; then `hitch release` an environment and confirm it says "actual unknown" rather than "up to date". Record both transcripts.

---

## As executed

All seven tasks landed on `explainable-ux` in a single commit. `just format`,
`just format-check`, `just lint`, and `just test` are green; the suite is
68 lib + 387 integration + 1 `no_args_help`, **zero ignored** (P3 removed the
last two `#[ignore]`s). `git diff --name-only main..explainable-ux -- crates/`
is empty.

### Deviations from the plan

1. **`ChangedInput` instead of §11.2's `Vec<String>`** — as predicted in
   "Deviations", above, and now non-negotiable: the spec's own example output
   (`2a42d1c → 7c931af`) is unreachable with a `String`.
2. **`EnvironmentBuildRecord` gained `PartialEq, Eq`** (`utils/build_record.rs`).
   Tests compare a whole record against a freshly-read one; without the derive
   every such assertion degrades into field-by-field prose. P2's module did not
   need it and so did not have it.
3. **Presentation fields moved onto `EnvironmentState`.** `locked_by`,
   `locked_at`, `rebuilt_at`, and `released_at` live on the snapshot's
   environment state rather than being re-read from config by each renderer.
   The plan had `core/details.rs` and `commands/status.rs` each reaching into
   config for the same four values; that is a second-opinion bug waiting to
   happen, and the snapshot already reads config, so it carries them.
4. **`build_status_model` is infallible and takes only the snapshot.** The plan
   said `build_status_model(context, snapshot: &…)`. It became
   `build_status_model(&RepositoryStateSnapshot) -> StatusModel` with no
   `Result` and no context. A pure projection *cannot* drift from its input, and
   a `Result` on a function that reads nothing fallible was a lie about its
   failure modes.
5. **`membership_within` is the real membership query; `EnvironmentState::membership_of`
   is record-only.** The plan's exit criteria need a membership that consults
   the record *and* the live base, because "already in base" is not in any
   record — it is a live fact. `membership_of` (record-only) answers
   "what did the last build do with this branch"; `membership_within(branch,
   env)` answers "what is this branch's status in this environment", which is
   the question §11.1's feature view actually asks. Both exist because both
   are real; conflating them was the alternative.
6. **`MissingBranch` is decided before the record is read.** Ordering, not
   design: a declared branch with no ref anywhere is missing whether or not we
   happen to know what the last build contained. Reading the record first would
   let a `LegacyUnknown` shadow a fact we can establish for free.
7. **`NeedsRebuild` outranks `PartiallyRealised`.** As planned; recorded on the
   enum with the rationale, because it is the one precedence choice a future
   reader will want to "fix".
8. **The snapshot is offline and the *renderer* now is too.** The plan scoped
   "no network" to `build_state_snapshot`. Task 5 discovered the render loop
   still called `branch_exists_anywhere` — a `git ls-remote --heads origin`
   per promoted branch — and switched it to read existence out of the snapshot
   it already had. So `hitch status` went from O(branches) round trips to
   zero, which was not on the plan and is the single most user-visible
   performance change in the phase.

### Findings

1. **The timestamp comparison had more reach than the plan's count implied** —
   six call sites across three implementations, as noted in Global Constraints
   above. `hitch status` was computing one verdict up to three times per
   environment. Any one of those three could have drifted independently, and
   the reason it had not produced a visible bug is that all three were
   literally the same function — the divergence risk was structural, not
   actual, and P3 removes the structure anyway.
2. **No existing test asserted the wrong behaviour.** The full suite passed
   with **zero** test changes after the rewrite. That is the strongest argument
   for Task 6 existing: a bug this old, this central, and this wrong had no
   test standing behind it in either direction, so a green suite before and
   after proved nothing at all. The suite was measuring that the code did not
   crash, not that it was right.
3. **The real driver of `LegacyUnknown` is a pre-P2 repo, not `hitch release`.**
   Task 6's "legacy test" was specced as "an environment last published by
   `hitch release`". `hitch release` prunes the integrated environment branch
   as part of its job, so that environment reads as `MissingBranch` — with
   `ActualComposition::LegacyUnknown` underneath — and `release`'s *default*
   dependent rebuild writes a fresh record for everything downstream of the
   target anyway. Getting the plain recordless case required
   `--no-rebuild-dependents`. The test now simulates what actually happens in
   the field: build for real, assert `refs/hitch/state/dev` exists (so it
   cannot pass vacuously), then `git update-ref -d` it.
4. **Both timing-`#[ignore]`d tests asserted too little to notice the bug.**
   Their assertion was `stdout.contains("Rebuild needed") || stdout.contains("main has newer commits")` — an `||` over two loose strings, satisfiable by whichever environment rendered first. Replaced with a count of the SHA arrows (2, because two environments share a base) plus the per-environment rebuild hint. They were ignored *and* weak; both are now fixed.
5. **A test asserting a command's output must not assert on a colourised label.** The multi-environment test's first draft asserted `stdout.contains("Rebuild dev")`, which fails for ever: the suggestion list colourises the environment name with ANSI escapes, so the literal substring never appears. Assert the command (`hitch rebuild dev`) or the uncoloured prose, never a label that passes through a styler.

### Non-vacuity

Two probes, one derivation each, reverted after:

| Probe | Change | Tests that failed |
|---|---|---|
| A | Drop the base-SHA comparison in `health_from_record` | `test_hitch_status_detects_base_branch_changes`, `test_hitch_status_multiple_envs_with_changed_base` |
| B | Drop the per-branch SHA comparison in `health_from_record` (base check restored) | `test_a_rebased_feature_is_needs_rebuild_even_though_its_commits_are_newer`, `test_a_backdated_commit_still_counts_as_needs_rebuild`, `test_status_shows_per_branch_staleness` |

Together those are the old heuristic's exact blindness, split so each failure
is attributable. A third probe (deleting the whole function) was not run: it
would fail every test and teach nothing.

The `--date 2099-01-01` removal is the qualitative version of the same check.
A staleness test that has to fake a future date to see a *content* change is
demonstrating the defect in its own setup; deleting the hack and watching the
test still pass is a stronger result than any probe.

### Manual check

Against `/tmp/hitchp3/repo`, per `AGENTS.md`'s rule for user-visible CLI
changes. `cargo build -p hitch` (debug), a fresh `git init` + `hitch init` +
`hitch add dev` + `hitch promote feat dev`.

| Step | Result |
|---|---|
| status after a clean promote | `✅ Up to date`, 0 need rebuild, `✅ 1. feat` |
| commit to `main`, then `git rebase main` on `feat` | `1 need rebuild`; `🔄 1. feat (new commits since last rebuild)`; Status shows **both** moved inputs by SHA: `⚠️ 8273abe → 6be37e2  main` and `⚠️ 5d2978b → 75984c0  feat`; suggestion `hitch rebuild dev` |
| `git update-ref -d refs/hitch/state/dev` | `❓ Actual unknown — no build record for this environment (it was last built by a hitch that does not record builds, or published by 'hitch release'/'hitch resolve')`, **exit 0**, no "Up to date", still offers `hitch rebuild dev` |

The rebase row is the one that matters: under the old code that repository
would have reported *up to date*, because the rebased commit is brand new and
therefore newer than any `rebuilt_at`.

---

## What P4 inherits

1. **`build_state_snapshot` is the only place a staleness verdict may be
   computed.** P4 builds a *future* state, so: build the plan, then project it
   through the same comparison logic rather than re-deriving staleness inline.
   Do not call `get_commit_timestamp` (one production caller left,
   `core/timeline.rs:96`, formatting a date for display) and do not call
   `read_state` outside `state.rs`.
2. **`core/status.rs` is a pure projection and should stay that shape.**
   `build_status_model(&snapshot) -> StatusModel`, no context, no `Result`.
   P4's preview output wants the same property: a preview that cannot disagree
   with what would happen, because it is computed from the same inputs.
3. **P1's open item is P4's.** `commands/resolve.rs:131,180` still selects its
   resolution mode from `preflight_compatibility_report`, a second merge
   opinion. P3 made the prediction/fact distinction *visible* (the ⛔ now
   consults the record's `held` list first and words the two cases
   differently) but did not fix the underlying re-plumbing, which needs a plan
   to choose a mode from — which is what P4 builds. `preflight_compatibility_report_local`
   is still called from `conflicts.rs:44`, `status.rs`, and `tree.rs:138`; those
   are display callers and are legitimate.
4. **`hitch status` is now offline end to end, and that is a guarantee to
   preserve.** The snapshot resolves `refs/heads/<b>` then
   `refs/remotes/origin/<b>` and never fetches. A new status-shaped path
   inherits that for free; a new `ls-remote` in one does not.
5. **`is_actionable()` is the single predicate for "should the user be told to
   do something".** `NeverBuilt` is a subset of it, not an alternative, and
   `LegacyUnknown` is *not* actionable even though `hitch rebuild` would fix it
   — it is a different signal ("I don't know") and conflating the two in a
   counter is how a status line starts lying. A new summary counter should
   call the predicate, not re-list variants.
