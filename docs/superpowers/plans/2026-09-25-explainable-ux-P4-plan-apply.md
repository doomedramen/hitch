# P4 — Rebuild plan / apply, end to end

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking. All 44 boxes are ticked; see "As executed" at the end for what actually landed, and "What P5 inherits" for what is deliberately still open.

**Goal:** Prove the plan → validate → apply → receipt architecture on exactly one operation, so four later operations have a shape to copy rather than invent.

**Architecture:** The planner is P1's `compose_environment` plus a summary of what publish will do. The executor is P2's atomic publish, with a freshness check in front of it. `hitch rebuild` becomes plan → (preview | apply) → validate fingerprint → apply → receipt.

**Tech Stack:** Rust, `anyhow`, `serde`, `thiserror`, `chrono`, `GitOperations` plumbing (including `hash_object_bytes` for the fingerprint digest), `HitchTestFramework`. **No new dependencies.**

**Spec section:** §2.3 (one source of truth for planning), §2.4 (staleness), §7.1–§7.4 (plan / composition / effects / fingerprint), §8 (receipts), §9 (planner/executor separation), §10.2 (`--yes`), §10.4 (dry-run semantics), §32 (remote races), §33 (error design), M3, M4, §30.3 (plan-vs-apply invariants).

---

## Global Constraints

- **`file:lines` refs below were resolved against `05f7126` on `explainable-ux`.** Re-resolve if the tree has moved.
- **Scope every cargo invocation to `-p hitch`.** A bare `cargo build`/`check`/`clippy`/`test` compiles the whole workspace including the broken `hitch-desktop` crate.
- **P4 changes no merge or publish behaviour.** `compose_environment` (`src/utils/prelude.rs:634`), `pin_environment_inputs` (`:777`), and `publish_branch` (`:1303`) keep their contracts. The differential test `test_merge_tree_compose_matches_real_merge_across_scenarios` and the four `HITCH_TEST_ABORT_AFTER` crash-fuzz suites are the oracle; if one of them needs editing to make this refactor pass, the refactor is wrong.
- **`rebuild_environment_opts` (`src/utils/prelude.rs:919`) must keep working for its other four callers** — `promote`, `demote`, `approve`, and `release`'s post-release rebuild. P4 makes it a thin plan-then-apply wrapper; it does not remove it. P5 is what rewires those four commands to show a plan.
- **The composed commit is named by the plan and landed by the apply.** `compose_environment` stamps `commit-tree` with wall-clock time, so re-composing at apply time yields a *different* SHA. The plan must therefore carry the commit it composed, anchored under `refs/hitch/build/*` for the window between planning and publishing, and the apply must land *that* commit. This is the same anchor `rebuild_environment_opts` already creates (`src/utils/prelude.rs:1050`), moved earlier in the sequence — not a new mechanism.
- **A preview must not anchor, must not lock, and must not synchronise.** Those three are the properties that make `--dry-run` safe, and P1 established the third deliberately (a preview that fetched would move the user's branches). `PlanPurpose` encodes all three so a future caller cannot opt into one accidentally.
- **Never sort a promoted branch list.** `PinnedInputs.branches` order is composition order and is load-bearing (master plan Constraint 8).
- **`ResolutionUse` keys are content hashes**, so a plan that replays resolutions can fingerprint the keys alone: a key match *is* an identical-content match. Do not put resolution blobs in the fingerprint.
- **A fingerprint gates staleness; it does not authorise.** A plan is core-generated. Nothing in this phase accepts a caller-authored plan or a caller-supplied `RefEdit` (master plan Constraint 9).
- **The rebuild exit-code-2 contract survives.** `rebuild::run` keeps returning `Ok(true)` for "applied with holds" and `main.rs` keeps turning that into exit 2. `OperationOutcome::AppliedWithHolds` is a *typed* expression of the same fact, never a substitute for it.
- **`EnvironmentBuildRecord` still rides the same `ref_transaction` as the branch move** (`src/utils/prelude.rs:1041-1045` builds the `RefEdit`; `publish_branch` extends the batch with it at `:1418`). Do not move it to a second transaction.
- **Manual end-to-end check for user-visible changes** — build the binary and exercise it in `/tmp` (master plan Constraint 12).

---

## What P4 is *not*

**P4 does not add an interactive confirmation prompt.** Master plan P4's arrow reads `plan → (confirm | --dry-run | --yes) → …`, but the prompt is a UX decision and P6 owns the CLI's presentation. What P4 makes real is the part that matters for correctness: a plan is a value, a validator exists, and `apply_rebuild_plan(plan)` refuses a stale one. A test can therefore plan, mutate the repository, and apply — which is exactly the spec M4 test, and it is the behaviour the fingerprint exists to provide. P6 adds the prompt that makes a human see it. `hitch rebuild`'s existing push confirmation is untouched.

**P4 does not render a pretty plan.** P6 builds the §10.1 layout. P4 keeps `hitch rebuild`'s current human output essentially unchanged and changes only its *data path*, so that P6 is a rendering change rather than a plumbing change. The one visible difference is the dry-run, which now renders from the plan instead of calling `compose_environment` itself.

---

## Deviations from the spec, and why

1. **`resulting_state` is a `RepositoryStateSnapshot`, not a new `StateSummary`.** Spec §8's `StateSummary` is unspecified, and master plan Constraint 2 forbids a second state model. P3 already built exactly this type, so the receipt reuses it. Same for the plan's `current`/`proposed`: rather than a generic `StateSummary`, they are `EnvironmentProjection`, a per-environment view with only the fields a rebuild plan actually predicts (base, branches-in-order with their SHAs, and the environment branch tip). That is the spec's §5.1 vocabulary narrowed to one operation, not a parallel hierarchy.
2. **`OperationKind` starts with one variant per *migrated* operation, not all eleven.** The spec lists eleven. Declaring eleven now means eleven variants with no planners behind them, and the first thing a future agent would do is add a `todo!()`. P4 declares `Rebuild`; P5 adds `Promote`/`Demote`/`Release`; the metadata-only ones arrive with P8. The *kind* is what a receipt records, so a receipt can always say what it was.
3. **`PlanFingerprint::digest` is a git object hash over a canonical byte encoding**, not a serde round-trip and not a new `sha2` dependency. `serde_json` output is not guaranteed stable across versions in a way that would make "same inputs → same digest" a property a test can rely on, and the fingerprint's whole job is to be compared for equality. A hand-rolled canonical encoding (length-prefixed `key\0value` pairs in a fixed order) is deterministic by construction. Hashing it with `GitOperations::hash_object_bytes` — the same mechanism `resolutions::resolution_key` already uses — keeps the dependency list unchanged and yields a normal 40-char object id. (This is a change from the plan's first draft, which specified `sha2`; `Cargo.toml` has no `sha2` and the existing content-addressing helper is the better precedent.)
4. **`OperationIntent` is a small structured enum, not a prose string.** Spec §7.1 lists the field without defining it, and §10.1's header line (`Promote feature/login → dev`) is *derived* from it. A string would push that derivation into every renderer, which is precisely the "one source of truth for planning" rule §2.3 exists to enforce.

