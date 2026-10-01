# P10 — Documentation, compatibility, legacy removal: one path, documented

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When P10 ends, exactly one piece of code can answer "what does this environment contain, and what is held": the shared composition (`compose_environment`), which the plans and predictions all reach, and `core::state` for what was actually built. Every second oracle, every dead narration path and every desktop-only adapter is gone. The open plan/receipt defects are fixed, and the documentation matches the code.

**Architecture:** Five kinds of change, in this order:
1. **Delete dead plumbing** first, because it changes signatures that later tasks touch: `StepNarration`, `StepLogger`, the `on_step` callbacks, `rebuild_environment_opts`, `get_commit_timestamp`, `format_conflict_report`, and blanket `#[allow(dead_code)]`s.
2. **Delete the desktop-only adapters** (user decision; see deviation 1).
3. **Route every conflict prediction through one new helper.** `predict_composition` is `pin_environment_inputs(synchronize: false)` + `compose_environment(OnConflict::Eject)`. The helper replaces all seven preflight call sites: the promote planner, release's dependent planning, `hitch resolve`'s mode selection, `hitch conflicts`, `hitch status`, `hitch tree`, and the approval snapshot. Then the `preflight_*` family and `merge_tree_write_tree_name_only` are deleted, and the compiler proves nothing still calls them.
4. **Fix the plan/receipt defects** carried from P8/P9.
5. **Write the documentation:** architecture, JSON schema, README, SKILL, DEVELOPMENT, and a CHANGELOG with migration notes.

**Tech Stack:** Rust, `serde`, `clap`, `anyhow`. **No new dependencies.**

**Spec:** `docs/explainable-ux-spec.md`: Milestone 13, §2.3 (one source of truth for planning), §9, §10.3 (`--json`), §11 (Desired/Actual/Proposed), §2.4 and §7.4 (staleness, fingerprint), §33 (errors), §34 (backward compatibility), §36.4 (no dual sources of truth), §40 (definition of done). Master plan: `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`. Its **Global Constraints** bind this phase, **except Constraint 2**, which deviation 1 below lifts.

---

## Global Constraints

1. **One composition per kind, reached by one door.** After Task 5, the only
   code that runs git's merge engine to answer "would this compose / what is
   held" is `compose_environment` (environment builds, and every prediction
   via `predict_composition`) and `compose_release` (a release's merge chain,
   a different kind; see `AGENTS.md`, "One composition per kind"). Deleting
   `preflight_compatibility_merge_tree`, `preflight_compatibility_report`,
   `preflight_compatibility_report_local` and
   `GitOperations::merge_tree_write_tree_name_only` is the enforcement: the
   compiler refuses a new caller. Do not re-add a tree-based `--merge-base`
   loop under another name.

2. **A prediction never touches the network, never anchors, never takes a
   lock.** `pin_environment_inputs(.., synchronize: false)` is **not** offline
   enough. It starts with `branch_exists_anywhere(&environment.base)`
   (`src/utils/prelude.rs:810`), which runs `git ls-remote` when the base is not
   local (`src/utils/git_operations.rs:1222-1234`). It also refuses a branch
   that exists only as a cached `origin/*` ref (`:825`).

   So `predict_composition` pins its own inputs, the way
   `build_state_snapshot` does: `rev_parse_opt("refs/heads/<b>")`, then
   `refs/remotes/origin/<b>`, and nothing else. It then calls
   `compose_environment`, which is pure (tested by
   `compose_environment_is_pure_and_deterministic`).

   A branch that resolves nowhere is handled exactly as
   `preflight_compatibility_report_local` handles it today. Read that function
   before writing this, and preserve its behaviour. `hitch status`, `tree`,
   `conflicts`, `why` and `log` stay offline. `rebuild --dry-run`
   (`PlanPurpose::Preview`) keeps using `pin_environment_inputs` unchanged;
   making the preview offline is a behaviour change for another day.

3. **A prediction always holds; it never halts.** It passes
   `OnConflict::Eject` whatever the environment's policy, because the question
   is "what would be held", and `Halt` answers by returning `Err`. It passes
   `replay: false`, matching what every replaced preflight did: none of them
   knew about recorded resolutions. A prediction that replays is a behaviour
   change, and it is not this phase's.

