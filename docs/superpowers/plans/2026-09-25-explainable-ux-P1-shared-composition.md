# P1 — Shared Composition Primitive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Extract the pure composition half of `rebuild_environment_opts` into one reusable function, so that planning and executing a build are provably the same computation rather than two implementations that must agree forever — and so `hitch rebuild --dry-run` stops being a different calculation from a real rebuild.

**Architecture:** `rebuild_environment_opts` (`src/utils/prelude.rs:616-824`) is already two halves. Steps 1–5 — synchronize, pin concrete SHAs, snapshot the remote, then loop `merge_tree_compose` over the pinned SHAs — are pure: they take pinned SHAs and return `(result_sha, held, replayed)`, touching no lock, no checkout, and no ref. Steps 6–9 — anchor, publish, drop anchor, stamp — are the only ones that mutate. Extract the first half as `compose_environment`; the second becomes `rebuild_environment_opts` calling it. P2 then records `CompositionResult` as the build's provenance, and P4 uses the very same call as its planner.

**Tech Stack:** Rust, `anyhow`, the existing `GitOperations` plumbing, `HitchTestFramework`.

**Parent plan:** `docs/superpowers/plans/2026-09-25-explainable-ux-program.md` (P1). Read its Global Constraints before starting.

---

## Why this is not a cosmetic refactor

Today `hitch rebuild <env> --dry-run` (`src/commands/rebuild.rs:78`) calls `preflight_compatibility_report` (`src/utils/prelude.rs:2080`) — a **tree-based** loop over `merge_tree_write_tree_name_only` with an explicit `--merge-base`. The real build is a **commit-based** loop over `merge_tree_compose` (ORT, which picks its own merge-base). Two wrong answers follow from that split:

1. `hitch rebuild dev --dry-run --replay-resolutions` reports branches as held that replay would have resolved. The dry-run branch short-circuits at `rebuild.rs:77` into a preflight that knows nothing about resolutions.
2. The two paths enter the merge engine through different doors — which is precisely the wrong-merge-base bug class `AGENTS.md` documents at length, where passing a tip instead of a true common ancestor makes `merge-tree` silently fast-forward instead of reporting a conflict.

Eliminating the second implementation removes both. `preflight_compatibility_report` then has exactly one remaining caller class — the read-only `hitch conflicts` / `hitch status` display paths that must stay fast and offline (`preflight_compatibility_report_local`) — and that is legitimate, because a *display* preflight is allowed to be an approximation. What is not legitimate is the *execution* path having a cheaper sibling.

---

## Global Constraints

- `just format`, `just format-check && just lint`, `just test` clean before any task is done. **All four are `-p hitch`-scoped — never run a bare workspace-wide cargo command**, because `hitch-desktop` does not compile and is not ours.
- **This is a pure move.** The 354 existing tests must pass **unchanged**. If a test needs editing to accommodate the extraction, the extraction is wrong — investigate before touching the test.
- `test_merge_tree_compose_matches_real_merge_across_scenarios` (`tests/unit/git_operations_tests.rs`) and all four `HITCH_TEST_ABORT_AFTER` crash-fuzz suites are the regression oracle. They are not to be edited.
- Do not touch `RefEdit::Update`'s `expected_old` semantics, the pinned-SHA discipline, the eject/halt decision order, or the replay-before-halt ordering. See the parent plan's Global Constraints 4 and 7.
- **Never sort `inputs.branches`.** Composition order is semantic.
- Comments explain *why*, not *what*. The extracted function inherits the existing comments about pinned SHAs, ORT equivalence, and eject-as-don't-advance; do not add new commentary restating the loop.

---

### Task 1: Extract `compose_environment`

**Files:**
- Modify: `src/utils/prelude.rs:571-578` (next to `RebuildOutcome`, where the new types belong)
- Modify: `src/utils/prelude.rs:616-824` (`rebuild_environment_opts` — extract the loop, keep steps 1–2 and 6–9)
- Modify: `src/utils/prelude.rs:1332-1341` (read-only: `try_replay_resolution` is already free-standing and moves in unchanged)