---

## Tasks

### Task 1 — The plan and receipt model

**Files:** `src/operations/mod.rs` (new), `src/operations/model.rs` (new), `src/lib.rs`

**Interfaces:**
- Produces: `OperationPlan<I>`, `OperationKind`, `OperationIntent`, `PlanFingerprint`, `EnvironmentProjection`, `CompositionPlan`, `PlannedBranch`, `PlannedBranchState`, `PlannedEffect`, `UnaffectedResource`, `PlanWarning`, `ConfirmationRequirement`.
- Produces: `ExecutionReceipt`, `OperationOutcome`, `AppliedEffect`, `ExecutionWarning`, `PlanApplyError`.
- Consumes: `crate::core::state::RepositoryStateSnapshot`, `crate::utils::build_record::PinnedBranch`.

- [x] `src/operations/mod.rs` with `pub mod model;` and `pub mod rebuild;` (the latter added in Task 2). Register `pub mod operations;` in `src/lib.rs`, next to `core`.
- [x] Write the module header. It must say, in the words a future agent needs: a plan is an *immutable description of a decision already made* (the merges are already computed by the time a plan exists — the objects are in the object database), not a recipe to be re-evaluated; therefore a plan that is re-computed can differ from the one the user read, which is exactly why the fingerprint exists; and a receipt is a *record of what happened*, never a prediction.
- [x] `OperationKind { Rebuild }` with a doc comment stating that the variant set grows as operations migrate, and that a receipt's `operation` is the answer to "what was this" — so an unmigrated operation is simply absent, never mislabelled.
- [x] `OperationIntent::RebuildEnvironment { environment: String }` — deliberately the only variant. A human sentence derived from this is the renderer's job (P6); the model carries the fact.
- [x] `EnvironmentProjection { environment, base, branches: Vec<PinnedBranch>, branch_sha: Option<String> }`. `branches` keeps declaration order; `branch_sha: None` means the environment branch does not exist yet, which is a real state (first build) and not an error.
- [x] `PlannedBranchState { Included, Held { conflicts_with, files }, ReplayedResolution { resolution_id }, AlreadyInBase, Missing }`, per §7.2. Doc-comment that `AlreadyInBase` means the merge produced no tree change, which is the case where `compose_environment` does not create a commit — and that including it as a distinct state is what stops a plan from claiming N branches landed when only N−1 commits exist.
- [x] `CompositionPlan { environment, base: PinnedBranch, branches: Vec<PlannedBranch>, result_sha: String, holds: Vec<CompatibilityConflict> }` plus `pub fn included(&self) -> impl Iterator<Item = &str>` and `pub fn held(&self) -> Vec<&str>` so callers stop re-filtering `included` by hand. Every existing call site that reconstructs this list is a chance to reintroduce the "record drops a branch" bug from `src/utils/prelude.rs:1000-1005`; centralise it once.
- [x] `PlannedEffect` with exactly the three variants rebuild can produce: `MetadataChange { refname, description }`, `LocalRefUpdate { refname, old: Option<String>, new: String }`, `RemoteRefUpdate { refname, old: Option<String>, new: String }`. Doc-comment that the variant set is per-operation and that a missing variant means "this operation cannot cause that effect" — the struct is the allow-list a receipt is checked against.
- [x] `UnaffectedResource { kind, name }` with `kind: Branch | Environment | Remote`. The plan fills it from the *same* pinned inputs the composition consumed, so "will not change" cannot name a branch the plan did not actually read.
- [x] `PlanWarning { message, blocking: bool }`. Doc-comment that `blocking` means "the plan refuses to apply as written" (a halt-policy conflict), and that a non-blocking warning is a fact the user should see but that does not stop the operation (a held branch).
- [x] `ConfirmationRequirement { required: bool, reason: Option<String> }`. For P4 this is always derived from whether the push will be attempted; it exists so P6 can ask without re-deriving the condition.
- [x] `OperationPlan<I> { id, kind, intent, fingerprint, current, proposed, compositions, effects, unaffected, warnings, confirmation, detail: I }`. Generic over the per-operation detail (`RebuildPlanDetail` in Task 2) so shared code can carry any operation's plan without knowing its shape — and so `Vec<OperationPlan<I>>` stays homogeneous. Doc-comment that `id` is an *identity* for logs and correlation and is **not** a freshness proof (spec §7.4: "Do not treat the plan ID alone as proof of freshness").
- [x] `PlanFingerprint { metadata_sha: Option<String>, refs: BTreeMap<String, String>, remote_refs: BTreeMap<String, Option<String>>, resolution_keys: Vec<String> }` plus `fn digest(&self, git: &GitOperations) -> Result<String>`. `resolution_keys` is sorted and deduped on construction so two replays of the same set in different order fingerprint identically.
- [x] `fn digest`: canonical encoding, then `git.hash_object_bytes(&canonical)` — lowercase hex, 40 chars, same mechanism as `resolutions::resolution_key` (`src/utils/resolutions.rs:142`). Canonical form: `metadata_sha` first, then `refs` and `remote_refs` in that fixed order (both `BTreeMap`, so already ordered), then `resolution_keys` in order; each entry is `len(key)` `\0` `key` `\0` `len(value)` `\0` `value` `\n`, with an `Option::None` value encoding as the literal `-`. Write the unit test for it in this step: two fingerprints differing in any one component produce different digests, and a `None` does not collide with the string `-`.
- [x] `ExecutionReceipt { plan_id, operation, started_at, completed_at, outcome, effects, warnings, resulting_state: Option<RepositoryStateSnapshot> }` and `OperationOutcome { Applied, AppliedWithHolds, ApprovalRequested, NoChange }` per §8. Doc-comment that `AppliedWithHolds` must never be collapsed into `Applied` and that `NoChange` is a *successful* outcome, not a degenerate one.
- [x] `AppliedEffect` mirrors `PlannedEffect` but with the value that actually happened (`LocalRefUpdate { refname, old, new }` with `new` read back from the ref *after* the transaction, not remembered from the plan). That distinction is the receipt's entire value: a predicted SHA and an observed SHA are different claims.
- [x] `PlanApplyError` with the five §33 variants: `StalePlan { changed: Vec<ChangedInput> }`, `PolicyBlocked { reason }`, `Conflict { conflict: CompatibilityConflict }`, `PublishRace { branch, detail }`, `RemotePushFailed { branch, remedy, detail }`. `thiserror` is already a dependency (`Cargo.toml`), so derive `std::error::Error` and write each variant's `#[error(...)]` message by hand — there is no derive that can append a command line.
- [x] Write a unit test for every variant's `Display`, asserting each message **ends with** the exact command to run next (`hitch rebuild <env>`, or `hitch rebuild <env> --force` for `PolicyBlocked`). This is the one place the "render human guidance at the edge" rule from §33 has to be honoured today, because P6 is what moves it out to the renderer — so the test is the guarantee that the move cannot silently drop it.
- [x] `cargo build -p hitch` clean.