4. **Preserve every existing verdict except where a test proves the old one
   wrong.** Each migration task starts with a *differential* test. It runs the
   old oracle and `predict_composition` over the conflict scenarios in
   `tests/integration/resolve_tests.rs`, `rebuild_tests.rs` and
   `promote_demote_tests.rs`, and asserts the same `(branch, conflicts_with,
   files)` set. Keep that test, reduced to the new side, when the old oracle
   is deleted. Where the two disagree, the composition is right: it is what
   the build does. The disagreement is recorded in `## As executed` and its
   scenario becomes a named regression test. `AGENTS.md`'s
   `--dry-run --replay-resolutions` entry records one such disagreement,
   already fixed in P1.

5. **Master Constraint 6 stands:**
   `test_merge_tree_compose_matches_real_merge_across_scenarios` and the
   four crash-fuzz suites pass **unchanged**. Deleting
   `merge_tree_write_tree_name_only` may delete *its own* differential test in
   `tests/unit/git_operations_tests.rs` (grep `name_only`). That test guards a
   primitive that no longer exists; it is not the composition oracle.

6. **Exit codes do not change.** Human-output changes are intended, and every
   one is listed in `CHANGELOG.md` (Task 8). Machine users are pointed at
   `--json` (spec §34).

7. **`src/core/render.rs` is the only place that chooses words**, and the
   terminology test (`tests/integration/terminology_tests.rs`) stays green.
   A new user-facing string belongs in `render.rs`, or in an error built the
   AGENTS.md way, ending with the command to run next.

8. **`crates/hitch-desktop` source is not edited.** Its build gets worse
   (deviation 1), and nothing else about it changes. Every gate stays
   `-p hitch`-scoped (master Constraint 1), so a workspace-wide `cargo build`
   failing on the desktop crate is expected, not a regression.

9. **`main` stays at `5d81fb2`.** All work lands on `explainable-ux`. Merging
   the program is a separate decision made after P10.

---

## Deviations from the spec and the master plan

1. **The desktop-only adapters are deleted before the desktop migrates.**
   This is the user's decision, taken at authoring time. It overrides master
   Constraint 2 ("do not reshape `WorkspaceIndexModel` or `BranchRow`") and
   the master plan's scope line that nothing may make `crates/hitch-desktop`
   *more* broken. Spec M13 says "remove compatibility adapters only after
   callers have migrated", so this deviates from the spec too. The reason is
   that the desktop is on a separate repair stream, and keeping CLI-side
   adapters for a crate that does not compile preserves a second vocabulary,
   `TimelineItem.summary` prose, that nothing in the CLI reads. The repair
   stream rebuilds against the typed API: `ActivityLog`,
   `RepositoryStateSnapshot`, `MatrixModel`, `WhyExplanation`. Deleted:
   `src/core/timeline.rs`, `src/core/details.rs`,
   `src/core/workspace_index.rs`, `src/core/workspace.rs`. Before deleting,
   Task 2 verifies that none of them has a CLI caller.

2. **Kept, not removed:**
   - `refs/hitch/prev/*` alongside `refs/hitch/backup/*`: both are
     byte-identical, but removing one changes a user-visible ref family;
   - the legacy `refs/hitch/pending-resync/*` reader: it still repairs an
     upgrade taken mid-publish;
   - `rebuilt_at`/`released_at`: `core/state.rs` uses `rebuilt_at` to tell
     `LegacyUnknown` from `NeverBuilt`;
   - `LEGACY_OPERATION_LOCK_WINDOW`: un-purposed history still exists in
     every repo older than P9.

   Each is documented in the architecture doc as a compatibility surface with
   the condition for removing it. Spec M13 says to remove adapters "only after
   callers have migrated", and these have live callers.

