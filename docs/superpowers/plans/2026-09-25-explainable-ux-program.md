# Hitch Explainable UX Redesign — Program Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make hitch a declarative state manager — `INTENT → PLAN → APPLY → RESULT` — so that any meaningful mutation can answer three questions without the user reading a README or a commit graph: *why is hitch doing this*, *what exactly will change*, and *what state will exist afterwards*. Achieved by exposing the state hitch already reasons about, never by replacing its Git safety model.

**Architecture:** One shared structured domain layer, consumed by every surface. Read repository → produce an immutable `OperationPlan` → validate its fingerprint → apply → produce an `ExecutionReceipt`. The planner is deliberately *the front half of the existing executor*, not a parallel implementation: hitch's rebuild is already cleanly two halves (a pure composition loop over pinned SHAs, then an atomic publish), and extracting that loop is what makes the plan and the apply provably the same computation rather than two implementations that must agree forever.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, `chrono`, the existing `GitOperations` plumbing layer (`src/utils/git_operations.rs`), `HitchTestFramework` integration-test harness (`tests/test_framework/`).

**Source specification:** `docs/explainable-ux-spec.md` — a verbatim copy of the author's original, sections 1–42. This plan implements it.

---

## Where this work lives

**Branch `explainable-ux`, forked from `main` at `5d81fb2`.** `main` is untouched and stays that way for the whole program; nothing here is intended to land on `main` piecemeal.

Six commits, so far:

- `e01c2ee` — docs only: the spec and the P0–P2 phase plans.
- `1f5b2dd` — P1 + P2's code, landed together. They share `src/utils/prelude.rs` and `src/commands/rebuild.rs`, and splitting them by hunk would leave a commit that does not compile — a worse artifact than a coarser one.
- `310de28` — the P0–P2 phase plans' "As executed" sections.
- `05f7126` — P3: the new `src/core/state.rs`, the timestamp→SHA staleness fix, and the pure `core/status.rs` projection.
- `01d19c8` — P4: `src/operations/`, and `hitch rebuild` rewired to plan → validate → apply → receipt.
- `313189d` — the P4 plan's "As executed" / "What P5 inherits" sections.

None of them touch `crates/hitch-desktop` (scope rule, above), and `git diff --name-only main..explainable-ux -- crates/` is empty as a standing check.

**P0–P5 are complete. P6 is next.** Four of the six mutating commands now plan → validate → apply → receipt: `rebuild`, `promote`, `demote`, and `release`. `rebuild_environment_opts` survives as a thin plan-then-apply wrapper, but no longer as a *top-level* entry point — promote/demote/release reach it only as the nested rebuild for a dependent environment, and `approvals/approve.rs` calls its no-argument wrapper. **`hitch approve` is the one mutating command with no plan**, and `src/commands/resolve.rs` is the one mutating path still choosing a mode from a second merge opinion (`preflight_compatibility_report`); see P5's "What P6 inherits" for why the first is deliberate and the second is not.

---

## Scope

**In scope:** the shared Rust core, the CLI, and the structured typed API that the desktop app will later consume. Phases P0–P10 below. Covers the source spec's §5–§19, §27–§28 (M1–M8, M12, M13), §30–§37, and the non-desktop rows of §40.

**Out of scope, deliberately:**

- **The `hitch-desktop` application** (source §20–§26, §30.5, milestones M9/M10/M11). `crates/hitch-desktop` does not currently compile — 5 errors at the version pinned in this repo (see P0 Task 1 for the full list) — and is being repaired on a separate stream. The shared typed API those milestones specify is still built here, CLI-side, because the CLI is the first consumer. Nothing in P0–P10 may make the desktop crate *more* broken than it already is.
- **The CI/baseline repair.** `main` fails CI three independent ways today (a newer-clippy `?`-operator lint, a Windows-only temp-path test failure, and the desktop compile errors). Tracked separately.

**Deliberate departures from the source spec's §29 order**, each with its reason stated at the phase it applies to:

| Spec says | This plan does | Why |
|---|---|---|
| M1 (state) before M2 (provenance) | **P2 (provenance) before P3 (state)** | The state model's entire reason to exist is Desired-vs-Actual, and Actual is unknowable without a build record. Building state first means building it on the timestamp heuristic §11.2 tells us to delete, then immediately rewriting its core. |
| M3 "critical rule": don't duplicate merge calculations | **P1 extracts the shared primitive before any plan type exists** | The rule is one sentence carrying the whole program's cost. Honoring it while the code is green and before plan types exist to hide behind is far cheaper than retrofitting it. |
| M3 builds 4 planners, M4 builds 4 executors | **P4 does rebuild alone, end to end** | Batching means the first real plan/apply test is smeared across promote's approvals, demote's removals, and release's target-inference + dependent-rebuilds + pruning. This also matches the spec's own §36.2, which its own M3/M4 split violates. |

---

## Global Constraints

1. **Scope every cargo invocation to `-p hitch`.** `just format`, `just format-check`, `just lint`, and `just test` are all `-p hitch`-scoped and are the only gates. A bare `cargo build` / `check` / `clippy` / `test` compiles the whole workspace, including `hitch-desktop`, which does not build — so those commands fail on a crate this program does not own. This is the single most important operational constraint in this document.

