# P0 — Scenario Inventory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish, before any refactoring begins, exactly which of the source spec §28 Milestone 0's scenarios the existing 354-test suite already covers — and record the current exit codes and ref effects that P1's "pure move" claim will be measured against.

**Architecture:** No production code changes. This phase reads `tests/integration/*` and `tests/scenarios/*`, classifies each required scenario, closes the two genuinely-missing assertions with a new fixture, and writes down the observable behaviour (exit codes, ref names, journal state) that later phases must not change.

**Tech Stack:** Rust, `HitchTestFramework` (`tests/test_framework/`), the existing integration suites.

**Parent plan:** `docs/superpowers/plans/2026-09-25-explainable-ux-program.md` (P0). Read its Global Constraints before starting; Constraint 1 (`-p hitch` only) applies to every command in this plan.

---

## Scope boundary

This is the **scenario-inventory slice** of Milestone 0 only. It is *not* the baseline repair. `main` currently fails CI three independent ways — a newer-clippy `?`-operator lint, a Windows-only temp-path test failure, and the `hitch-desktop` compile errors — and all three are tracked on a separate stream. Do not fix them here, and do not widen any `just` gate to cover the desktop crate.

If a CI breakage lands mid-program and makes `just lint` fail, that is the clippy lint arriving on a newer toolchain, not a regression from this work.

---

## Scenario inventory (surveyed 2026-09-25)

The source spec §28 M0 lists 13 required scenarios. Twelve are already covered; one (`--no-push`) is only half-covered and needs two assertions. Every claim in the table below was verified against the test bodies on 2026-09-25 — see Task 1 for the four load-bearing reads and what they turned up.

| # | Scenario | Status | Covering tests |
|---|---|---|---|
| 1 | clean promote + rebuild | **covered** | `promote_demote_tests::test_hitch_promote_basic`, `::test_hitch_promote_demote_workflow`, `rebuild_tests::test_hitch_rebuild_basic` |
| 2 | demote | **covered** | `promote_demote_tests::test_hitch_demote_basic`, `::test_demote_no_rebuild_skips_rebuild`, `::test_demote_rollback_functionality` |
| 3 | rebuild with held branch | **covered** | `rebuild_tests::test_hitch_rebuild_ejects_conflicting_branch_by_default`, `::test_hitch_rebuild_dry_run_reports_held_branch`, `conflicts_tests::test_conflicts_reports_held_branch_and_policy` |
| 4 | rebuild with `halt` | **covered** | `rebuild_tests::test_hitch_rebuild_on_conflict_halt_flag_restores_all_or_nothing` |
| 5 | recorded-resolution replay | **covered** | `resolve_tests::test_replay_composes_recorded_resolution`, `::test_replay_is_a_miss_after_branch_moves`, `::test_replay_is_a_miss_after_branch_head_is_amended` |
| 6 | clean release | **covered** | `release_tests::test_hitch_release_basic`, `::test_hitch_release_preserves_ancestry_for_stacked_branches`, `::test_hitch_release_with_default_target_branch` |
| 7 | failed release conflict | **covered** | `release_tests::test_hitch_release_with_conflicts`, `::test_hitch_release_is_atomic_when_a_later_branch_conflicts` |
| 8 | dependent env rebuild after release | **covered** | `release_tests::test_hitch_release_rebuilds_dependent_environments_in_order`, `::test_hitch_release_rebuilds_the_released_environment_even_when_locked` |
| 9 | prune after release | **covered** | `release_tests::test_hitch_release_prunes_promoted_branches_in_other_envs`, `::test_hitch_release_does_not_prune_from_locked_environments` |
| 10 | locked environment | **covered** | `rebuild_tests::test_hitch_rebuild_locked_environment`, `::test_hitch_rebuild_locked_environment_force`, `::test_rebuild_blocked_when_lock_held`, `::test_rebuild_proceeds_with_stale_lock` |
| 11 | approval-required promotion | **covered** | `approval_workflow_tests::test_promote_with_approval_does_not_apply_until_approved`, `::test_approval_request_creation`, `::test_batch_approval_requests_are_atomic_on_failure` |
| 12 | `--no-push` | **partial** | half covered by `rebuild_tests::test_publish_journal_records_push_obligation`; see Task 2 |
| 13 | interrupted publish recovery | **covered** | `rebuild_tests::stage_interrupted_publish` + `::test_interrupted_publish_is_finished_by_the_next_command` + `::test_interrupted_publish_recovery_refuses_to_touch_an_edited_tree`; plus `crash_recovery_tests.rs` across all four abort points (`journal-written`, `ref-moved`, `resync-done`, `push-succeeded`) |