**Interfaces:**
- Produces: `PinnedInputs { base_name: String, base_sha: String, branches: Vec<(String, String)> }` — `branches` in configured promotion order, never sorted.
- Produces: `CompositionResult { result_sha: String, included: Vec<String>, held: Vec<CompatibilityConflict>, replayed: Vec<String> }`.
- Produces: `pub fn compose_environment(context: &GlobalContext, inputs: &PinnedInputs, on_conflict: OnConflict, replay: bool, require_signed_resolutions: bool, on_step: &mut dyn FnMut(&str)) -> Result<CompositionResult>`.
- Consumes: nothing new. `try_replay_resolution`, `merge_tree_compose`, `commit_tree`, `rev_parse`, and `conflict_result_from_compose` are all called exactly as they are today.

**Note on `remote_env_sha_before`:** it is deliberately *not* part of `PinnedInputs`. It is not a composition input — it exists so the eventual push is leased against what was observed *before* the build, and it belongs to the publish half. The caller captures it alongside the pinned inputs, at the same moment, so the two observations stay coherent; P4's fingerprint covers it as a remote ref.

- [x] **Step 1: Add the two types**

Insert above `RebuildOutcome` at `src/utils/prelude.rs:571`, with doc comments stating the two invariants that matter — that `branches` order is semantic and must never be sorted, and that the whole computation is a pure function of `inputs` (which is what lets a plan and an execution be the same call):

```rust
/// Concrete commits a composition will build from, resolved once up front.
///
/// The order of `branches` is the configured promotion order and is
/// load-bearing: composition is sequential, and a later branch is checked
/// against everything that actually accumulated before it. Never sort.
pub struct PinnedInputs {
    pub base_name: String,
    pub base_sha: String,
    pub branches: Vec<(String, String)>,
}

/// What a composition produced, with no side effects of its own.
///
/// A pure function of [`PinnedInputs`] plus the arguments to
/// `compose_environment` — no ref moves, no locks, no checkouts, no network.
/// This is what lets the dry-run preview and the real build be the same call.
pub struct CompositionResult {
    pub result_sha: String,
    pub included: Vec<String>,
    pub held: Vec<CompatibilityConflict>,
    pub replayed: Vec<String>,
}
```

- [x] **Step 2: Add `compose_environment`**

Move the closure at `src/utils/prelude.rs:700-788` into a new function, verbatim in its decision logic. The mechanical changes, and only these:

- `let mut composed = base_sha.clone();` becomes `let mut composed = inputs.base_sha.clone();`
- `let mut last_composed = environment.base.clone();` becomes `let mut last_composed = inputs.base_name.clone();`
- `for (branch, sha) in &pinned_branches` becomes `for (branch, sha) in &inputs.branches`
- `logger.step(format!("Merging '{}'", branch));` becomes `on_step(&format!("Merging '{}'", branch));`
- `OnConflict::Halt` becomes the `on_conflict` parameter
- `config.require_signed_resolutions` becomes the `require_signed_resolutions` parameter
- `env_name` — needed for the halt message and the merge message — comes from... see Step 3.
- The trailing `if environment.branches.is_empty() { logger.step(...) }` moves to the caller, because it is a step-count concern, not a composition concern.
- `held`, `replayed`, and `confirmed_replay_keys` move from the caller's stack into the function; the first two become part of the returned `CompositionResult`.
- `included.push(branch.clone())` is added on both the clean-advance path and the replay path — this is new bookkeeping, not new behaviour, and P2 depends on it.

On the hold path, keep the `context.log_warning` call exactly where it is. Logging is not the problem this phase solves, and the message is user-facing; removing it is out of scope.

- [x] **Step 3: Resolve the `env_name` dependency**

The halt error message (`format_conflict_report(branch, ..., &environment.base, env_name, ...)`) and the merge message (`format!("Hitch: merge {} into {}", branch, env_name)`) both need the environment name. `PinnedInputs` deliberately does not carry it — the name is not an input to the *composition*, and P4's plan already knows the environment it is planning for.

Add an `env_name: &str` parameter to `compose_environment`. Do not smuggle it in through `base_name`; they are different concepts (`base_name` is the base *branch*, which for a released environment is the release target, not the environment).

- [x] **Step 4: Repoint `rebuild_environment_opts`**