2. **Do not reshape `WorkspaceIndexModel` or `BranchRow`** (`src/core/workspace_index.rs`, `src/core/workspace.rs`). They feed `WorkspaceIndexDto` in the frozen desktop crate, whose TypeScript we cannot update in this program. New state models go *alongside*; adapting these is a follow-up for whoever does the desktop migration. This is a known, deliberate, temporary second model — the exact thing source §36.4 warns against — and the condition that keeps it honest is that no *second new* model gets added. One new model, one frozen legacy consumer.

3. **Every git/gh spawn goes through `GitOperations::git_command`** (or an already-blessed builder). `clippy.toml` denies `std::process::Command::new` crate-wide, so any new spawn point needs `#[allow(clippy::disallowed_methods)]` plus a one-line reason, and must null stdin. The annotation is the review signal.

4. **Do not touch `RefEdit::Update`'s `expected_old: Some(String::new())` vs `None` semantics.** That CAS distinction is load-bearing; see `AGENTS.md`'s Conventions section and the regression test `publish_survives_leftover_publish_journal_ref` in `src/utils/prelude.rs`. It was once wrongly set to `None` and turned a benign leftover journal record into a permanent, self-perpetuating publish wedge.

5. **`refs/hitch/state/*` is a live pointer, not an archive.** `src/commands/cleanup.rs:174` hardcodes `for namespace in ["backup", "prev"]` as the prunable set. It must never gain `state`. P2 adds a regression test for this.

6. **The existing differential and crash-fuzz tests must pass unchanged.** `test_merge_tree_compose_matches_real_merge_across_scenarios` (`tests/unit/git_operations_tests.rs`) and the four `HITCH_TEST_ABORT_AFTER` crash-fuzz suites are the regression oracle proving composition and publication behaviour is untouched. If one of them needs editing to make a refactor pass, the refactor is wrong.

7. **Preserve, never reimplement:** pinned-input semantics, the exact merge engine (`merge_tree_compose` / ORT), compare-and-swap ref publication, `publish_journal` ordering, checkout scan/resync safety, remote lease behaviour, held-branch semantics, release's all-or-nothing semantics, and recorded-resolution lineage checks. Refactor *around* these primitives.

8. **Never sort a promoted branch list for presentation.** Composition order is semantic. The status matrix may sort *feature rows* alphabetically; an environment's equation and its plan must retain configured promotion order (source §36.7).

9. **Never execute a plan from an untrusted caller.** Plans are core-generated and identified by fingerprint. The fingerprint gates staleness; it does not authorize. A plan arriving as caller-authored ref edits must never be applied (source §35).

10. **User-facing errors keep a concrete next step** — `hitch rebuild <env>`, `hitch resolve <env> --branch <b>`, `hitch push <branch> -f`. A bare error string is a worse experience than the rest of the CLI.

11. **Keep `AGENTS.md` current in the same change** that alters anything it documents — architecture, command registration, gotchas. This repo's own rule, and the reason its gotchas section is worth reading.

12. **Manual end-to-end check for user-visible changes.** Build the binary and exercise it against a throwaway git repo in `/tmp`. The integration framework is good but does not replace it; `AGENTS.md` records a real correctness bug found exactly this way, by hand, that no test covered.

---

## Phase dependency graph

Linear by design. Each phase is only meaningful on a green tree, and each one's file:line references are resolved against the tree as it exists when that phase is authored.

```text
P0  scenario inventory
 │
 └─> P1  shared composition primitive ──┐
                                        │
     P2  build provenance ──────────────┤
                                        │
     P3  state model ───────────────────┤
                                        │
     P4  rebuild plan/apply ────────────┤
                                        │
     P5  promote/demote/release ────────┤
                                        │
     P6  CLI renderers, --json ─────────┤
                                        │
     P7  status matrix + why ───────────┤
                                        │
     P8  metadata-only mutations ───────┤
                                        │
     P9  structured activity ───────────┤
                                        │
     P10 docs + legacy removal ─────────┘
```

**Authoring note:** P0–P5 are authored and **all six are executed**, as `2026-09-25-explainable-ux-P0-scenario-inventory.md`, `2026-09-25-explainable-ux-P1-shared-composition.md`, `2026-09-25-explainable-ux-P2-build-provenance.md`, `2026-09-25-explainable-ux-P3-state-model.md`, `2026-09-25-explainable-ux-P4-plan-apply.md`, and `2026-09-25-explainable-ux-P5-plan-apply.md`. Each carries an "As executed" / "Implementation status" section recording what actually landed, the deviations from its plan, and anything the next phase inherits. P6's task steps are authored next, because P5 changed what a plan and a receipt can say — four commands now produce both, and the `ExecutionWarning { owes_effect }` / `OperationOutcome` distinctions P6 has to render did not exist before it. P6–P10 follow the same naming convention and are authored as their phase approaches. The Goal/Architecture/Interfaces/Constraints for all ten phases are recorded below and contain no line references, so they do not go stale.

**What P1 handed forward, beyond `compose_environment` and `PinnedInputs`:**

- `commands/resolve.rs:131,180` still selects its Mode A vs Mode B and its "nothing to resolve" refusal from `preflight_compatibility_report` — a tree-based approximation. P1's central invariant ("one composition") therefore holds for `rebuild` and is still **violated** for `resolve`. P4's planner is the natural place to fix it, since `resolve` needs a *plan* to choose a mode from rather than a second ad-hoc opinion. Until then, do not add further dependants.
- `prelude::format_compatibility_report_for_rebuild` is the single halt renderer, and `utils::conflict_report::format_conflict_report` now has no production caller (kept, public, tested, a deletion candidate). If a later phase needs a richer per-merge diagnostic, that is the function to reach for — but never to decide a mutation's outcome.
- `rebuild_environment_opts` takes `on_conflict_override: Option<OnConflict>`; `rebuild_environment` passes `None`. Any new caller that needs to influence composition must go through a parameter like it, not through a pre-check.