3. **Not in P10** (recorded in the master plan's closing notes):
   - recording the release target;
   - `hitch log`'s three-line approval apply;
   - the invisible stuck operation lock (P9's documented limit);
   - collapsing `delete_branch`'s worktree escalation.

   Each is a behaviour or schema change with its own design question, not a
   removal.

---

## Task 1 — Dead plumbing

**Files:**
- `src/utils/prelude.rs`: `StepNarration` (~:1030-1045), the `Log` arm (~:1120),
  `rebuild_environment` (:931), `rebuild_environment_opts` (:978),
  `rebuild_environment_gated`, `compose_environment` (:663) and its `on_step`
  parameter
- `src/utils/progress.rs`: `StepLogger` (:110); delete the file if nothing
  else in it has a caller, and update `src/utils/mod.rs`
- `src/operations/{rebuild,declaration,release,metadata,cleanup}.rs`:
  `on_step` parameters
- `src/commands/{rebuild,release,promote,demote,approvals/approve}.rs`:
  call sites
- `src/utils/git_operations.rs:1020` (`get_commit_timestamp`) and its tests
  at `tests/unit/git_operations_tests.rs:1034,1059`
- `src/utils/conflict_report.rs` (`format_conflict_report` :128 and its dead
  items :20, :74), `tests/unit/conflict_report_tests.rs`: delete what has no
  production caller
- `src/commands/remove.rs:24`: the unread `yes` field
- `src/commands/why.rs:41`, `src/commands/status.rs:43`: change
  `context.verbose = args.verbose` to `context.verbose || args.verbose`
- `#[allow(dead_code)]` in `src/`: every site listed in this task's audit
  step, including the file-level `#![allow(dead_code)]` in
  `src/utils/logging.rs:9` and `src/utils/output.rs:1`
- Tests: existing callers updated; one new test per behaviour change below

**Interfaces:**
- Produces:
  - `pub fn rebuild_environment(context: &GlobalContext, env_name: &str) -> Result<RebuildOutcome>`
    (no narration argument). `rebuild_environment_gated` keeps its gate and
    loses its narration argument.
  - `compose_environment(context, inputs, env_name, on_conflict, replay, require_signed_resolutions) -> Result<CompositionResult>`
    (no `on_step`). Tasks 3–5 call this signature.
  - Planners and executors lose `on_step`. The one real consumer,
    `commands/release.rs:85,269` (`log_verbose(step)`), becomes
    `context.log_verbose(...)` calls inside `operations/release.rs` at the same
    points, so `--verbose` output for a release is unchanged. Verify this by
    diffing `hitch --verbose release … --dry-run` output before and after.
    Paste both in the report.

- [x] **Failing tests first** for the two behaviour changes:
  - `hitch --verbose why <x>` and `hitch --verbose status` honour the global
    flag. Model them on P9's `a_global_verbose_flag_is_honoured_by_log` in
    `tests/integration/log_tests.rs`.
- [x] Remove the plumbing. Callers passing `StepNarration::Suppressed` or
  `&mut |_| {}` lose that argument. `rebuild_environment_opts` merges into
  `rebuild_environment` only if no caller passes a non-default replay or
  `on_conflict_override`. The inventory says none do outside `prelude.rs`;
  re-verify with grep, and if one does, keep `_opts` and say so in the report.
- [x] Audit each `#[allow(dead_code)]` in `src/`
  (`types.rs:203`, `utils/confirm.rs:52`, `utils/diff.rs:249`,
  `utils/snapshot.rs:227`, `commands/global_context.rs:53,67`,
  `git_operations.rs:21`, `utils/logging.rs:9`, `utils/output.rs:1`; the
  `core/{details,workspace_index,timeline}.rs` ones go with Task 2):
  - Remove the attribute and run `just lint`.
  - Delete whatever clippy then reports as dead.
  - Keep an attribute only where the item is used from `tests/` alone and the
    test is worth keeping. In that case add a one-line reason comment.
  - Report the list.
- [x] Update `AGENTS.md`:
  - the "nested operation narrates nothing" gotcha: the mechanism is gone, and
    the rule now is that executors call `context.log_verbose` for mechanism;
  - the `src/utils/prelude.rs` and `src/operations/` entries;
  - delete every mention of `StepNarration`, `StepLogger` and
    `format_conflict_report` as live code.
- [x] Gates: `just format`, `just format-check && just lint`, `just test`.
  Commit: `P10 Task 1: delete the dead narration plumbing and other dead code`.

---

## Task 2 — Delete the desktop-only adapters

**Files:** delete `src/core/timeline.rs`, `src/core/details.rs`,
`src/core/workspace_index.rs`, `src/core/workspace.rs`. Modify
`src/core/mod.rs`, `AGENTS.md`, and any test that exercises them (grep
`timeline::`, `details::`, `workspace_index`, `BranchRow`, `WorkspaceIndexModel`
under `tests/`).

**Interfaces:**
- Consumes: nothing.
- Produces: nothing new. `core::activity`, `core::state`, `core::status` and
  `core::why` are the typed API the desktop repair stream builds against.

- [x] Prove there is no CLI caller: `grep -rn "timeline::\|details::\|workspace_index\|workspace::BranchRow\|WorkspaceIndexModel\|BranchRow" src/ | grep -v "^src/core/\(timeline\|details\|workspace_index\|workspace\)\.rs"`
  must be empty. If it is not, stop and report NEEDS_CONTEXT naming the caller.
- [x] Delete the four files, their `mod` lines, and their tests. Anything in
  them that only they used, such as a helper in `core/activity.rs` that
  `timeline.rs` imported, goes too, *if* nothing else uses it.
- [x] `git diff --stat -- crates/` shows nothing (Constraint 8). Record in the
  report the list of desktop symbols that no longer resolve:
  `grep -rn "hitch::core::" crates/hitch-desktop/src-tauri/src`.
- [x] Update `AGENTS.md`:
  - the `src/core/` entry: remove `workspace_index.rs`, `details.rs` and the
    timeline-adapter lines;
  - the "What this is" paragraph: the desktop crate no longer compiles
    against the core. State this and point at deviation 1 of this plan.
- [x] Gates. Commit: `P10 Task 2: delete the desktop-only adapters`.

---

## Task 3 — `predict_composition`, and the planners stop running a second merge

**Files:**
- `src/utils/prelude.rs`: add `predict_composition`; rewrite
  `pre_promote_conflict_reason` (:2695) over it; delete
  `check_pre_promote_conflicts` (:2668) if nothing else uses it once the
  planner has moved
- `src/operations/declaration.rs:512` (promote planner)
- `src/operations/release.rs:821` (`plan_dependents` skip)
- Tests: `tests/integration/promote_demote_tests.rs`, `release_tests.rs`,
  and a new unit/integration test for `predict_composition`

**Interfaces:**
- Consumes: Task 1's `compose_environment` signature.
- Produces:

```rust
/// What a build of `environment` would produce right now, from local refs.
/// Offline, lock-free, anchor-free; always holds, never halts; never replays.
pub fn predict_composition(
    context: &GlobalContext,
    environment: &Environment,
    env_name: &str,
) -> Result<CompositionResult>;
```

It pins through a private
`pin_inputs_offline(context, environment) -> Result<PinnedInputs>`
(Constraint 2). It never calls `pin_environment_inputs` or
`branch_exists_anywhere`. Add a test that `predict_composition` succeeds, and
issues no `ls-remote`/`fetch`, with `remote.origin.url` pointing at a
nonexistent path (`GIT_TRACE=1`, grep the trace), for a base that exists only
as `refs/remotes/origin/<base>`.

Both callers build a *proposed* `Environment`, a clone with the new branch
appended in promotion order, and ask whether the result holds that branch.

**Behaviour, and the P9 D6 defect this fixes.** A conflicting promote used to
be refused with `"{new_branch} conflicts with {base_branch}"`
(`prelude.rs:2737-2741`) and a `git rebase <base>` remedy, even when the real
conflict was with a peer. `CompatibilityFailure` had no field for the peer.
`CompositionResult.held[i].conflicts_with` names the real partner:
- the base → the old wording and the rebase remedy;
- a peer → `"<new> conflicts with <peer>, which is already in <env>"`, with
  remedy `hitch resolve <env> --branch <new>` once it is promoted and held,
  or rebasing onto the peer. Choose the remedy by reading what `hitch
  resolve` supports for a *not-yet-promoted* branch, and state the choice in
  the report.

`render.rs` owns both wordings (Constraint 7), and the refusal stays a
`PlanWarningKind::PolicyRefusal`.

- [x] **Differential test first** (Constraint 4). Over the conflict scenarios:
  - `pre_promote_conflict_reason`'s old verdict, conflict or not, must agree
    with `predict_composition(proposed).held.iter().any(|h| h.branch == new)`;
  - `plan_dependents`' old skip decision must agree likewise.

  Commit the test while the old code still exists, then migrate.
- [x] **Failing test for D6's partner:** add `a`, then `b` conflicting with `a`
  but not with the base; `promote b` is refused and the message names `a`,
  not `main`.
- [x] Implement `predict_composition` and migrate both callers. Delete
  `preflight_compatibility_merge_tree` (:2455) once it has no caller, and the
  `CompatibilityFailure` struct if nothing uses it.
- [x] Gates. Commit: `P10 Task 3: planners predict through the one composition`.

---

## Task 4 — `hitch resolve` chooses its mode from the composition

**Files:** `src/commands/resolve.rs:131-146` (mode choice) and `:170-190`
(`resolve_target_branch`); `tests/integration/resolve_tests.rs`;
`tests/integration/resolve_crash_recovery_tests.rs` (must pass unchanged).

**Interfaces:**
- Consumes: `predict_composition` (Task 3).
- Produces: `resolve` with no call to `preflight_compatibility_report`.

`hitch resolve` is mutating, but the mode decision is a *read*. It uses the
prediction as is. It does not sync, because it did not sync before, and a
resolve that fetches is a behaviour change for another phase. Mode A (the
branch conflicts with the base) versus Mode B (it conflicts with a peer)
reads `held.conflicts_with == environment.base`, the same comparison as
`:146` today. "Nothing to resolve" is `held` not containing the target
branch. `resolve_target_branch` picks the single held branch, or errors on
zero or several, as it does now.

- [x] **Differential test first:** for every scenario in `resolve_tests.rs`
  that reaches mode selection, the old report's `(branch, conflicts_with,
  files)` for the target equals the prediction's. Where a scenario disagrees,
  follow Constraint 4: the composition wins, and the scenario becomes a named
  regression test.
- [x] Migrate both call sites. The whole `resolve_tests.rs` and
  `resolve_crash_recovery_tests.rs` suites pass.
- [x] Update the `AGENTS.md` gotcha "`preflight_compatibility_report` is
  **not yet** a display-only function". After this task no mutation depends
  on it. Rewrite the entry to say so, and say that Task 5 deletes it.
- [x] Gates. Commit: `P10 Task 4: resolve reads its mode from the composition`.

---

## Task 5 — Display predictions, the approval snapshot, and deleting the last oracle

**Files:**
- `src/commands/conflicts.rs:44`, `src/commands/status.rs:443` ("would be
  held"), `src/commands/tree.rs:160`: move to `predict_composition`
- `src/commands/status.rs:465,749`: the `is_branch_merged_into` calls that
  re-derive "already in base" outside the snapshot. Read the snapshot's
  `MatrixCell::AlreadyInBase` / `core::state` answer instead.
- `src/utils/snapshot.rs:121,178,192`: `check_for_merge_conflicts` /
  `merge_tree_has_conflicts`, the approval snapshot's conflict oracle; move
  to `predict_composition`
- delete `preflight_compatibility_report` (:2536),
  `preflight_compatibility_report_local` (:2598), and
  `GitOperations::merge_tree_write_tree_name_only` (`git_operations.rs:328`)
  together with its own differential test (Constraint 5)
- Tests: `tests/integration/status_tests.rs`, `state_model_tests.rs`,
  `tree` / `conflicts` / approval tests as affected

**Interfaces:** consumes `predict_composition` and `build_state_snapshot`.
Produces no new API.

Keep the rule from `AGENTS.md`: **a fact and a prediction must not share a
word.** `status`'s ⛔ reads the record's `held` (fact) first, and uses the
prediction only to fill the gap, with the wording "would be held on the next
rebuild". `test_status_distinguishes_a_held_branch_from_one_that_would_be_held`
must pass unchanged.

- [x] **Differential test first**, as in Tasks 3–4, for `conflicts` and
  `status`'s would-be-held verdicts and for the snapshot's conflict boolean.
- [x] Migrate. Delete the three functions and the primitive. The compiler is
  the proof: `cargo build -p hitch` must succeed with them gone.
- [x] Update `AGENTS.md`:
  - rewrite "One composition per kind" to say the preflight family is gone
    and that predictions go through `predict_composition`;
  - rewrite the "Wrong merge-base in `merge-tree` preflights" gotcha as
    history, keeping the warning for `merge_tree_compose`;
  - rewrite "A fact and a prediction must not share a glyph" to name the new
    prediction path.
- [x] Gates. Commit: `P10 Task 5: one oracle — status, tree, conflicts and approvals predict through the composition`.

---

## Task 6 — A refusal never narrates a rollback

**Files:** `src/commands/promote.rs:97-170`, `src/commands/demote.rs:85-130`,
`src/utils/rollback.rs:16-37`, `tests/integration/promote_demote_tests.rs`.

The defect (P9 D6, first half). `promote.rs:126` arms `previous_config`
before `apply_declaration_plan` (`:127`), and the policy refusal is raised
*inside* the apply (`operations/declaration.rs:807`, `plan.blocked_by()` →
`PolicyBlocked`). So a refused promote prints "Rolling back … / Declaration
restored — nothing was changed" and costs extra metadata commits. The
comment at `:97-117` says every refusal happens before arming, and that is
false for a policy block. `demote.rs` has the same shape.

- [x] **Failing test first:** a conflict-refused promote (Task 3's scenario):
  - stderr has no "Rolling back"/"restored";
  - `hitch-metadata` gains exactly **2** commits (lock and unlock; see the
    `AGENTS.md` rollback gotcha);
  - exit code is 1.

  Add the same test for demote, if demote can be policy-refused.
- [x] Fix. Check `plan.blocked_by()` in the command *before* arming, returning
  the same `PolicyBlocked` error the executor would, so the error is
  identical. Alternatively, arm only after the executor has passed its
  blocked check. Pick whichever keeps a single place deciding the refusal,
  and state the choice. Correct the comment at `:97-117`.
- [x] Gates. Commit: `P10 Task 6: a refused promote or demote rolls nothing back`.

---

## Task 7 — Release plan: an honest "Will not change", and a remedy that is not the failed command

**Files:** `src/operations/release.rs:436-460` (`unaffected`) and `:933-967`
(`build_conflict_error`), `src/core/render.rs:148-153`,
`tests/integration/release_tests.rs`.

- [x] **Failing tests first:**
  - A release that rebuilds `qa` and `prod` as dependents. "Will not change"
    lists neither of them, and lists each unaffected item exactly once.
    Decide what the list holds by reading `render_plan`'s "Will not change"
    doc and the release planner's comment: environments, branches, or both,
    in deterministic order. P9 saw `prod, search, qa, search`.
  - A release that fails on a merge conflict. The printed remedy does not
    contain the exact failing invocation `hitch release <env> <target>` as its
    final step. The last step is the concrete fix, and the retry is only
    worded "then run `hitch release <env> <target>` again" after the fixing
    step.
- [x] Fix. `unaffected` excludes every environment in `dependents` and
  de-duplicates in plan order. `build_conflict_error`'s remedy names the
  conflicting branch and the resolution command first. Read the four current
  steps; step 4 is the repeat.
- [x] Gates. Commit: `P10 Task 7: release plans say what they will not touch, once`.

---

## Task 8 — Documentation

**Files:**
- Create `docs/architecture/explainable-operations.md`
- Create `docs/architecture/json-schema.md`
- Create `CHANGELOG.md` (repo root)
- Rewrite the stale sections of `README.md` and `SKILL.md`
- Update `DEVELOPMENT.md:295-312` (the structure tree) and `AGENTS.md`

**Interfaces:** consumes the code as it stands after Tasks 1–7. Every claim
must be checkable against it.

Content requirements (spec M13 and §34):

- `explainable-operations.md`:
  1. **Desired / Actual / Proposed.** Precise definitions, and where each
     lives in code (`core::state`, the build record, the plan's projections).
  2. **Environment health states**, and `LegacyUnknown` as a normal state.
  3. **Plan → validate → apply → receipt**, with the fingerprint and what
     makes a plan stale. The whitelist principle: why an unrelated branch
     does not stale a plan.
  4. **Composition as the one oracle.** `compose_environment` for builds and
     predictions, and `compose_release` for releases, with the reason.
  5. **The receipt's warning contract** (plan predicts; receipt reports what
     the apply learned).
  6. **Activity events** and their derivation (Phase A; no persistence).
  7. **Compatibility surfaces kept on purpose:** deviation 2's list, each with
     its removal condition.
  8. **"Adding a new operation":** the checklist from `AGENTS.md`'s
     `src/operations/` entry, as prose a newcomer can follow.

  Diagrams: at most two, in Mermaid or ASCII, for the plan/apply sequence and
  the Desired/Actual/Proposed relationship.
- `json-schema.md`: every envelope, with one real example each, taken by
  running the debug binary, not hand-written:
  - mutation: `{schema_version, plan, receipt}`, with `receipt: null` for
    `--dry-run`;
  - read-only: `{schema_version, status|why|log}`;
  - approve below its threshold: `{schema_version, plan: null, receipt: null, approval}`;
  - cleanup: the `failures` key.

  Also document enum casing, timestamps (RFC 3339 UTC), the `schema_version`
  policy, and the `--json`-without-`--yes` exit 1. It lists the fourteen
  commands, and a test keeps it honest: extend
  `tests/integration/json_support_tests.rs` to parse the command list out of
  `json-schema.md` too and compare it with the `--json` doc comment's list.
- `CHANGELOG.md`: one `## Unreleased — explainable UX` entry, in Keep a
  Changelog style:
  - Added: `hitch why`, `hitch log`, `--json`, `--dry-run` everywhere, plans
    and receipts, `hitch status` matrix;
  - Changed: every intentional human-output change, grouped by command, from
    the P6–P10 plans' "As executed" sections;
  - Fixed;
  - Removed: the desktop adapters and `preflight_*`;
  - **Migration notes**:
    - `hitch.json` gains `lock_purpose` (serde-defaulted; older hitch
      ignores it);
    - `refs/hitch/state/*` is new and must not be pruned;
    - `LegacyUnknown` persists until an environment's first rebuild;
    - scripts should use `--json`;
    - exit codes are unchanged, and exit 2 means "applied with holds".
- `README.md`: rewrite "See what is deployed" (`:506-535`: status matrix,
  `why`, `log`), "Rebuilding" (`:382-407`: plan/receipt, `--dry-run`, exit
  2), and the Common commands table (`:569-598`: all 27 subcommands from
  `src/cli.rs`, or clearly "common" ones plus a pointer to `hitch --help`).
  Examples are real output from the debug binary against a throwaway repo.
- `SKILL.md`: add `why`, `log`, `--json` in Global Flags, `--dry-run` for
  every mutating command, plan/receipt reading, and exit code 2. It stays
  condensed.
- `DEVELOPMENT.md`: the structure tree includes `core/`, `operations/` and
  `crates/`.

- [x] Write the docs, with real output throughout.
- [x] Add the `json-schema.md` ↔ doc-comment test. It fails first against a
  deliberately wrong list, then passes.
- [x] Gates. Commit: `P10 Task 8: architecture, JSON schema, README, SKILL, CHANGELOG`.

---

## Task 9 — Manual verification, definition of done, and closing the program

**Files:** `AGENTS.md`, the master plan, this file.

- [x] Build the **debug** binary and have a fresh agent walk a realistic day
  against a throwaway repo with `--yes --no-push`, in the same shape as P8
  Task 11 and P9 Task 9:
  - a conflicting promote: the refusal names the real partner and no rollback
    narration appears;
  - `hitch resolve` in both modes;
  - `hitch conflicts`, `status`, `tree`, `why`, `log`;
  - a release with dependents: "Will not change" is right, and the conflict
    remedy is right;
  - `cleanup`;
  - the approval round trip.

  Check also that every README example still matches real output.
  Walkthrough defects are fixed before closing; they are not deferred.
- [x] Gates, in order, all clean.
- [x] Run `grep -rn "preflight_compatibility\|merge_tree_write_tree_name_only\|StepNarration\|StepLogger\|format_conflict_report\|get_commit_timestamp" src/ tests/ AGENTS.md SKILL.md README.md`.
  It is empty except for history prose in `AGENTS.md` that says the item is
  gone.
- [x] Master plan:
  - tick the **Definition of Done** checklist item by item, and next to each
    tick name the test or doc that proves it;
  - mark P10 **COMPLETE**;
  - add the commit list;
  - write a short **"Program closed"** section: what shipped, deviation 1
    (the desktop), deviation 3's open items, and the next decision (merging
    `explainable-ux` into `main`), which is the user's.