The function becomes, in order: acquire the rebuild lock → read config → sync branches → pin inputs → snapshot `remote_env_sha_before` → compute `timestamp` → **call `compose_environment`** → anchor `refs/hitch/build/<env>/<ts>` → `publish_environment_build` → drop anchor.

Construct the `StepLogger` exactly as today (`total_steps = 2 + merge_steps`, `src/utils/prelude.rs:640-646`) and pass a closure:

```rust
let composition = compose_environment(
    context,
    &PinnedInputs { base_name: environment.base.clone(), base_sha, branches: pinned_branches },
    environment.on_conflict,
    replay,
    config.require_signed_resolutions,
    &mut |step| logger.step(step.to_string()),
)?;
```

The `if environment.branches.is_empty() { logger.step("No promoted branches to merge".to_string()); }` step must still fire, now in the caller, so the `[n/N]` numbering stays identical. Then the existing `RefEdit`/anchor/publish/stamp sequence continues untouched.

- [x] **Step 5: Update the caller to use the returned result**

`RebuildOutcome { held, replayed }` is still what `rebuild_environment_opts` returns, now sourced from `composition.held` and `composition.replayed`. Leave `RebuildOutcome`'s own shape alone — `src/commands/rebuild.rs` and `src/commands/resolve.rs` both consume it, and changing it is P4's business, not this phase's.

- [x] **Step 6: Run the full suite and confirm nothing moved**

`just test`. Expect the same pass count P0 Task 1 recorded, with no test edited. Then diff the git output of a real rebuild before and after the change to confirm the composed commit messages are byte-identical:

```bash
just build
# in a throwaway /tmp repo: rebuild an env, then
git log --format='%s' -5 dev
```

The subjects must still read `Hitch: merge <branch> into <env>` in the same order, and a branch already contained in the base must still produce **no** commit (the `composed_tree == outcome.tree_oid` short-circuit at the old line 715-718 is what preserves that, and it is exactly the kind of subtlety a refactor drops).

---

### Task 2: Prove composition is a pure function of its inputs

**Files:**
- Test: `tests/integration/rebuild_tests.rs`

**Interfaces:**
- Consumes: `compose_environment` (Task 1), via the CLI rather than directly — this is a behavioural test, not a unit test of a private function.
- Produces: no new production API.

- [x] **Step 1: Write the test**

Two rebuilds of an unchanged environment must produce the same **tree**, though not the same commit SHA — `commit_tree` stamps the ambient wall clock with no `GIT_AUTHOR_DATE` override, so identical inputs legitimately yield different commit OIDs. This is the same trap `crash_recovery_tests.rs` already documents, and the reason the assertion is on `^{tree}`:

```rust
#[test]
fn test_rebuild_is_deterministic_for_unchanged_inputs() -> anyhow::Result<()> {
    // ... HitchInit env with 3 promoted branches, all clean ...
    env.hitch().run(&["rebuild", "dev"])?;
    let first_tree = env.git().rev_parse("dev^{tree}")?;
    let first_sha = env.git().rev_parse("dev")?;

    env.hitch().run(&["rebuild", "dev"])?;
    let second_tree = env.git().rev_parse("dev^{tree}")?;
    let second_sha = env.git().rev_parse("dev")?;

    assert_eq!(first_tree, second_tree, "same inputs must compose to the same tree");
    // Recorded, not asserted: the commit SHAs are expected to differ.
    eprintln!("commit shas: {first_sha} vs {second_sha}");
    Ok(())
}
```

If the two commit SHAs turn out to be *equal*, that is also fine and worth a comment — it just means the two rebuilds landed in the same second. Do not assert on it either way.

- [x] **Step 2: Also assert held branches are stable across rebuilds**

Extend the test (or add a sibling) with a held-branch environment: rebuild twice, assert the same branch is held both times, with the same `conflicts_with` partner and the same conflicted file list. This is the property P4's plan depends on — a plan that predicts "held: dashboard vs payments" must be able to trust that a second identical composition reaches the same verdict.

- [x] **Step 3: Run it**

`just test-file rebuild`. Both properties must hold on the first run; if either fails, the composition is not pure and Task 1 has a real bug — investigate rather than adjusting the test.

---

### Task 3: Repoint `rebuild --dry-run` onto the shared primitive