---

### Task 2 — The planner: `plan_rebuild`

**Files:** `src/operations/rebuild.rs` (new), `src/utils/prelude.rs`

**Interfaces:**
- Produces: `PlanPurpose { Confirm, Preview }`, `RebuildPlanDetail`, `plan_rebuild(context, env_name, on_conflict_override, replay, purpose) -> Result<OperationPlan<RebuildPlanDetail>>`, `discard_plan(context, plan) -> Result<()>`.
- Consumes: `pin_environment_inputs`, `compose_environment`, `access_metadata_read_only`, `crate::utils::build_record`.

- [x] `PlanPurpose` with a doc comment carrying the whole reason it is an enum and not two `bool`s: `Preview` implies `synchronize: false` (P1's deliberate asymmetry — a preview that fetched would move the user's branches), no environment lock, and no anchor ref. `Confirm` implies all three. Encoding purpose rather than knobs is what makes "a preview that mutates" unrepresentable.
- [x] `RebuildPlanDetail { environment, result_sha, held: Vec<CompatibilityConflict>, replayed: Vec<ResolutionUse>, anchor_ref: Option<String>, backup_timestamp: String, remote_env_sha_before: Option<String>, record: EnvironmentBuildRecord, state_edit: RefEdit }`. `state_edit` and `anchor_ref` are the plan's *own* ref operations, computed once at plan time and applied verbatim at apply time — recomputing them at apply time would be a second decision point, and the build record is a claim about *this* composition.
- [x] `plan_rebuild` signature exactly as above. It must NOT take `&RebuildCommand` or any clap type — planner and command layer stay separable (spec §9: "Command modules should become thin argument adapters").
- [x] Inside `plan_rebuild`, in this order: (a) `pin_environment_inputs(context, &environment, matches!(purpose, Confirm))`; (b) read `refs/remotes/origin/<env>` **before** composing, so the push leases against what was observed rather than against whatever the remote is when the build finishes — this is `remote_env_sha_before` at `src/utils/prelude.rs:966-968` today, and moving it into the plan is what makes the lease part of the plan rather than a side observation; (c) `compose_environment(...)` with a progress callback; (d) build the `EnvironmentBuildRecord` and its `RefEdit` exactly as `src/utils/prelude.rs:1006-1045` does — same construction, same unconditional-overwrite `expected_old: Some(String::new())`; (e) create the `refs/hitch/build/<env>/<timestamp>` anchor **only for `Confirm`**; (f) assemble the fingerprint.
- [x] Fingerprint assembly, and this is the load-bearing part: `metadata_sha` from `resolve_metadata_sha`; `refs` containing the base branch, every promoted branch, and `refs/heads/<env>` (absent → omitted, not a sentinel — an absent entry and an entry whose value is `None` are different facts, and `remote_refs`' `Option` value is what distinguishes them); `remote_refs` containing `refs/remotes/origin/<env>`; `resolution_keys` from `detail.replayed`, sorted and deduped.
- [x] `CompositionPlan` assembly: one `PlannedBranch` per entry in `pinned.branches`, **in order**, with the state derived from the `CompositionResult` — `Included` for `included`, `Held { conflicts_with, files }` for `held`, `ReplayedResolution { resolution_id }` for `replayed` (a replayed branch is *also* included, so check replay first and let one variant carry it; document that choice), `Missing` for any pinned branch absent from both lists, which cannot happen today but must not be silently rendered as `Included`.
- [x] `effects`: one `LocalRefUpdate` for `refs/heads/<env>` (old = the observed tip, new = `result_sha`), one `MetadataChange` for the build record's ref, and one `MetadataChange` for the `rebuilt_at` stamp. One `RemoteRefUpdate` for `refs/remotes/origin/<env>` **only when `context.should_push()`** — a plan that predicts a push the command will not attempt is exactly the false "fully synced" §M4 warns about, so the condition is evaluated at plan time and the same condition is evaluated at apply time.
- [x] `warnings`: one non-blocking warning per held branch (a held branch is a fact the user must see, not a reason to refuse), plus one blocking warning when `on_conflict == Halt` and the composition held anything — in which case the planner returns the same `format_compatibility_report_for_rebuild` refusal text the composition path already produces, so a halt under a plan and a halt without one say the same words.
- [x] `discard_plan(context, plan)`: drop `anchor_ref` if present, then return. This is the declined-confirmation and preview cleanup. Doc-comment that a leaked anchor is harmless-but-untidy (a commit that stays reachable under `refs/hitch/build/*`, which nothing prunes — see the `build/` namespace in `commands/cleanup.rs`'s prunable set) and that the call must therefore happen on **every** non-apply path, which is why it is a named function rather than a `Drop` impl that cannot report failure.
- [x] Test first: a clean promotion produces a plan whose `composition.branches` lists every promoted branch in declaration order, whose `effects` name `refs/heads/dev`, and whose `fingerprint.digest()` is stable across two runs against an unchanged repository.
- [x] Test first: a `Preview` plan anchors nothing (`git rev-parse --verify refs/hitch/build/...` fails) and does not move `refs/heads/*`, `refs/remotes/*`, or any worktree — snapshot every ref before and after, the same technique as `compose_environment_is_pure_and_deterministic`.
- [x] Run the tests.

---

### Task 3 — Fingerprint validation

**Files:** `src/operations/rebuild.rs`

**Interfaces:**
- Produces: `validate_plan(context, plan) -> Result<(), PlanApplyError>`.

- [x] `validate_plan` re-reads every component of the plan's fingerprint from the live repository and compares. On any difference, return `PlanApplyError::StalePlan { changed }` where `changed` is a `Vec<ChangedInput>` — **reuse `crate::core::state::ChangedInput`, do not define a new one** (master plan Constraint 2, and P3 already renders that type with a `→` arrow that P6 will reuse for exactly this message).
- [x] Comparison rule per component, written down on the function: `metadata_sha` compares as an `Option<String>` against a fresh `resolve_metadata_sha`; each `refs` entry compares against a fresh `rev_parse_opt` of that exact refname (so a *deleted* branch reads as `None` and is reported, rather than being skipped because the map lookup failed); each `remote_refs` entry likewise; `resolution_keys` compares as a sorted set, and a *missing* key is reported too — a resolution deleted between plan and apply means the replay would now miss, which is a materially different operation.
- [x] A ref in the live repository that is *not* in the plan's fingerprint is **not** a change. The fingerprint is a whitelist of what the plan depends on, not a snapshot of every ref. Doc-comment this, because the opposite rule is the intuitive one and would make every unrelated push in the repository refuse the apply.
- [x] `StalePlan`'s message must say what changed *and* what to do: per §2.4, "refuse stale plan / calculate a fresh plan / show what changed", and the remedy is to re-run the same command. It must not say the plan is invalid, corrupt, or an error in the user's repository.
- [x] Test first: plan, then commit to a promoted branch, then `validate_plan` → `StalePlan` naming that branch with old and new SHAs.
- [x] Test first: plan, then commit on `hitch-metadata` (e.g. promote into a *different* environment), then `validate_plan` → `StalePlan` naming `refs/heads/hitch-metadata`.
- [x] Test first: plan, then move `refs/heads/<env>` (another publish), then `validate_plan` → `StalePlan`.
- [x] Test first: plan, then `git push` to move `refs/remotes/origin/<env>`, then `validate_plan` → `StalePlan`.
- [x] Test first: plan, nothing changes, `validate_plan` → `Ok`.
- [x] Run the tests.

---

### Task 4 — Push outcome, so an owed push is representable

**Files:** `src/utils/prelude.rs`

**Interfaces:**
- Produces: `PushOutcome { NotAttempted, Declined, Pushed, Failed { error: String } }`, `PublishOutcome { push: PushOutcome, journal_cleared: bool }`.
- Changes: `publish_branch`'s `push` parameter from `impl FnOnce() -> Result<()>` to `impl FnOnce() -> Result<PushOutcome>`, and its return from `Result<()>` to `Result<PublishOutcome>`.

- [x] The problem this solves, stated on the type: `publish_branch` today logs a push failure and returns `Ok(())`, because a failed push is not fatal to an already-successful local publish. That is correct for the CLI and useless for a receipt — a receipt built from it would report "applied" and a reader would conclude everything is synced. §M4 requires the opposite: "push failure represented as warning/owed effect, not falsely 'fully synced'". The information exists at the call site and is currently thrown away; this is the minimum change that lets a receipt keep it.
- [x] `publish_environment_build`'s push closure (`src/utils/prelude.rs:1523-1558`) returns `PushOutcome::Declined` when the user says no, `Pushed` on success, and `Failed { error }` on the existing mapped error. **Its control flow is otherwise byte-for-byte the same** — the declined case must still `return Ok(())` to `publish_branch` so the journal record clears, and the failure case must still leave the record in place. Only the *reported* value changes.
- [x] `publish_branch`: `!context.should_push()` → `Ok(PublishOutcome { push: NotAttempted, journal_cleared: true })`. Push `Ok` → `{ Pushed, true }`. Push `Err` → `{ Failed { error }, false }` — and still `Ok(())`, preserving the existing "a push failure is not fatal" contract.
- [x] The two other callers (`hitch release`'s landing and `resolve`'s Mode A in `src/commands/resolve.rs`) must have their push closures return a `PushOutcome` too. `release`'s push is a plain fast-forward; if it currently returns `Ok(())` on success, return `Pushed`. If either caller's push is genuinely best-effort-without-distinction today, return `Pushed`/`Failed` on the same rule — do **not** collapse a real failure into `Pushed` to avoid touching the call site.
- [x] Test: a rebuild whose push fails leaves `journal_cleared == false` and `push == Failed`, with the journal record still present (`refs/hitch/publish/<env>` resolves). Reuse the existing crash-recovery fixtures rather than inventing a new remote setup.

---

### Task 5 — The executor: `apply_rebuild_plan`

**Files:** `src/operations/rebuild.rs`

**Interfaces:**
- Produces: `apply_rebuild_plan(context, plan, on_step) -> Result<ExecutionReceipt>`.
- Consumes: `validate_plan`, `publish_environment_build`, `update_rebuilt_timestamp_for_rebuild`, `crate::core::state::build_state_snapshot`.

- [x] `apply_rebuild_plan` in this order, and the order is the contract: (1) `validate_plan`; (2) build the `EnvironmentBuildRecord`'s `RefEdit` — *reuse `plan.detail.state_edit` verbatim*, never rebuild it; (3) `publish_environment_build(context, env, &plan.detail.result_sha, &[state_edit], &plan.detail.backup_timestamp, &plan.detail.remote_env_sha_before)`; (4) drop the anchor ref; (5) `update_rebuilt_timestamp_for_rebuild`; (6) assemble the receipt.
- [x] Step (4) must run even if step (3) failed, and the anchor must be dropped on **every** exit path including the error path — otherwise a failed publish leaks a reachable commit under `refs/hitch/build/*` forever. Implement it as a scope guard or an explicit drop before the `?`, and add a test that a publish failure leaves no anchor ref.
- [x] `AppliedEffect`s are read back from the repository, not copied from the plan: after (3), `rev_parse` `refs/heads/<env>` and assert it equals `result_sha`, and read `refs/remotes/origin/<env>` for the remote effect. If a read-back disagrees with the plan, that is a bug worth an error, not a silent correction — but note that the *push* legitimately changes `refs/remotes/origin/<env>` via `record_pushed_tip`, so a `RemoteRefUpdate` effect is only "applied" when the push actually happened.
- [x] `outcome`: `AppliedWithHolds` when `plan.detail.held` is non-empty, else `Applied`. `NoChange` is reachable when the plan's `result_sha` already equals the current tip — compute it by comparing at apply time, and make sure the dry-run/plan path does not claim a change in that case. `ApprovalRequested` is unused by rebuild; it exists for P5 and must be documented as such rather than left looking live.
- [x] `warnings`: copy the plan's non-blocking warnings forward (a held branch was a fact at plan time and is still a fact), and **add** a warning for `PushOutcome::Failed { error }` carrying the same remedy text `publish_branch` prints. This is the specific thing §M4's test is looking for.
- [x] `resulting_state`: `build_state_snapshot(context)?` after the publish and the timestamp write. It is one snapshot, not two — the plan never builds one.
- [x] `started_at`/`completed_at`: `chrono::Utc::now()` at entry and at exit. Doc-comment that these are wall-clock and therefore *presentation only* — the same rule `AGENTS.md` records for `rebuilt_at`, and a future agent must not read them for a verdict.
- [x] Test first: an unchanged plan applies and every `AppliedEffect` matches the plan's `PlannedEffect` of the same variant — the §30.3 invariant, stated as a comparison rather than as a re-derivation.
- [x] Test first: the receipt's `outcome` is `AppliedWithHolds` for a held build and `Applied` for a clean one, and `hitch rebuild` still exits 2 and 0 respectively.
- [x] Test first: with no remote, the receipt carries a `RemoteRefUpdate` *warning* and no `RemoteRefUpdate` effect, and the command exits 0 — a failed push is not a failed rebuild.
- [x] Run the tests.

---

### Task 6 — Rewire `hitch rebuild`

**Files:** `src/commands/rebuild.rs`, `src/utils/prelude.rs`

**Interfaces:**
- Changes: `rebuild_environment_opts` becomes a plan-then-apply wrapper preserving today's output.
- Changes: `commands::rebuild::run` drives `plan_rebuild` / `discard_plan` / `apply_rebuild_plan`.

- [x] `rebuild_environment_opts`: replace its body (currently `src/utils/prelude.rs:919-1083`) with `plan_rebuild(..., PlanPurpose::Confirm)` then `apply_rebuild_plan`, mapping the receipt back to today's `RebuildOutcome { held, replayed }` and preserving the `StepLogger` output shape **exactly** — the same "Rebuilding environment 'X'" header, the same "Synchronizing branches" / "Merging 'B'" / "Publishing 'X'" steps, the same `complete()`. Four commands call this and their tests assert on that output; a rendering change here is a breaking change for them, and P6 is where rendering changes.
- [x] That means the `StepLogger` is created by the *caller* and threaded into both the planner (for the sync and merge steps) and the executor (for the publish step), as a `&mut dyn FnMut(&str)`. The planner's callback must be invoked for the same step descriptions the old inline code produced, in the same order, or the existing tests' output assertions break visibly.
- [x] `commands::rebuild::run`: replace the `if args.dry_run` block (`src/commands/rebuild.rs:83-144`) with `plan_rebuild(..., Preview)` rendered from `plan.composition` and `plan.effects` instead of from a private `compose_environment` call. The exit-code contract is unchanged: `Ok(!held.is_empty())` for the preview, so a preview that *would* hold still exits 2.
- [x] The dry-run renderer must keep the `♻️ … recorded resolution` line and the "would rebuild cleanly" / "would rebuild with N of M branches" wording, because those are asserted by existing tests and because the test framework compares verdicts, not prose — but the *numbers* must now come from `plan.composition.included()` and `.held()`, never from a second filter over a different list.
- [x] The non-dry-run path: `plan_rebuild(..., Confirm)`, then `apply_rebuild_plan`. `--force` still bypasses `with_locked_env` and calls straight through, as today.
- [x] Keep `--pr-comments` exactly where it is — after the apply, using `receipt`'s held list. It is a GitHub side effect, not part of the plan, and pretending otherwise would put a network call inside planning.
- [x] Delete nothing from `rebuild.rs` that another caller uses. `format_held_report` is used by both the preview and the real path; it stays.
- [x] Test: every existing test in `tests/integration/rebuild_tests.rs`, `resolve_tests.rs`, and the four crash-recovery suites passes **unchanged**. If one needs editing, this step has changed behaviour and that is a finding, not a chore.

---

### Task 7 — The plan-vs-apply invariant tests

**Files:** `tests/integration/plan_apply_tests.rs` (new), `tests/integration/mod.rs`

- [x] New module registered in `tests/integration/mod.rs`.
- [x] **§30.3 invariant**, for each effect rebuild can produce: `plan predicts X` → `execute` → `receipt reports X` → `repository now equals X`. Four effects: metadata (build record), metadata (`rebuilt_at`), local ref (environment branch), remote ref (`origin/<env>`, against a bare-origin fixture placed as a **sibling** path, never inside the test repo).
- [x] **Staleness refusals** (spec M4): plan → mutate `hitch-metadata` → apply refused; plan → move a promoted branch → apply refused; plan → move the environment branch → apply refused. Each must assert the refusal *names the ref that changed*, not merely that it failed — a refusal that cannot say what changed forces the user to diff the repository themselves, which is the thing the fingerprint was for.
- [x] **Unchanged plan applies exactly the predicted effects**: assert the plan's `LocalRefUpdate.new` equals the post-apply `refs/heads/<env>` and equals the receipt's, so the three agree.
- [x] **Push failure is a warning with an owed effect**: `--no-push` and a failing remote; assert the receipt has a `RemoteRefUpdate` warning, no `RemoteRefUpdate` effect, and the journal record survives. Assert the command exits 0.
- [x] **`AppliedWithHolds` survives the exit-code contract**: a held rebuild exits 2, and the receipt's `outcome` is `AppliedWithHolds`; a clean rebuild exits 0 with `Applied`.
- [x] **Locks preserved**: a held environment still refuses without `--force`; `--force` still proceeds; the repo-wide lock still serialises two concurrent rebuilds (the existing concurrency test covers this — confirm it passes rather than writing a new one).
- [x] **Preview agrees with apply**: `--dry-run` and a real run report the same `included`/`held` sets from the same planner, in the same order. This is P1's invariant restated at the plan level; the existing verdict-agreement tests still apply and must pass unchanged.
- [x] **Non-vacuity**: temporarily make `validate_plan` return `Ok(())` unconditionally and confirm the three staleness-refusal tests fail. Revert. (A second probe — removing the read-back assertion in Task 5 — is optional; the first is the one that matters, because a validator that always passes is the exact failure mode this architecture exists to prevent.)
- [x] Run the whole suite.

---

### Task 8 — Docs

**Files:** `AGENTS.md`, `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`, this file

- [x] `AGENTS.md`: add `src/operations/` to the architecture map. Add gotchas for: the plan carries an already-composed commit, not a recipe (so re-planning can differ and that is why the fingerprint exists); `PlanPurpose` is an enum so a preview cannot mutate; `publish_branch` now returns a `PushOutcome` and why a receipt cannot be built from `Ok(())`; the fingerprint is a whitelist of what the plan depends on, not a snapshot of all refs; and that `rebuild`'s exit code 2 is now *also* expressible as `OperationOutcome::AppliedWithHolds` but the two must not be allowed to drift.
- [x] Master plan: mark P4 complete with exit criteria and deviations; extend "Where this work lives" with the commit; update the P5 section if any of its line references moved.
- [x] This file: fill in "As executed" — deviations, findings, non-vacuity, manual-check transcript — and write "What P5 inherits".
- [x] Manual check against a throwaway repo in `/tmp`: build `dev`, confirm a clean rebuild's output is unchanged from before this phase; confirm `--dry-run` still reports the same holds for a conflicting branch; confirm the recordless/`LegacyUnknown` path still renders; and confirm a real (push-enabled) rebuild against a bare origin still pushes.

---

## Exit criteria

- [x] Plan → mutate `hitch-metadata` → apply refused, naming the ref.
- [x] Plan → move a promoted branch ref → apply refused, naming the ref and both SHAs.
- [x] Unchanged plan applies exactly the predicted effects, and plan/receipt/repository agree on every one.
- [x] Receipt reports the *observed* published SHAs, read back after the transaction.
- [x] A push failure is a warning with an owed effect and a surviving journal record — never a false "fully synced" — and the command still exits 0.
- [x] `AppliedWithHolds` is preserved distinctly, and `hitch rebuild`'s exit code 2 still happens for held builds.
- [x] Repo lock, `RebuildLock`, and the `locked` metadata flag all still apply exactly as before.
- [x] `--dry-run` is a renderer over the same planner, and agrees with a real run.
- [x] `rebuild_environment_opts` still serves `promote`, `demote`, `approve`, and `release` with byte-identical output.
- [x] All four gates green; `git diff --name-only main..explainable-ux -- crates/` empty.

---

## As executed

**Status: COMPLETE.** All 8 tasks landed on `explainable-ux` as a single commit
after `05f7126`. The full suite is green (405 integration + lib, 1 in
`no_args_help`) and `just format-check`/`just lint` are clean.

### Deviations from the plan as written

Five, on top of the four already recorded in "Deviations from the spec" above:

5. **The anchor is released by one unconditional call, not per exit path.**
   The plan's Task 5 spelled out a numbered step ("3. drop the anchor — on
   **every** exit path, including the error path"). Implementing that literally
   means a cleanup line at each `return`/`?` in the function, and the first
   place that gets forgotten is the `?` on `validate_plan` — which fires before
   anything else has run, is invisible on a green run, and leaks a commit under
   `refs/hitch/build/*`, a family nothing prunes. The executor is therefore split
   into a thin `apply_rebuild_plan` that holds the inner call in a variable,
   calls `discard_plan` unconditionally, then returns the result — one cleanup
   path rather than N. The doc comment states this is a `finally`, not a step,
   precisely so a later reader doesn't "tidy" it back into a step.
6. **`PlanApplyError::into_anyhow` is a public constructor rather than an
   implicit `?` conversion.** `PlanApplyError` derives `std::error::Error`
   (thiserror, already in `Cargo.toml`), so `anyhow::Error::new(self)` keeps the
   typed error intact across the `anyhow::Result` boundary. That is what lets
   `err.downcast_ref::<PlanApplyError>()` still yield a `StalePlan` with its
   `changed` list after it has crossed — erasing it to a `String` would throw
   away the structure the model exists to carry.
   `a_plan_is_refused_once_the_declaration_moves` relies on exactly that.
7. **`RefEdit` gained `PartialEq, Eq`,** along with `RepositoryStateSnapshot` in
   P3. Not needed by the executor, but it is what lets a test assert that a
   plan's `state_edit` is structurally the one that landed rather than comparing
   three fields by hand.
8. **`validate_plan` is `pub`.** Not merely reachable: the staleness tests call
   it directly so a refusal can be asserted with the repository unmoved, which
   distinguishes "the validator said no" from "the validator said no and the
   apply then stopped." `apply_rebuild_plan` calls it too, so the direct call is
   not a second door into the verdict.
9. **`apply_rebuild_plan` does not write the `rebuilt_at` stamp, and emits no
   trailing step.** Both are inherited from `publish_environment_build` /
   `StepLogger` rather than added: the stamp is already inside the operation for
   `hitch resolve`'s Mode B and `rebuild_environment_opts`, and a second write
   would stamp the environment twice for one rebuild; and a synthetic "Done"
   step would inflate the step *count* that `logger.complete()` closes on, in
   output that four other commands' tests read.

### Findings

- **A plan can never report a halt.** `OnConflict::Halt` returns `Err` from
  *inside* `compose_environment` (P1), so by the time a plan exists the halt has
  already happened and the plan is never built. The observable behaviour is
  right — the operation refused rather than partially applying — but it means
  the `blocking: true` warning branch is **unreachable in P4** and was deleted
  as dead code, and `PlanApplyError::PolicyBlocked` likewise has no production
  caller from the rebuild apply path. Both are kept: `PlanWarning.blocking` is
  P5's approval machinery and `PolicyBlocked` is the typed form of the halt
  refusal P6 will want to render. A manual check confirms nothing observable
  moved: `--on-conflict halt` still exits 1 with the full `Cannot rebuild 'dev'
  — compatibility check failed` report and the `git checkout … && git rebase …`
  next step, printed exactly once.
- **A test cannot force a ref transaction to fail by colliding paths.**
  `a_publish_failure_leaves_no_anchor_behind` originally made
  `.git/refs/hitch/state/dev` a *directory*, on the reasonable theory that git
  could not write a loose ref file there. It can: git removes the empty
  directory and writes the ref anyway, so the transaction succeeded and the test
  asserted nothing. Verified directly against `git update-ref`, then replaced
  with a refname containing `..`, which is rejected outright with `fatal:
  invalid ref format` *before* any part of the batch runs — a certain failure
  rather than a likely one.
- **Two compositions of identical inputs get different commit SHAs.**
  `a_preview_and_a_confirm_plan_agree_about_every_branch` initially asserted the
  two plans' `result_sha` were equal, and failed. This is the documented
  `commit-tree` wall-clock behaviour (master plan Constraint 5 — the same trap
  the crash-fuzz convergence check sidesteps by comparing `^{tree}`), and it is
  the *reason* the plan carries its composed commit rather than recomputing one
  at apply time. The assertion is now on `^{tree}`, with a comment recording
  why, so the difference reads as the invariant it is rather than as a
  loosened test.

### Non-vacuity

`validate_plan` was temporarily short-circuited to `Ok(())` and the module
re-run. Exactly the three staleness tests failed —
`a_plan_is_refused_once_the_declaration_moves`,
`a_plan_is_refused_once_a_promoted_branch_moves_and_names_both_shas`, and
`a_plan_is_refused_once_the_environment_branch_moves` — each reporting a real
`Applied` receipt and `refs/heads/dev` moved to the *pre-mutation* composition.
Reverted; all 18 green.

`a_preview_anchors_nothing_and_moves_no_ref` is self-non-vacuous: it snapshots
every ref, asserts a `Preview` plan changed none of them *and* that
`anchor_ref == None`, then builds a `Confirm` plan in the same repo and asserts
its anchor ref *does* exist. The first half therefore cannot pass because
anchors are never written.

### Manual check

`cargo build -p hitch` (debug — a release build silently ignores the crash hook,
per the AGENTS.md note) against a throwaway repo in `/tmp/hitchp4/repo`:
`init`, `add dev`, two clean features promoted, then a third branch edited to
conflict.

- Clean rebuild: `[1/4] … Synchronizing / [2/4] … Merging 'feat-a' / [3/4] …
  Merging 'feat-b' / [4/4] … Publishing 'dev'`, then `✅ Environment 'dev'
  rebuilt successfully!` — unchanged from pre-P4, exit 0.
- `--dry-run`: `✅ 'dev' would rebuild cleanly (2 branches).`
- Held rebuild: exit **2**, with `⛔ 'dev': 1 branch held (excluded from this
  build)` and the `git checkout feat-conflict && git rebase feat-b` next step.
- `--on-conflict halt`: exit **1**, single report, unchanged.
- `git for-each-ref | grep refs/hitch/build` after every one of the above:
  **empty** — no anchor leaked, including after the halt refusal and after the
  held build.
- `hitch status` afterwards still renders the held branch as `⛔ feat-conflict
  (held in the last build — conflicts with feat-b)`, confirming P3's
  fact-vs-prediction wording survived the rewire.

---

## What P5 inherits

- **A planner and an executor to copy, and one operation that uses them.**
  `plan_rebuild(context, env, options, purpose, on_step) -> Result<Plan>` and
  `apply_rebuild_plan(context, &plan, on_step) -> Result<ExecutionReceipt>` in
  `src/operations/rebuild.rs`. P5's `promote`/`demote`/`release` planners take
  the same shape with their own `*PlanDetail`, reusing
  `PlanFingerprint`/`validate`/`assemble_receipt` as shared machinery.
- **`rebuild_environment_opts` is now a thin wrapper** that plans and applies
  inside `with_locked_env` and emits `StepLogger` steps. It still exists and
  still serves `promote`, `demote`, `approve`, and `release`'s post-release
  rebuild, byte-identically — **none of those four shows a plan yet**, which is
  exactly P5's job. Do not delete the wrapper before P5 rewires them.
- **`PlanPurpose` needs a second job.** `Confirm` and `Preview` currently differ
  only in synchronise/lock/anchor. P5 adds commands whose plans are cheaper to
  build than to re-verify, and may need a purpose that anchors but does not
  synchronise (a `promote` reading refs it has just fetched itself, say).
- **Three model members are declared but unproduced:**
  `PlanApplyError::PolicyBlocked`, `PlanWarning.blocking == true`, and
  `OperationOutcome::ApprovalRequested`. P5's approvals are the intended first
  producers of all three. Leave them in place.
- **`OperationKind` has one variant.** Adding `Promote`/`Demote`/`Release` is the
  first thing P5 should do, *before* any planner, so a receipt can never be
  ambiguous about what it was.
- **No interactive confirmation and no plan rendering.** `hitch rebuild`'s
  existing push confirmation is untouched and P4 added none. P6 owns both, and
  will find `OperationIntent` and `EnvironmentProjection` already built and
  waiting to be rendered.