### Why scenario 12 is only partial

`--no-push` is the test harness's **default injection** — `rebuild_tests.rs:1130` says so explicitly ("`run()` already defaults to `--no-push` (and `--yes`)"), and `AGENTS.md` records that the release/resolve crash-fuzz suites depend on that default. So nearly every integration test already exercises the `!context.should_push()` branch of `publish_branch` (`src/utils/prelude.rs:1186-1190`) without saying so.

Split the scenario into its three claims and check each:

| Claim | Covered? | Evidence |
|---|---|---|
| environment branch still moves locally | **yes** | implicitly: ~every rebuild test |
| no publish-journal record survives | **yes** | `rebuild_tests.rs:1139-1147` (`test_publish_journal_records_push_obligation`) asserts `for-each-ref refs/hitch/publish` is empty after a completed publish, and its comment ties that directly to the `--no-push` default. Also `crash_recovery_tests.rs:271-280` |
| `refs/remotes/origin/<env>` left untouched | **NO** | nothing in the suite asserts a remote-tracking ref is *unchanged*; every `refs/remotes/origin` assertion in `tests/` is about `hitch push` refreshing it or rejecting a moved remote |
| `refs/hitch/build/<env>/<ts>` anchor dropped | **NO** | `refs/hitch/build` appears **nowhere** in `tests/` — zero assertions on the transient anchor, despite `AGENTS.md` making its create-then-drop ordering load-bearing |

So the two genuinely-missing assertions are the remote-tracking ref and the build anchor. Both matter: the remote-tracking one guards the deploy-key/`record_pushed_tip` interaction described in `AGENTS.md`, and the anchor one guards against a leak that would accumulate unreachable commits in every repo that rebuilds. The anchor assertion also becomes load-bearing in P1, whose Task 1 moves that create/drop sequence.

`push_tests.rs` covers `hitch push` itself (plain, force, lease-reject-when-remote-moved) and is not the right home for this; the assertion belongs with rebuild.

---

## Global Constraints

- `just format`, `just format-check && just lint`, `just test` clean before this phase is done. All are `-p hitch`-scoped; never run a bare workspace-wide cargo command.
- Follow the existing test naming pattern in `tests/integration/`: `test_<command>_<scenario>`.
- New fixtures go through `HitchTestFramework::new()` + `.with_test_environment(TestSetup::HitchInit, |env| { ... })`, using `env.git` / `env.hitch` / `env.fs`.
- Tests creating linked worktrees must place them **beside** the repo, not inside it, or they appear as untracked content in the repo's own `git status`.
- Every git subprocess spawned by the harness already nulls its stdin (`tests/test_framework/command_runners.rs`). A new raw `Command::new` in test code needs `#[allow(clippy::disallowed_methods)]` with a one-line reason.

---

### Task 1: Confirm the inventory

**Files:**
- Read: `tests/integration/{promote_demote,rebuild,release,conflicts,approval_workflow,lock_unlock,cleanup,resolve,push}_tests.rs`
- Read: `tests/integration/crash_recovery_tests.rs`
- Read: `tests/scenarios/{full_workflow,concurrent_operations,edge_cases,error_handling}_tests.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: confirmation (or correction) of the 13-row table above, appended as a dated note to this plan.

- [x] **Step 1: Verify the table — all 30 cited test names exist**

Done 2026-09-25. Every one of the 30 test names cited in the table above was confirmed present in `tests/` by name. No row cites a test that does not exist.

- [x] **Step 2: Read the four load-bearing test bodies**

Done 2026-09-25. All four confirmed; **no row needed downgrading, and two were stronger than the table claimed.**

- **`test_hitch_rebuild_ejects_conflicting_branch_by_default`** (`rebuild_tests.rs:372`) — asserts exactly what the table claims, and more. It checks exit code 2, the correct conflict *attribution* (`branch-b conflicts with branch-a`, not `main` — i.e. the branch it genuinely collides with), the conflicted filename, **and** that `git show dev:shared.txt` equals `from branch-a`, proving the held branch is absent from the published tree rather than merely reported. It additionally asserts no leaked `hitch-tmp-*` branch, no leaked worktree, and that the user is still on `main`. **Row 3 stands; it understated the test.**

- **`test_hitch_release_is_atomic_when_a_later_branch_conflicts`** (`release_tests.rs:852`) — asserts the failure *and* two independent post-conditions: `main_before == main_after` (the ref is byte-identical) and `git cat-file -e main:a.txt` **fails** (feat-a's content is not there). Two levels, both content-level. **Row 7 stands.**

- **`crash_recovery_tests.rs`** — the oracle comparison is on a tree OID, as documented: the helper is literally `rev-parse refs/heads/<branch>^{tree}` (`crash_recovery_tests.rs:101-107`), and `test_publish_converges_after_abort_at_each_step` (`:121`) builds that oracle from a separate uninterrupted run before looping the three pre-push abort points. Two things exceed what the table claimed:
  - It asserts the interrupted run **actually crashed** (`!interrupted.success()`, `:154-161`) with a message naming `HITCH_TEST_ABORT_AFTER`. That is a guard against the `#[cfg(debug_assertions)]` no-op under `--release` — the exact trap `AGENTS.md` records — so the test cannot silently pass as a no-op.
  - It **proves the abort landed the state its name claims** before recovery erases the evidence (`:182-240`): for `journal-written` it asserts neither ref moved; for `ref-moved`/`resync-done` it asserts the journal record exists, parses it, checks `branch`/`to_sha`/`push_owed`, and uses a real file on disk (`2.txt`) as signal for whether resync ran. A fault-injection test that never verified the fault occurred would prove nothing; this one does.
  - **Row 13 stands.** The fourth point, `push-succeeded`, is a separate test (`:321`).