**Files:**
- Modify: `src/commands/rebuild.rs:77-107` (the `if args.dry_run` block)
- Modify: `src/commands/rebuild.rs:120-128` (the halt pre-check, which becomes redundant for the dry-run path)
- Read: `src/utils/prelude.rs:2080` (`preflight_compatibility_report` — callers narrow, do not delete)

**Interfaces:**
- Consumes: `compose_environment`, `PinnedInputs` (Task 1).
- Produces: no new public API. `rebuild --dry-run` gains `--replay-resolutions` awareness as a side effect, because it now runs the same code that does the replay.

- [x] **Step 1: Replace the dry-run preflight with a real composition**

`--dry-run` must do everything the real run does except publish: synchronize, pin, compose, report. It must **not** take the rebuild lock, must not create the environment branch, must not write any ref, and must not push.

```rust
if args.dry_run {
    let inputs = pin_environment_inputs(context, &base_branch, &promoted_branches)?;
    let result = compose_environment(
        context, &inputs, on_conflict, args.replay_resolutions,
        config.require_signed_resolutions, &mut |_| {},
    )?;
    // render from `result` — held, replayed, included
}
```

Factor the synchronize-and-pin block out of `rebuild_environment_opts` into a small `pin_environment_inputs(context, &base, &branches) -> Result<PinnedInputs>` helper, so the dry-run and the real run cannot drift in *how* they pin. That shared pinning is as important as the shared composition: a dry-run that pins differently is a dry-run that predicts something else, which is the failure mode this whole phase exists to remove.

- [x] **Step 2: Preserve the halt-policy refusal**

Under `OnConflict::Halt` with a conflict, `--dry-run` must still refuse (exit 1) and still print the same report. `compose_environment` already returns that `Err` — it halts inside the loop — so the dry-run path gets the correct behaviour for free, from the same code that produces it at execution time. The `format_compatibility_report_for_rebuild` message is currently built from a preflight conflict list; it now has to be built from the `Err` that `compose_environment` returns. Keep the user-facing wording identical, including the `git checkout <branch> && git rebase <other>` remedy lines that `AGENTS.md`'s error convention requires.

- [x] **Step 3: Remove the now-redundant pre-check**

`src/commands/rebuild.rs:120-128` skips the halt pre-check when `--replay-resolutions` is set, precisely because the preflight cannot see resolutions. With the dry-run path composed from real code, the dry-run branch returns before reaching it, and the remaining real-run halt pre-check at 120-128 is now a duplicate second opinion computed by the *other* implementation. Remove it and let `compose_environment`'s in-loop halt be the single decision — which is also what the existing comment there already argues for ("a single source of truth, rather than a second, separately-timed check that could in principle disagree with the real merge").

Record in this plan that `preflight_compatibility_report`'s remaining callers are the read-only display paths. If that leaves it with no callers at all, leave the function in place but mark it `#[allow(dead_code)]` with a comment saying it exists for the offline display preflight — do not delete it as part of this task; `hitch conflicts` and `hitch status` are its real consumers and re-plumbing those is P7's job.

- [x] **Step 4: Update the dry-run messaging**

The current dry-run output says `'dev' would rebuild cleanly (3 branches)` / `would rebuild with 2 of 3 branches (1 held)`. Keep that shape — it is already good, semantic, and matches what the source spec §2.2 asks for. Add a line when a resolution was replayed, because that is now visible in preview for the first time and it is the whole point of the fix:

```text
♻️ 1 branch would be composed from a recorded resolution: dashboard
```

- [x] **Step 5: Run the suite**

`just test`. `rebuild_tests::test_hitch_rebuild_dry_run_reports_held_branch` must still pass **unedited** — it is the direct check that this task preserved behaviour rather than changing it.

---

### Task 4: Prove the dry-run and the real build agree

**Files:**
- Test: `tests/integration/rebuild_tests.rs`

**Interfaces:**
- Consumes: `rebuild --dry-run` and `rebuild` (both now sharing one composition).
- Produces: no new production API.

- [x] **Step 1: Write the test**

For an environment containing a conflicting branch, assert the dry-run's reported hold set equals the real build's actual hold set, read from the published environment's provenance-free state — the branch set in the metadata, and the fact that the environment tip's tree does **not** contain the held branch's file.