**What P2 and P3 handed forward, for P4 specifically:**

- **`src/core/state.rs` is the only place a staleness verdict may be computed.** `build_state_snapshot` reads Desired live from refs and Actual from the build record and returns a `RepositoryStateSnapshot`; `EnvironmentHealth` is the verdict, and `ChangedInput { branch, previous_sha, current_sha }` is the "`old → new`" material spec §11.2 asks for. P4's planner renders a *future* state, so it should build a plan and then project it — not reach for `get_commit_timestamp`, and not call `read_state` itself. The old timestamp heuristic is gone and reintroducing it is a regression with tests guarding it (see `AGENTS.md`).
- **`core/status.rs` is now a pure projection** — `build_status_model(&snapshot)`, no `GlobalContext`, no repo handle. That is deliberate: it makes "the view cannot disagree with the model" true by construction rather than by discipline, and P4's dry-run/preview output should take the same shape. `commands/status.rs` now formats and decides nothing.
- **`preflight_compatibility_report` is still a prediction, and P3 made that explicit rather than fixing it.** The ⛔ glyph used to print "held on rebuild" for a *next-build* prediction, which reads as a fact about the branch in front of you. It now consults the build record's `held` list (a fact) first and words the two cases differently — "held in the last build" vs "would be held on the next rebuild". This is a naming fix, not a resolution of P1's open item: `commands/resolve.rs:131,180` still chooses its resolution mode from a second merge opinion, and `preflight_compatibility_report_local` is still called from `conflicts.rs:44`, `status.rs`, and `tree.rs:138`. **P4 owns that.** `resolve` needs a *plan* to choose a mode from, which is exactly what P4 builds.
- **`hitch status` is now fully offline.** P3 removed the per-branch `branch_exists_anywhere` (a `git ls-remote --heads origin` per promoted branch) from the render loop, so the snapshot's `rev_parse_opt refs/heads/*` → `refs/remotes/origin/*` resolution is the only ref lookup. Any new status-shaped path inherits that for free; do not add a network call to one.

**What P4 handed forward, for P5 specifically:**