- [x] Add `## As executed` to this file.

---

## As executed

P10 complete 2026-10-01. Suite: 204 lib + 632 integration + 1 `no_args_help`, zero failed; format, format-check, lint clean. Legacy-name grep is empty except history prose that says the item is gone.

### Commits
- Task 1 `1828a89` dead plumbing (`StepNarration`/`on_step`, `rebuild_environment_opts` merged, dead-code allows). Task 2 `81780d8` desktop adapters deleted.
- Task 3 `a67e22a` (`predict_composition` + differential against the old oracles), `3b4183e` (planners migrated, old oracle callers deleted).
- Task 4 `345c131`, `344106c` (resolve reads its mode from the composition; fix restores its sync).
- Task 5 `4f1c7f3`, `7cd01db`, `5ceae3d` (conflicts, status, tree, approval snapshot; the last oracle deleted; `contained_in_base`).
- Task 6 `2b06349` (refusal rolls nothing back). Task 7 `c905bb2` (release plan wording). Task 8 `39a1bfc`, `0f7968b` (docs, `HITCH_YES` fix).
- Task 9: `c9895b4`, `7cb4b5c` (true partner), `056413c`, `9bf4170`, `09bb270` (walkthrough fallout), `9e6e989` (deferred-minor sweep), then this docs commit.