- **`--no-push`** — this one **did** need correcting, and in the opposite direction from what the table claimed. It was not a gap; it was half-covered. See the scenario-12 table above and Task 2.

- [ ] **Step 3: Read the remaining rows' bodies**

The nine non-load-bearing rows (1, 2, 4, 5, 6, 8, 9, 10, 11) rest on name-matching only. Low risk — a name overstating its body would mean the scenario is *less* covered than recorded, which fails safe for a refactor (P1 would find a missing oracle rather than trust a phantom one) — but not zero. Read them if P1's pure-move claim is ever contested.

- [x] **Step 4: Record the suite's current size and result**

Done 2026-09-25. `just test` on commit `5d81fb2`: **354 passed, 0 failed, 2 ignored**, 88.62s, plus 1 passing test in `tests/no_args_help.rs` and 0 doc-tests. `just lint` clean locally. This is the number P1's exit criteria compare against; it should have risen to 355 after Task 2.

**A note on the baseline:** the local tree is green, but `main` fails CI three ways — a newer-clippy `?`-operator lint that does not reproduce on rustc 1.96.0, one Windows-only temp-path failure (path mangling to `scenario_C:\Users\RUNNER~1\...`), and the `hitch-desktop` compile errors. All three are out of scope here. If `just lint` starts failing mid-phase, check the toolchain before assuming a regression.

---

### Task 2: Close the two `--no-push` gaps

**Files:**
- Modify: `tests/integration/rebuild_tests.rs`
- Read: `src/utils/prelude.rs:1186-1190` (the `!context.should_push()` early return in `publish_branch`)
- Read: `tests/integration/push_tests.rs` (`init_bare_origin` helper, for the remote-assertion idiom)
- Already covered, do **not** re-test: `rebuild_tests.rs:1139-1147` (journal cleared) — see the scenario-12 table above

**Interfaces:**
- Consumes: `HitchTestFramework`'s existing bare-origin setup, used the way `push_tests.rs::init_bare_origin` uses it.
- Produces: `test_rebuild_with_no_push_leaves_remote_and_build_anchor_untouched`.

- [ ] **Step 1: Write the test**

Add to `tests/integration/rebuild_tests.rs`. Model the setup on `push_tests.rs::init_bare_origin` so a real remote exists, and capture `pre_rebuild_remote_sha` before the rebuild. Then assert the two things the suite does not currently cover:

```rust
// 1. The remote-tracking ref did NOT move — nothing was pushed. Nothing in
//    the suite asserts a remote-tracking ref is *unchanged*; every existing
//    refs/remotes/origin assertion is about `hitch push` refreshing it.
assert_eq!(
    env.git.run(&["rev-parse", "refs/remotes/origin/dev"])?.stdout().trim(),
    pre_rebuild_remote_sha,
    "--no-push must leave origin/dev untouched"
);

// 2. The build anchor is cleaned up. `refs/hitch/build` currently has zero
//    assertions anywhere in tests/, despite AGENTS.md making the
//    create-then-drop ordering load-bearing (a leak accumulates an
//    unreachable commit on every rebuild).
let build_refs = env
    .git
    .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/build"])?
    .stdout();
assert!(
    build_refs.trim().is_empty(),
    "rebuild left a build anchor behind:\n{build_refs}"
);
```