- **`src/operations/` is the pattern to copy, and it has exactly one operation in it.** `model.rs` holds the operation-agnostic types (`OperationPlan<I>`, `PlanFingerprint`, `PlannedEffect`/`AppliedEffect`, `ExecutionReceipt`, `OperationOutcome`, `PlanApplyError`); `rebuild.rs` holds `plan_rebuild` / `validate_plan` / `apply_rebuild_plan` / `assemble_receipt` / `discard_plan`. A second planner reuses the first four; it does not re-derive the fingerprint or the receipt.
- **The plan carries an already-composed commit, not a recipe.** `commit_tree` stamps wall-clock time, so re-composing at apply time yields a *different* SHA than the plan promised. This is why `PlanPurpose::Preview` does not synchronise (P1's deliberate asymmetry) and why a preview/confirm pair is compared on `^{tree}`, never on the commit. Anything that re-plans mid-apply has to answer for the SHA it is about to land.
- **Three model members are declared but unproduced, deliberately:** `PlanApplyError::PolicyBlocked`, `PlanWarning.blocking == true`, and `OperationOutcome::ApprovalRequested`. P5's approvals are the intended first producers. `OperationKind` likewise has one variant (`Rebuild`) — add `Promote`/`Demote`/`Release` *before* writing their planners, so a receipt can never be ambiguous about what it was.
- **`publish_branch` now returns `PublishOutcome { push, journal_cleared }`** rather than `Result<()>`. A receipt cannot be built from `Ok(())` — that is the whole reason it changed, and the reason a failed push is representable at all. P5 consumed it in `apply_release_plan` (a denied push there is an owed effect naming `hitch push <target>`, and *not* `-f`, because release's push is a plain fast-forward). `commands/resolve.rs` is the one call site that still drops the outcome; P6/P7 will want it.
- **The anchor under `refs/hitch/build/*` is a live leak risk, and it is now a `finally`.** Nothing prunes that family (`cleanup`'s prunable set is `["backup", "prev"]`), so `apply_rebuild_plan` calls `discard_plan` unconditionally after the inner call rather than per exit path — see the P4 plan's deviation 5. Every new planner owes the same `finally`.
- **P1's open `resolve` item is still open.** P4 built the thing `resolve` needs a plan *from*, but did not re-plumb it: `commands/resolve.rs:131,180` still selects Mode A vs Mode B from `preflight_compatibility_report`. Do not add a further dependant on that function while it is still a second merge opinion.

**What P5 handed forward, for P6 specifically:**

- **Four commands produce a plan and a receipt; none of them is rendered.** `rebuild`, `promote`, `demote`, and `release` all return an `ExecutionReceipt` that P6 formats. The distinctions P6 has to render did not exist before P5: `ExecutionWarning { owes_effect }` is separate from a plain warning (a denied push, a failed dependent rebuild, an unbuilt environment are all *owed work*, not failures), and `OperationOutcome` separates `Applied` from `AppliedWithHolds` from `ApprovalRequested`. Read the receipt; do not re-derive any of it, for the reason `core/state.rs` gives.
- **P5 added output but owns no wording.** Four new release progress lines are unstyled ("Tagging 'main'", "Publishing 'main'", "Updating release metadata", "Rebuilding 'dev' — its declaration was pruned"), as are the approval warnings' remedies and the three skip advisories ("Skipping rebuild", "will not be rebuilt", "will be left stale"). All P6 material.
- **`hitch approve` produces no plan, and that is deliberate.** `approvals/approve.rs` still edits the declaration and rebuilds directly, because the bug P5 found there was a *timing* bug, not an architectural one, and routing it through `plan_promote` would re-run `check_pre_promote_conflicts` on a request already vetted when it was created while splitting the declaration edit from `mark_request_applied` into two transactions. If P6 wants a receipt for it, the honest route is a `plan_declaration_change` for the approved request's change applied *after* the approval commits.
- **`src/commands/resolve.rs` is the last un-planned mutation, and the last `preflight_compatibility_report` decision point.** P1 removed `rebuild`'s second merge path and P5 removed release's; `resolve.rs:131,180` still chooses Mode A vs Mode B and refuses when there is no conflict at all. A preflight/composition disagreement there picks the wrong resolution mode, not merely a stale preview. P6 or P7 gives it a planner, or the "one planner per operation" claim stays an overstatement.

---

### P0 — Scenario inventory

**Goal:** Establish what the existing 354-test suite already covers, so P1's "pure move" claim is measured against a recorded oracle rather than assumed.

**Scope note:** This is the scenario-inventory slice of the source spec's Milestone 0 only. The CI repair is out of scope (see Scope, above).

**Interfaces:**
- Consumes: nothing.
- Produces: a written inventory in the phase plan mapping each of the source spec §28 M0's 13 scenarios to the test(s) that already cover it — clean promote+rebuild, demote, rebuild with held branch, rebuild with `halt`, recorded-resolution replay, clean release, failed release conflict, dependent environment rebuild after release, prune after release, locked environment, approval-required promotion, `--no-push`, interrupted publish recovery — plus recorded current exit codes and ref effects.

**Status: COMPLETE** (2026-09-25). All 30 cited test names confirmed to exist; the four load-bearing bodies were read and none overstated its row — two were stronger than recorded. Result: **12 of 13 covered, 1 partial.** `--no-push` was half-covered: the journal-clearing half is already asserted by `rebuild_tests::test_publish_journal_records_push_obligation` (the harness injects `--no-push` by default, which that test's own comment notes), and the remote branch itself is covered indirectly by `push_tests::test_hitch_push_force_succeeds_against_existing_remote_branch`. What was genuinely missing was hitch's *own* `record_pushed_tip` bookkeeping (both call sites gated on push success, so a `--no-push` rebuild must not advance `refs/remotes/origin/<env>` to an unpushed commit) and the `refs/hitch/build` anchor drop (**zero** assertions in `tests/`). Both are now asserted by `test_rebuild_with_no_push_leaves_hitch_push_bookkeeping_alone`, with the anchor assertion verified non-vacuous against a real build. Exit codes, per-operation ref effects, and the `refs/hitch/*` namespace inventory are recorded in the phase plan. `just test` 355 passing.

Two corrections to `AGENTS.md` came out of this: `just test-file <name>` does not work for integration tests (they are modules of the single `tests/mod.rs` target, not separate targets — use `cargo test -p hitch --test mod -- <full::module::path> --exact`), and `just build` is a *release* build, so it compiles out the `HITCH_TEST_ABORT_AFTER` abort hook.

**Exit criteria:** met. Every one of the 13 scenarios classified covered / partial / gap; assertions exist for every gap; exit codes and ref effects recorded; all four gates green.

---

### P1 — Shared composition primitive

**Goal:** Extract the pure composition half of `rebuild_environment_opts` so that planning and executing are the same code, and so `hitch rebuild --dry-run` stops being a different computation from a real rebuild.

**Architecture:** `src/utils/prelude.rs`'s `rebuild_environment_opts` is already two halves: steps 1–5 (sync → pin SHAs → snapshot remote → the composition loop) are pure, returning `(result_sha, held, replayed)` and touching nothing mutable; steps 6–9 (anchor → publish → drop anchor → stamp) are the only ones that take locks, move refs, or talk to a remote. Extract the first half as `compose_environment`. `rebuild_environment_opts` becomes pin → compose → anchor → publish.

**This phase also fixes a live divergence.** Today `rebuild --dry-run` (`src/commands/rebuild.rs:78`) calls `preflight_compatibility_report` (`src/utils/prelude.rs:2080`) — a **tree-based** loop over `merge_tree_write_tree_name_only` with an explicit `--merge-base` — while the real build is a **commit-based** loop over `merge_tree_compose` (ORT, no explicit merge-base). Two concrete wrong answers today: `hitch rebuild dev --dry-run --replay-resolutions` reports holds that replay would resolve, because the dry-run branch short-circuits into a preflight that knows nothing about resolutions; and the two paths enter the merge engine through different doors, which is precisely the wrong-merge-base bug class `AGENTS.md` documents at length. Routing dry-run through the extracted primitive fixes both for free.

**Interfaces:**
- Produces: `PinnedInputs { base_name: String, base_sha: String, branches: Vec<(String, String)> }` — `branches` in configured promotion order, never sorted. Deliberately excludes `remote_env_sha_before`: that is not a composition input but a publish-time lease observation, so the caller captures it alongside the pinned inputs and P4's fingerprint covers it as a remote ref.
- Produces: `CompositionResult { result_sha: String, included: Vec<String>, held: Vec<CompatibilityConflict>, replayed: Vec<String> }`.
- Produces: `compose_environment(ctx: &GlobalContext, inputs: &PinnedInputs, env_name: &str, on_conflict: OnConflict, replay: bool, require_signed_resolutions: bool, on_step: &mut dyn FnMut(&str)) -> Result<CompositionResult>` — a pure function: no ref moves, no locks, no checkouts, no writes.
- Consumes: nothing.

**Status: COMPLETE** (2026-09-25). 32 steps, 10 of 11 exit criteria checked — the 11th deliberately left unchecked and carried forward. Full plan, deviations, findings, and the manual-check results table: `docs/superpowers/plans/2026-09-25-explainable-ux-P1-shared-composition.md`. P1 found three things worth carrying forward: `hitch rebuild --on-conflict halt` never reached the composition (it worked only through the duplicate pre-check P1 deleted), a halt printed two different reports depending on `--replay-resolutions`, and `hitch resolve` still gates on `preflight_compatibility_report` — so P1's "one composition, two callers" invariant holds for `rebuild` but is still violated for `resolve`. That last one is P4's to fix, not P2's.

**Exit criteria:** `compose_environment` is the only conflict/composition calculation in the codebase. The 354 existing tests pass unchanged. Two new properties are covered: purity (identical pinned inputs → identical *tree* OID) and dry-run/real agreement (identical pinned inputs → identical hold set). Manual end-to-end check done for a held branch and a replayed branch. `AGENTS.md` records the invariant.

---

### P2 — Build provenance

**Goal:** Make "Actual" state reliable, so Desired/Actual/Proposed are three real concepts rather than one plus a guess.

**Architecture:** A versioned `EnvironmentBuildRecord` stored at `refs/hitch/state/<environment>`, written **inside the same atomic ref transaction** as the environment-branch move, via one additive `extras: &[RefEdit]` parameter on the existing `publish_branch`. This is derived state and does not live in `hitch.json` (source §6, §34). An environment whose record predates this version reports `LegacyUnknown`; hitch never fabricates Actual membership.

**Interfaces:**
- Consumes: `CompositionResult` and `PinnedInputs` (P1) — the record's fields are exactly the composition result's fields, which is why P1 precedes P2.
- Consumes: `publish_branch` (`src/utils/prelude.rs`), extended with `extras: &[RefEdit]`.
- Produces: `EnvironmentBuildRecord { schema_version, environment, metadata_sha, base_name, base_sha, desired_branches, included_branches, held, replayed_resolutions, result_sha, built_at, hitch_version }`, serialized to a blob at `refs/hitch/state/<env>`.
- Produces: a reader that maps a missing or mismatched record to an explicit degraded/unknown state rather than a guess.

**Exit criteria:** The record matches the published branch tip, or is absent — never inconsistent, because it rides the same transaction. Held and replayed branches are recorded. A branch move makes the state "needs rebuild". A corrupted record is explicit degraded state. `cleanup` cannot prune `state`. All four gates green.

**Status: COMPLETE** (2026-09-25). 7 tasks, all steps, 9 unit tests in `build_record.rs` and 9 new integration tests. Full plan, deviations, findings, and the manual-check results table: `docs/superpowers/plans/2026-09-25-explainable-ux-P2-build-provenance.md`.

Six deviations from the plan as authored, all deliberate and all recorded in P2's own Implementation status:

- **`RefEdit` instead of source §6's `PublishExtraRef`.** Field-for-field duplicate of the existing type.
- **`held: Vec<CompatibilityConflict>` instead of §6's `HoldRecord`.** Same three fields, already what `CompositionResult.held` holds; a third conflict shape would be worse. Only additive serde derives on `CompatibilityConflict`.
- **`ResolutionUse` pulled *forward* into P2.** `CompositionResult.replayed` went from `Vec<String>` to `Vec<ResolutionUse>` here, a phase earlier than originally scoped, because a branch name cannot say *which* recorded resolution ran — the key is content-addressed, so the same branch replayed later resolves under a different one.
- **The state ref uses `expected_old: Some(String::new())` (unconditional), not a CAS.** This is a *new* edit and deliberately not the publish-journal CAS mistake Global Constraint 4 warns about. See the AGENTS.md gotcha.
- **`metadata_sha` is weaker than originally documented** — see below; this is the one finding that changes what P3 may rely on.
- **`read_state` probes `schema_version` before the full parse**, after a test proved the gate otherwise unreachable for exactly the case it exists for (a newer hitch's record, which has every known field *plus* unknown ones).

Four things P3 must know, recorded here because they narrow what "Actual unknown" means:

- **`LegacyUnknown` is a normal state in this release, not a defect.** Only `hitch rebuild` writes a record. `hitch release` and both of `hitch resolve`'s publish paths pass `extras: &[]` — deliberately, because neither has the data an `EnvironmentBuildRecord` truthfully describes (a release is a tag-and-branch landing, not a composition; a Mode B resolve composes from a hand-resolved worktree whose included-branch list hitch does not track). Writing a record for either would be a fabricated Actual, which is the exact failure this phase exists to eliminate. **P3 must render `LegacyUnknown` as a first-class honest state, not treat it as missing work and not paper over it.** `test_resolve_mode_b_publishes_without_a_record_and_leaves_no_usable_actual` pins the gap and the fix direction.
- **`metadata_sha` is *not* a staleness signal at all, and P2's own plan overclaimed for it.** It was documented as "a recorded fact and a fast path" that "answers 'did the declaration change at all'". The manual check disproved the fast-path half: a rebuild brackets the commit it records on **both** sides with its own metadata writes — `with_locked_env` commits the lock before the declaration is read, and publishing commits the `rebuilt_at` stamp and the unlock after — so the recorded SHA is a commit that only ever existed as a transient tip, is **not observable from outside the command at all**, and is always a strict *ancestor* of `hitch-metadata`'s final tip. Comparing it to anything is meaningless. P3's staleness check must compare the per-branch pinned SHAs in `desired_branches` against freshly pinned ones, and must not read `metadata_sha` for correctness at all.
- **A build record is a snapshot of a build, not a live view of intent.** After `hitch promote`, the record still describes the previous build — deliberately, and `test_a_promotion_after_a_rebuild_leaves_the_record_describing_the_old_build` pins it. P3 derives "needs rebuild" from comparing it; it must not expect hitch to have updated it.
- **No new CLI surface.** `read_state` has no production caller yet — the writer side is live, the reader side is wired in by P3. Nothing in P2 changed a command's output; the only user-visible effect is one new ref.

One scope fact, because P3's snapshot builder has to know which publishes leave a record: the record is written inside `rebuild_environment_opts`, not inside `src/commands/rebuild.rs`. So every caller that goes through it writes one — `hitch promote`, `hitch demote`, `hitch approve`, and the post-release environment rebuild at `src/commands/release.rs:709`/`:712`. What does *not* write one is any publish that goes straight to `publish_branch`/`publish_environment_build` with `extras: &[]`: `hitch release`'s own branch/tag landing, `hitch resolve`'s Mode A rebase landing, and `hitch resolve`'s Mode B worktree publish. The distinction is not cosmetic — it is "did a composition run, and do we know what it composed". `test_promote_refreshes_the_build_record` is what stops that construction from drifting into `commands/rebuild.rs`, where it would look correct in every rebuild test and be silently wrong in production.


---

### P3 — State model

**Goal:** One shared read-only `RepositoryStateSnapshot` that `status`, the desktop workspace index, and every future planner consume, with staleness computed by comparing pinned SHAs instead of timestamps.

**Architecture:** Read-only; changes no mutation behaviour. `src/core/status.rs`'s `determine_rebuild_state` (line 103) currently compares **commit timestamps** against a **wall-clock** `rebuilt_at`, so a rebased or cherry-picked branch, or any skewed-clock commit, reads as up-to-date when it is not. This phase replaces that with SHA comparison against P2's record. Timestamps are kept for presentation, never for correctness (source §11.2).

**This is a user-visible behaviour change and is shipped as a plain fix, not behind a flag.** It will newly report "needs rebuild" for branches that previously read as up-to-date. That is the bug being fixed. Tests cover the rebase and skewed-clock cases; release notes mention it.

**Interfaces:**
- Consumes: `EnvironmentBuildRecord` (P2), treating absence as `LegacyUnknown`.
- Produces: `RepositoryStateSnapshot { metadata_sha, current_branch, environments, features, captured_at }`.
- Produces: `EnvironmentState { name, base, desired, actual, health, locked, approval_policy }`, `DesiredComposition`, `ActualComposition`, `ActualMembership { Included, Held, AlreadyInBase, Missing, Unknown }`, `EnvironmentHealth { Realised, PartiallyRealised, NeedsRebuild, NeverBuilt, LegacyUnknown, MissingBranch }`.
- Produces: feature×environment membership as a first-class query, generalising what `src/core/workspace_index.rs` already computes locally (but without reshaping its public types — see Global Constraint 2).

**Exit criteria:** Same branch promoted to multiple environments; remote-only feature; missing branch; feature already integrated into base; base changed after rebuild; feature changed after rebuild; locked/approval metadata all represented accurately. `status` and the snapshot agree. All four gates green.

**Status: COMPLETE** (2026-09-25). `src/core/state.rs` is the new module; `core/status.rs` is now a pure projection of the snapshot; `commands/status.rs` formats and decides nothing. All seven exit-criterion scenarios have named tests in `tests/integration/state_model_tests.rs`, and the two long-`#[ignore]`d timing tests in `status_tests.rs` are live again with their `sleep(2)` calls and a `--date 2099-01-01` hack deleted — that is the strongest single piece of evidence the fix worked, since those tests previously could not pass without help from a clock. `just test` 68 + 387, zero ignored. Two findings worth carrying: **there were more than the four copies of the timestamp comparison the plan predicted** (the summary blocks re-derived the verdict per environment, so one `hitch status` computed it up to three times), and **no existing test asserted the wrong behaviour** — the full suite passed with zero test changes after the rewrite, which is why Task 6 exists at all. Details, deviations, and P4's inheritance list in the phase plan.

---

### P4 — rebuild plan/apply, end to end

**Goal:** Prove the plan→apply→receipt architecture on exactly one operation before four operations depend on it.

**Architecture:** The planner is P1's `compose_environment` plus a summary of what publish will do. The executor is P2's atomic publish. `rebuild` becomes plan → (confirm | `--dry-run` | `--yes`) → validate fingerprint → apply → receipt.

**Interfaces:**
- Consumes: `compose_environment` (P1), `EnvironmentBuildRecord` (P2).
- Produces: `OperationPlan { id, kind, intent, fingerprint, current, proposed, compositions, effects, unaffected, warnings, confirmation }`.
- Produces: `PlanFingerprint { metadata_sha, refs, remote_refs, resolution_keys }` with a stable `digest()`.
- Produces: `ExecutionReceipt { plan_id, operation, started_at, completed_at, outcome, effects, warnings, resulting_state }` and `OperationOutcome { Applied, AppliedWithHolds, ApprovalRequested, NoChange }`.
- Produces: `PlanApplyError { StalePlan { changed }, PolicyBlocked, Conflict, PublishRace, RemotePushFailed }`.

**Status: COMPLETE** (2026-09-25). `src/operations/{mod,model,rebuild}.rs` is new; `hitch rebuild` is plan → validate → apply → receipt, and `--dry-run` is a renderer over the same planner rather than a second merge path. `publish_branch` gained a `PublishOutcome` so a failed push is representable; `rebuild_environment_opts` survives as a thin plan-then-apply wrapper for its other four callers. `just test` 405 + 1, zero ignored; all four gates green. Three findings worth carrying: **a plan can never report a halt** (`OnConflict::Halt` errors out of `compose_environment` before a plan exists, so `PlanWarning.blocking` and `PlanApplyError::PolicyBlocked` are unreachable from `rebuild` — **superseded by P5**, which made both reachable from the declaration planner: `PlanWarningKind::PolicyRefusal` / `ApprovalRequired` are blocking, and `apply_declaration_plan` raises `PolicyBlocked` from `blocked_by()`), **the anchor release has to be a `finally` rather than a numbered step** (the `?` on `validate_plan` fires first and is invisible on a green run), and **two compositions of identical inputs get different commit SHAs** — which is the reason the plan carries its commit at all. Non-vacuity was probed by short-circuiting `validate_plan` and confirming exactly the three staleness tests fail. Full deviations, the manual-check transcript, and P5's inheritance list in the phase plan.

**Exit criteria:** met. Plan then mutate metadata → apply refused, naming the ref. Plan then move a feature ref → apply refused, naming the ref and both SHAs. Unchanged plan applies exactly the predicted effects, and plan/receipt/repository agree on every one. Receipt reports the real published SHAs. A push failure is a warning with an owed effect and a surviving journal record, never a false "fully synced", and the command still exits 0. `AppliedWithHolds` preserved distinctly, and the rebuild exit-code-2 distinction survives (re-checked from the library side as well as through the CLI). Repo lock, `RebuildLock`, and the `locked` flag all still apply. `--dry-run` agrees with a real run. `git diff --name-only main..explainable-ux -- crates/` empty.

---

### P5 — promote / demote / release

**Goal:** Extend the proven architecture to the three operations that carry extra structure.

**Architecture:** Same plan/apply/receipt shape, one planner each. Promote adds approvals and the environment-name-expands-to-branches form. Demote adds removals. Release is the most explicit plan in the product and adds dependent-environment rebuilds, promotion pruning, the release tag, and remote writes.

**Interfaces:**
- Consumes: P4's plan/apply/receipt types, and — the thing to be careful about — the `rebuild_environment_opts` wrapper P4 left in place, which is what all four of these commands currently call.
- Produces: per-operation planners. Extends `PlannedEffect` with `DependentEnvironmentRebuild` and `PromotionPrune`.
- `OperationKind` gains `Promote`/`Demote`/`Release` **before** their planners, and this is also where `PolicyBlocked`, `PlanWarning.blocking`, and `OperationOutcome::ApprovalRequested` get their first real producers.
- Release must keep all-or-nothing semantics (source §3, §16): a release that cannot compose changes nothing and says so.

**Exit criteria:** Each of the three has a planner that is side-effect-free apart from the fetch/pin it shares with execution, and a receipt that matches its plan. Same-inputs→same-plan and changed-input→different-fingerprint hold for each. All four gates green.

**Status: COMPLETE** (2026-09-26). `src/operations/declaration.rs` (one planner shared by promote and demote — they are one declaration edit with opposite directions) and `src/operations/release.rs` are new, and all three commands are rewired. `just test` 418 + 94 lib, zero ignored; all four gates green. Four findings worth carrying, because each one is a shape rather than a bug:

- **A planner whose fingerprint includes `metadata_sha` must be built after every metadata write that precedes it — including writes hitch makes on its own behalf.** `with_locked_env` commits the lock to `hitch-metadata` before its closure, so a plan built outside the lock is stale on arrival; measured at 13 of 17 promote tests failing with `hitch-metadata: d3da09e → 82658aa`. Consequence: the planner must *not* check `is_locked()` (by then the lock is the command's own), so the human-lock refusal lives in the command.
- **A failed dependent rebuild is an owed effect, not an error, in all three of promote, demote and release.** `rollback_metadata_changes` restores a whole-config snapshot, which is the wrong instrument for a nested rebuild that can fail *after* moving the environment branch. Contract: exit 0, the durable effect persists, the environment is left unbuilt, the message names `hitch rebuild <env>`. Rollback stays reachable for a failed metadata write, which is the only thing it can actually repair.
- **Release's prune predicate is evaluated against `result_sha`, not the live target ref** — for every environment based on the released target the answer changes *because of this release*, so evaluating it before the publish would prune nothing, silently. The same reason `get_branch_commit_sha` is wrong for the CAS's `expected_old` and `rev_parse_opt` is right.
- **The approve path's rebuild ran inside `modify_metadata`'s closure**, which commits *after* the closure returns, while the rebuild reads the declaration back off the ref. It therefore composed from the pre-approval declaration: an approved branch landed in `hitch.json` and never in the environment branch. Fixed by moving the rebuild after the transaction. Same class as the P1 merge-base bug — a shape that reads correctly and is not.


---

### P6 — CLI renderers

**Goal:** Show the plan before the mutation and the receipt after it, in the words the spec's §17 terminology table defines.

**Interfaces:**
- Consumes: `OperationPlan` / `ExecutionReceipt` (P4/P5).
- Produces: a shared CLI plan renderer and receipt renderer; a global `--json` emitting versioned (`schema_version: 1`) `{ plan, receipt }` with diagnostics on stderr and no ANSI or prose mixed into stdout; `--dry-run` on promote/demote/release routed through the same planner; `--yes` as confirmation-bypass only, never as plan-suppression.

**Exit criteria:** A user who has not read the README can read a plan and describe what will happen. `--dry-run` and a real run agree. `--json` is stable and non-interactive JSON without `--yes` fails clearly. All four gates green.

---

### P7 — status matrix and `hitch why`

**Goal:** Make "which feature is where, and is that state actually realised" answerable at a glance, and make every state in that matrix explainable.

**Interfaces:**
- Consumes: `RepositoryStateSnapshot` (P3). No bespoke conflict logic in either command (source §14.4).
- Produces: a feature×environment matrix as `hitch status`'s default view, with per-environment detail retained; the shared environment-equation renderer used by `status`, `tree`, plans, detail, release preview, and conflict explanations; `hitch why <branch>`, `hitch why <branch> <env>`, `hitch why <env>`.

**Exit criteria:** Golden-output tests for a clean repo, held branch, stale feature, changed base, missing feature, legacy no-build-record repo, released feature, and multiple environments — colour disabled, narrow terminal, long branch names, zero environments, many environments, many features. Every major matrix cell state has a useful `why`. All four gates green.

---

### P8 — Metadata-only mutations

**Goal:** Apply the same architecture to the remaining significant mutations, without over-engineering the simple ones.

**Architecture:** add/remove environment, lock/unlock, `set`, `cleanup`, and approval apply/execute mutate metadata but compose nothing — so they share a plan shape with no `CompositionPlan`, no holds, and no fingerprint over composition inputs. `push` gets a receipt if its effects benefit from one.

**Interfaces:**
- Consumes: P4's plan/receipt types, minus composition.
- Produces: metadata-only planners for the above; the `OperationKind` variants the source spec §7.1 lists.

**Exit criteria:** All significant mutations return structured receipts. Read-only commands are left alone. All four gates green.

---

### P9 — Structured activity and terminology

**Goal:** Make hitch tell the deployment story, and keep normal output explaining behaviour while `--verbose` explains mechanism.

**Architecture:** `src/core/timeline.rs` stores a preformatted `summary: String` (line 15). Convert the domain model to typed `HitchEvent`s and render prose from those, preserving the current derivation from `hitch-metadata` history. Per source §19.2, persistent operation receipts in hitch-owned Git metadata are explicitly deferred until their remote concurrency behaviour is proven — this phase does not introduce them.

**Interfaces:**
- Consumes: P2's build record, for current rebuild/hold detail.
- Produces: typed `HitchEvent` variants for environment creation, promote, demote, rebuild, rebuild-with-holds, release, lock, unlock, and the four approval transitions; semantic renderers; a terminology audit moving Git mechanics to `--verbose`.

**Exit criteria:** CLI can consume the activity event model directly. Normal output contains no unnecessary internal Git terms. All four gates green.

---

### P10 — Documentation, compatibility, legacy removal

**Goal:** Finish the transition so there is one obvious architectural path for future operations.

**Tasks:** Update `README.md` examples and diagrams. Add `docs/architecture/explainable-operations.md`. Document the JSON schema, Desired/Actual/Proposed, and plan staleness. Update `SKILL.md` and `AGENTS.md`. Remove superseded duplicate status calculations and compatibility adapters, once no caller needs them. Add changelog and migration notes.

**Exit criteria:** No orphaned calculation can answer "what branches are included?" or "what is held?" any more. Docs match the code. All four gates green.

---

## Definition of Done

The source spec's §40 checklist, minus the desktop rows, plus the constraints above as hard gates:

- [ ] One shared structured repository-state model.
- [ ] Desired, Actual, and Proposed have precise meanings in code.
- [ ] Environment builds persist trustworthy provenance.
- [ ] Promote, demote, rebuild, and release have shared planners, and those planners are the front half of their executors.
- [ ] Those operations execute validated plans and return structured receipts.
- [ ] CLI plans explain current → proposed before any mutation.
- [ ] `--dry-run` uses the same planning path as a real run.
- [ ] `--json` exposes versioned structured plans and results.
- [ ] `hitch status` provides a feature×environment view.
- [ ] Environment equations are rendered by one shared renderer, not hand-formatted per surface.
- [ ] `hitch why` explains branch and environment state.
- [ ] Holds explain conflict partner, files, and next action.
- [ ] Release previews dependent rebuilds, pruning, tags, and remote writes.
- [ ] Normal output explains behaviour; `--verbose` explains mechanism.
- [ ] Activity uses structured events.
- [ ] All significant mutations use the same plan/apply/receipt architecture.
- [ ] Existing Git safety, recovery, conflict, and release semantics remain intact — proven by the differential and crash-fuzz suites passing **unchanged**.
- [ ] `AGENTS.md`, `SKILL.md`, `README.md`, and `docs/architecture/explainable-operations.md` match the final design.
- [ ] `just format`, `just format-check`, `just lint`, `just test` all pass.