```rust
#[test]
fn test_dry_run_and_rebuild_agree_on_held_branches() -> anyhow::Result<()> {
    // ... two branches conflicting on the same file, OnConflict::Eject ...
    let dry = env.hitch().output(&["rebuild", "dev", "--dry-run"])?;
    assert!(dry.stdout.contains("held"), "dry-run should report a hold: {}", dry.stdout);

    env.hitch().run(&["rebuild", "dev"])?;

    // The held branch's content must be absent from the published tree.
    let file = env.git().show("dev:conflict.txt")?;
    assert!(!file.contains("feature-b"), "held branch must not be in the build");
    Ok(())
}
```

- [x] **Step 2: Add the replay-agreement case, which is the regression this phase exists to prevent**

Set up a recorded resolution (mirroring `resolve_tests.rs`'s `setup_and_record` helper), then assert that `rebuild dev --dry-run --replay-resolutions` reports **no holds**, and that the subsequent real `rebuild dev --replay-resolutions` likewise publishes with none.

```rust
#[test]
fn test_dry_run_with_replay_reports_no_holds() -> anyhow::Result<()> {
    // ... record a resolution for the conflict (see resolve_tests::setup_and_record) ...
    let dry = env.hitch().output(&["rebuild", "dev", "--dry-run", "--replay-resolutions"])?;
    assert!(!dry.stdout.contains("held"),
        "--replay-resolutions preview must not report a hold that replay resolves: {}", dry.stdout);
    assert!(dry.stdout.contains("recorded resolution"),
        "preview should say a resolution would be applied: {}", dry.stdout);
    Ok(())
}
```

**This test fails against the pre-P1 code.** That is the point: it is the executable form of the bug fixed in Task 3. Confirm it fails on `main` before applying the fix (stash the Task 3 change, run it, watch it fail, restore) — a regression test that never failed is not evidence of anything.

- [x] **Step 3: Run both**

`just test-file rebuild`. Both pass.

---

### Task 5: Manual end-to-end check and `AGENTS.md`

**Files:**
- Modify: `AGENTS.md` (architecture map + a new gotcha)
- Modify: this plan (record the manual check's result)

**Interfaces:**
- Consumes: everything above.
- Produces: the documented invariant that keeps a second implementation from being reintroduced.

- [x] **Step 1: Build and exercise the binary by hand**

`just build`, then drive a throwaway repo in `/tmp` — not the integration harness — through three cases, because `AGENTS.md` requires a real end-to-end check for user-visible changes and this phase changes what a user sees:

1. A clean environment. Confirm the `[n/N]` progress numbering is unchanged from before the extraction (it should be — `total_steps` arithmetic stayed in the caller).
2. An environment with a conflicting branch. Confirm the hold warning text is byte-identical to the pre-change output, and that `hitch rebuild dev --dry-run` reports the same hold *before* anything is published.
3. The same environment with a recorded resolution. Confirm the dry-run now mentions the replay and reports no holds — the behaviour that did not exist before this phase.

Capture the actual terminal output in this plan. If any of the three surprises you, that is a real finding; do not paper over it.

- [x] **Step 2: Document the invariant in `AGENTS.md`**

Add to the architecture map:

- `src/utils/prelude.rs` — note that `compose_environment` is the single composition entry point, and that `preflight_compatibility_report` is a *display-only* offline approximation that must never gate a mutation.

And add a gotcha entry, in the style of the existing ones:

> **`preflight_compatibility_report` is not a second opinion on a real build.** It is a tree-based, offline approximation used only by the read-only display paths (`hitch conflicts`, `hitch status`) that must stay fast and never fetch. `hitch rebuild <env> --dry-run` used to call it, which meant the preview was computed by different code than the build: it could not see recorded resolutions (so `--dry-run --replay-resolutions` reported holds that replay would resolve), and it entered the merge engine through `merge_tree_write_tree_name_only` with an explicit `--merge-base` rather than `merge_tree_compose`'s ORT. Both paths now call `compose_environment`. If you add a new mutation or preview, route it through `compose_environment` — a second composition implementation is exactly how the preview came to disagree with the thing it was previewing.

- [x] **Step 3: Record the `refs/hitch/build/` anchor ordering**

Note in `AGENTS.md`'s composition section that the composed commit is unreachable until the publish CAS lands, so it is anchored at `refs/hitch/build/<env>/<ts>` for that window and dropped only after publish is attempted. This is pre-existing behaviour that Task 1 preserves, but P2 will add a second ref into the same transaction, and the ordering constraint is easier to honour when it is written down. (`AGENTS.md` already records this under Conventions; confirm it is still accurate rather than duplicating it.)

- [x] **Step 4: Verify all four gates**

`just format`, then `just format-check && just lint`, then `just test`.

---

## Implementation status: COMPLETE

All 32 steps and all 11 exit criteria are met. `just format`,
`just format-check`, `just lint`, `just test` all clean; test counts went
354 → 357 in the `mod` target and 60 → 61 in the lib target (three new tests,
zero existing tests edited). Gates last run green after every step below.

### Deviations from the plan as authored

Five, all forced by something the plan did not anticipate. Each was a
correction, not a shortcut.

1. **`pin_environment_inputs` takes `&Environment` + a `synchronize: bool`,
   not `(&base, &branches)`.** As authored it would re-read the config, giving
   `rebuild_environment_opts` a second metadata transaction under the lock and
   a window in which two reads could disagree — and, worse, it would have left
   the real build's existing inline pinning in place beside it, recreating the
   two-implementations problem one level down. One routine, one read.

2. **`synchronize: false` for `--dry-run`, contradicting this plan's own
   Task 3 Step 1.** That step said the dry-run must synchronize, and also that
   it "must not write any ref". Those are incompatible:
   `synchronize_branches` runs `fetch_all_remotes`, fast-forwards every local
   branch, and creates remote-only ones. A preview that synchronized would
   move the user's branches, against the repo's strongest convention. Chose
   the ref guarantee; the residual disagreement is that a preview sees current
   *local* refs while the build syncs first. That is ordinary staleness, not
   the two-merge-engines bug this phase exists to remove, and the reasoning is
   recorded at the call site and in `AGENTS.md`.

3. **The halt formatter had to move into `prelude.rs`, and the halt message
   changed source.** Task 3 Steps 2–3 assumed the in-loop halt and the pre-check
   agreed. They did not: the pre-check emitted
   `format_compatibility_report_for_rebuild`, the loop emitted
   `format_conflict_report`, and only `--replay-resolutions` (which skipped the
   pre-check) ever reached the second. So a halt printed *two different reports
   depending on an unrelated flag*. Unified on the former, which is why it had
   to move into `prelude.rs` — the decider must render its own refusal.
   `format_conflict_report` lost its last production caller; kept, public and
   tested, documented as a deletion candidate.

4. **`rebuild_environment_opts` gained `on_conflict_override: Option<OnConflict>`,
   which fixed a latent bug.** Removing the pre-check revealed that
   `--on-conflict halt` had *never reached the composition* — the loop read
   `environment.on_conflict` from config and there was no parameter for the
   override. The flag only worked because the duplicate intercepted it. Now
   threaded explicitly; `None` = "use the environment's policy", which is every
   other caller. Recorded as a gotcha, because the general form is worth
   remembering: a flag observable only through a check that runs *instead of*
   the real operation is not wired to the real operation.

5. **Task 2's purity test is a unit test in `prelude.rs`, not the CLI-level
   integration test the plan specified.** `compose_environment` is `pub`, and
   the property that actually matters — *no ref moved* — is not observable
   through the CLI at all: a dry-run that moved a ref would be a bug the
   integration test simply could not see. The direct test snapshots every ref
   before and after, plus HEAD and `git status`. The CLI-level determinism check
   the plan asked for is real but weaker, and the agreement it wanted is
   covered by Task 4's two tests instead.

### Findings

- **`hitch resolve` still gates on `preflight_compatibility_report`**
  (`commands/resolve.rs:131` and `:180`), using it to choose Mode A versus
  Mode B and to refuse when it reports no conflict. So the phase's central
  invariant is true for `rebuild` and still *violated* for `resolve`. Out of
  P1's scope (the plan scoped it to the dry-run), and re-plumbing `resolve`'s
  mode selection needs a plan to choose from, which is P4's shape. Recorded in
  `AGENTS.md` and on `compose_environment` so nobody adds a second dependant.
  This corrects the plan's Task 3 Step 3 expectation that only read-only
  display paths would remain.
- **Two pre-existing user-facing pluralization bugs**, found by reading real
  output during Task 5 and fixed on sight. `'dev' would rebuild cleanly
  (2 branchs)` formatted `"{} branch{}"` with a `""`/`"s"` suffix, and
  `Checking compatibility of 1 promoted branches` likewise. The sibling
  `""`/`"es"` sites were already correct, which is why nothing caught it. No
  test covered either wording.
- **The first draft of the Task 4 eject-agreement test compared rendered prose
  and failed on the old code for the wrong reason** — the real build prints
  both compose's per-branch `⛔ Held …` warning and the summary report, the old
  dry-run printed only the latter. Comparing lines would have flagged cosmetics
  while staying blind to a real verdict divergence. Rewritten to compare the
  verdict (which branch, held against whom, how many), then re-verified by
  injecting a deliberate divergence (dropping the last branch from the
  preview's inputs) and confirming it fails.

### Task 5 manual check (throwaway repo, debug binary)

`cargo build -p hitch` (debug, per the abort-hook note) driven against a
hand-built repo in a temp dir. `[n/N]` numbering unchanged
(`[1/4] Synchronizing branches`, `[2/4] Merging 'branch-a'`, `[3/4] Merging
'branch-b'`). Observed:

| case | command | result |
|---|---|---|
| clean, 2 branches | `rebuild dev --dry-run` | `would rebuild cleanly (2 branches)`, exit 0 |
| clean, 1 branch | `rebuild dev --dry-run` | `would rebuild cleanly (1 branch)`, exit 0 |
| conflict, eject | `rebuild dev --dry-run` | `⛔ Held 'branch-b' — conflicts with 'branch-a' (1 file)`, `1 of 2 branches (1 held)`, exit 2 |
| same, real build | `rebuild dev` | identical hold and count, exit 2, `dev` published |
| conflict, halt | `rebuild dev --dry-run --on-conflict halt` | `✗ Cannot rebuild 'dev' — compatibility check failed` + `git checkout branch-b && git rebase branch-a`, exit 1 |
| same, real build | `rebuild dev --on-conflict halt` | **identical** error text, exit 1, `dev` not created — confirms the override is now honoured by the composition and not just the removed pre-check |
| side effects | `--dry-run` after a real build | `for-each-ref` byte-identical before/after; `dev` tip unmoved; no `refs/hitch/build/*` created |

The halt row is the one that was previously impossible: dry-run and real build
now produce byte-identical refusals from the same code.

## Exit criteria

- [x] `compose_environment` exists, takes pinned inputs, and is the only composition performed by any mutation or preview **of `rebuild`**.
- [x] `rebuild_environment_opts` is pin → compose → anchor → publish, with the publish half byte-identical to before.
- [x] All 354 pre-existing tests pass **unedited**.
- [x] Composition is proven pure: same pinned inputs → same tree OID, same hold set, and no ref anywhere moved.
- [x] `rebuild --dry-run` runs the real composition, including resolution replay.
- [x] The `--replay-resolutions` preview regression is captured as a test that was confirmed to fail before the fix.
- [ ] `preflight_compatibility_report` has no mutation depending on it. **NOT MET** — `commands/resolve.rs:131,180` still uses it for Mode A/B selection and for refusing outright. Deliberately deferred to P4's planner and recorded in `AGENTS.md`; see Findings.
- [x] `AGENTS.md` records the invariant and the `refs/hitch/build/` anchor ordering.
- [x] Manual end-to-end check done against a throwaway repo, output captured in this plan.
- [x] `just format`, `just format-check`, `just lint`, `just test` all pass.

## Handoff to P2

P2 writes `EnvironmentBuildRecord` inside the publish transaction. It needs, from this phase:

- `CompositionResult.included` / `.held` / `.replayed` — the record's content.
- `PinnedInputs.base_sha` / `.branches` — the record's pinned source SHAs.
- Confidence that a composition's result is a pure function of its inputs, so the record describes a reproducible build.