### What landed
One conflict oracle. `predict_composition` is `compose_environment` over offline pinning (Eject, no replay, no lock); promote and release planners, `resolve`, `conflicts`, `status`, `tree` and the approval snapshot all read it, and `preflight_*` / `merge_tree_write_tree_name_only` are gone. Dead narration plumbing and the four desktop-only adapters are deleted; `get_commit_timestamp` too. A refused promote/demote writes and rolls back nothing (`apply_may_write`). The release plan states what it will not touch once, and its conflict remedy is not the failed command. Docs: `docs/architecture/explainable-operations.md`, `docs/architecture/json-schema.md`, `CHANGELOG.md`, README, SKILL.md, AGENTS.md.

### Rulings (meaning preserved)
- **resolve keeps its own sync (T4).** `resolve` calls `synchronize_branches` on base and promoted branches before `predict_composition`; the prediction stays pure/offline. Why: Constraint 4 (preserve verdicts); the plan's "did not sync before" was wrong. Cost: resolve keeps its network dependency, as before.
- **Migrated callers keep their old oracle's sync at the call site (T5).** Why: same lesson; cost: `conflicts` keeps a network dependency.
- **`contained_in_base` is a snapshot fact (T5).** A per-branch "already in the base now" fact computed once in `build_state_snapshot` by local ancestry; the cleanup hint and status row read it. Why: a display reads the verdict, not re-derives it. Cost: one ancestry check per promoted branch.
- **`HITCH_YES` parsed in code (T8).** clap `BoolishValueParser` (1/true/yes/on) rather than rewording four promises of `=1`. Why: it was a real bug. Cost: a small code change in a docs task, plus a CHANGELOG Fixed entry.
- **Held-branch partner decided in `compose_environment` (T9, D1).** Base if the branch conflicts with base alone, else the first included peer it conflicts with pairwise, else the last composed. Why: the one oracle should name the true partner; fixes promote refusal, hold display and resolve's Mode A/B quirk at once. Cost: extra merge-tree calls per held branch only; `conflicts_with` in build records changes for base-collision-after-clean-peer (CHANGELOG).
- **Approver eligibility is a planner refusal (T9, D3).** "Not enough eligible approvers" is a `PolicyRefusal` decided before arming. Why: the Task 6 rule. Cost: none.
- **Walkthrough minors fixed too**, except short SHAs, which stay in normal output (spec §17).
- **Local unpushed commits rewritten (T9 fix B).** Three commits folded into two so every commit builds (the first failed `cargo check` alone). Why: bisectability. Cost: the SHAs in the interim reports (`be1dfeb`, `9c3e5bf`, `8d04ec9`) no longer exist; the final tree is byte-identical.