Use `for-each-ref` over the namespace rather than a literal `refs/hitch/build/dev`: the ref name carries a timestamp suffix, so a literal path would never match and the assertion would pass vacuously. This is the idiom already used four times in the suite for `refs/hitch/publish`. Confirm the exact ref layout against `src/utils/prelude.rs` when writing.

Capture `pre_rebuild_remote_sha` from the repo *before* invoking the rebuild.

- [ ] **Step 2: Run the test and confirm it exercises the branch it claims to**

`just test-file rebuild`. Both assertions should pass on the first run — unlike the old draft of this task, neither is expected to fail, because the behaviours are already correct and the gap was purely in coverage. That makes it a pure regression test, and worth recording as such here.

Verify it is not vacuous, which is the real risk with a coverage-gap test: assert that `refs/remotes/origin/dev` **exists** before the rebuild (if a bare origin was set up but nothing ever fetched it, `rev-parse` fails and the equality check could pass for the wrong reason), and confirm the anchor assertion is non-vacuous by checking that `refs/hitch/build` *does* appear mid-publish. The latter is hard to observe from outside the process; if verifying it is not worth the machinery, say so in this plan rather than silently shipping an assertion that might never have fired.

- [ ] **Step 3: Run the full suite**

`just test`. All green, and the pass count from Task 1 Step 3 has increased by exactly one.

---

### Task 3: Record the behavioural oracle

**Files:**
- Modify: this plan (append a dated section)

**Interfaces:**
- Consumes: nothing.
- Produces: a written record in this plan of current exit codes and ref effects, which P1–P10 must not change silently.

- [x] **Step 1: Record exit codes**

Recorded 2026-09-25, read from `src/main.rs:114-131`. `Commands::Rebuild` is the **only** command in the CLI with a non-0/1 exit code:

| Situation | Exit code | Mechanism |
|---|---|---|
| clean success | 0 | `rebuild::run` → `Ok(false)` → falls through to `Ok(())` |
| succeeded **with held branches** (or `--dry-run` that would hold) | **2** | `rebuild::run` → `Ok(true)` → `std::process::exit(2)` at `main.rs:128`, after an explicit stdout flush |
| halt-policy refusal, or any other failure | 1 | `Err` from `rebuild::run` → main's normal error path, same as every other command |

The `stdout().flush()` before `process::exit` at `main.rs:123-127` is load-bearing: `process::exit` skips normal shutdown, so buffered output would be lost when stdout is not a TTY (i.e. in CI). Don't "simplify" that away.

The 0/2/1 distinction is a CI contract — it lets a pipeline warn on holds without failing. P4 must preserve it and must not collapse `AppliedWithHolds` into a generic success (source spec §8).

- [x] **Step 2: Record ref effects per operation**

Recorded 2026-09-25. Verified against `src/utils/prelude.rs` (the `RefEdit` batch at `:1136-1153`) and, for `rebuild`, confirmed by hand against a throwaway repo.

- **`hitch rebuild <env>`** — one atomic ref transaction moves `refs/heads/<env>` and writes `refs/hitch/prev/<env>/<ts>` + `refs/hitch/backup/<env>/<ts>` (both `format!("refs/hitch/{}/{}/{}", ...)` at `:1144`/`:1149`; currently byte-identical to each other). `refs/hitch/publish/<env>` is written in the same transaction and cleared once every obligation it describes is settled. `refs/hitch/build/<env>/<ts>` is created at `:796` and dropped at `:814`, *after* publish is attempted but *before* its error is propagated — so it is dropped even on publish failure. `refs/remotes/origin/<env>` is moved only on a successful push, by `record_pushed_tip` (`:865`), which is hitch's own bookkeeping because deploy-key pushes target an explicit SSH URL and bypass git's own remote-tracking update.
- **`hitch release <env> <target>`** — moves `refs/heads/<target>` and anchors under `refs/hitch/release/<...>`; creates the release tag; commits pruning of promoted branches from other environments to `hitch-metadata`; rebuilds dependent environments (in order, unless `--no-rebuild-dependents`). It does **not** write `prev/`/`backup/` — it calls `publish_branch` with `backup_timestamp: None`, because the prior tip is already named by the release tag.
- **`hitch promote` / `demote`** — commit to `hitch-metadata`, then behave as `rebuild` for the affected environment unless `--no-rebuild`.

- [x] **Step 3: Record the `refs/hitch/*` namespace inventory**

Recorded 2026-09-25 by enumerating `refs/hitch/[a-z-]+` across `src/`:

| Namespace | Written by | Meaning | Prunable by `hitch cleanup`? |
|---|---|---|---|
| `build/` | `rebuild` (`:796`) | transient anchor keeping the composed commit reachable until the publish CAS lands | **should never persist** — a leak accumulates an unreachable commit per rebuild |
| `release/` | `release` | the same anchor role for release builds | same |
| `publish/` | `publish_branch` | the publish-journal record of unsettled obligations | no — cleared by the publish that writes it, or by `recover` |
| `prev/` | `publish_branch` (`:1144`) | timestamped archive of the replaced tip | **yes** |
| `backup/` | `publish_branch` (`:1149`) | byte-identical twin of `prev/` | **yes** |
| `resolutions/` | `hitch resolve --record` | phase-5 content-addressed conflict resolutions | no |
| `pending-resync/` | pre-journal hitch | legacy; read-only, so an upgrade mid-publish recovers | no |

`refs/hitch/state/` does not exist yet — P2 adds it.

**The load-bearing detail:** `src/commands/cleanup.rs:174` hardcodes `for namespace in ["backup", "prev"]`. That list is the *archive* set, and `state` is a **live pointer**, not an archive. Adding `state` there would make `hitch cleanup` delete the provenance record out from under a repository — and because the record is written inside the publish transaction, the next rebuild would then see a missing record and report the environment as `LegacyUnknown`, silently discarding the Actual state P2 exists to establish. Global Constraint 5 in the master plan covers this; P2 must add a regression test.

---

### Task 4: Update `AGENTS.md`

**Files:**
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: this phase's findings.
- Produces: a documented record that the scenario inventory exists, and where.

- [x] **Step 1: Add a pointer to this plan**

Done. `AGENTS.md` *What this is* gained a paragraph on the program, and the architecture map gained a `docs/superpowers/plans/` bullet naming P0's inventory and oracle tables as the reference for behaviour-preserving refactors.

- [x] **Step 2: Record the exit-code contract as a gotcha**

Done.

- [x] **Step 3: Correct a stale `AGENTS.md` line found while doing the above**

Done. `AGENTS.md`'s Build section documents

```
just test-file <name>   # cargo test --test <name>
```

That is **wrong for every integration test**. `tests/integration/*_tests.rs` and `tests/scenarios/*_tests.rs` are *modules* of a single `tests/mod.rs` target, not separate test targets — the only integration targets cargo knows are `mod` and `no_args_help`. So `just test-file rebuild` fails outright with `no test target named 'rebuild' in default-run packages`. It is also not `-p hitch`-scoped, unlike every other gate.

The working invocation is:

```bash
cargo test -p hitch --test mod -- integration::rebuild_tests::tests::<test_name> --exact
```

The full path matters — a bare test *name* filter matches nothing, because the module path is part of the test's identity. `AGENTS.md` now records this.

- [x] **Step 4: Record the `just build` / abort-hook interaction**

Done, and it belongs next to the existing "Recovery is tested by interruption" gotcha. `just build` produces a **release** binary (`justfile`: `cargo build --release`), and the `HITCH_TEST_ABORT_AFTER` hook is `#[cfg(debug_assertions)]`-gated — so a binary from `just build` silently ignores the variable and exits *successfully* instead of aborting. Confirmed by hand: `just build`, then `HITCH_TEST_ABORT_AFTER=journal-written hitch rebuild dev` returned 0 and left no anchor; the same command from `cargo build -p hitch` (debug) aborted with exit 134 and left `refs/hitch/build/dev/20260925160909`. Any manual verification of crash recovery needs `cargo build -p hitch`, not `just build`. `AGENTS.md` already warns that the `release` recipe's pre-flight `cargo test` must not use `--release` for this reason; this is the same trap reached from the other direction.

- [x] **Step 5: Verify all four gates**

Done. `just format`, `just format-check`, `just lint` clean; `just test` **355 passed, 0 failed, 2 ignored** in the `mod` target (up from 354 — the one new test), plus 60 in the lib target and 1 in `no_args_help`.

---

## Exit criteria

- [x] All 13 scenarios classified. All 30 cited test names confirmed to exist; the four load-bearing bodies read and confirmed; no row overstated its body.
- [x] The two missing `--no-push` assertions added — `test_rebuild_with_no_push_leaves_hitch_push_bookkeeping_alone` in `rebuild_tests.rs` — and confirmed non-vacuous against a real build.
- [x] Exit codes, per-operation ref effects, and the `refs/hitch/*` namespace inventory recorded in this plan.
- [x] `AGENTS.md` updated: program pointer, exit-code gotcha, corrected `test-file` recipe, and the `just build`/abort-hook interaction.
- [ ] `just format`, `just format-check`, `just lint`, `just test` all pass, with 355 passing (up from the 354 recorded in Task 1).