### Deviations from the tasks as written
- Task 4: the plan said the old preflight did not sync; it did (`preflight_compatibility_report` called `synchronize_branches`). Resolve keeps its sync.
- Task 5: needed a snapshot fact (`contained_in_base`) the plan did not list, to keep the cleanup hint and status row after the oracle's deletion.
- Task 3 note: `predict_composition` errors on an unresolvable base where the old `_local` oracle returned no conflicts; display callers map `Err` to empty to preserve behaviour.
- Task 9: the `compose_environment` partner change (D1) touched the oracle itself, beyond a display fix; the walkthrough also found D2 (`conflicts` printed "policy: Eject") and D3 (a no-approver promote narrated a rollback). All three re-checked by eye in a throwaway repo at close: D1 names `main` and the remedy is `git checkout c && git rebase main`, then `hitch promote c dev`; D2 has no "Eject"; D3 is refused by the plan with no rollback text.
- Task 8: `HITCH_YES` fix and a CHANGELOG correction came from review, not the brief.

### Behaviour changes
All listed in `CHANGELOG.md`. In brief: an already-held sibling no longer blocks promoting an unrelated branch; promote prediction no longer syncs; the approval snapshot's `merge_conflicts` is true for peer-only collisions; status cleanup block is offline; `conflicts_with` names the true partner; an unresolvable base is an `Err` in the release plan's dependents; release plan `current` is null; `HITCH_YES=1` works.

### Deferred minors
- Release: a conflict against an earlier branch of the same release is not fixed by rebasing onto the target (pre-existing); the retry line could mention force-push; the unaffected-environment arm is untested.
- Step-2 partner probe uses the peer's tip, not base-plus-peer (stale-peer label limit); the combination-only test relies on `merge.directoryRenames=true`.
- Post-migration "differential" tests are regression tests only; `tree.rs` verbose fix untested.
- A lost-test note: three lib tests went with deleted modules in Task 2 (report said none removed).
- Final review: release dependents are predicted against the pre-release target (open: compose against the release result); display callers (`status`, `tree`) now map only an unresolvable base to "no prediction" via `predict_composition_if_base_resolves` and warn on any other error; approver feasibility is one predicate (`eligible_approver_shortfall`) and one renderer.
- Open program items are listed in the master plan's "Program closed".
