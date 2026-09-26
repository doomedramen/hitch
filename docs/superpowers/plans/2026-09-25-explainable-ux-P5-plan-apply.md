# P5 — Promote / demote / release, plan + apply

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the plan → validate → apply → receipt architecture to the three operations that carry extra structure, and give the model the three members it has been declaring without producers.

**Architecture:** Same shape as P4, one planner each. Promote and demote are the *same* operation on the declaration — change an environment's promoted-branch list, then rebuild what that invalidates — so they share one planner and one executor and differ only in `OperationKind`, `OperationIntent`, and the direction of the edit. Release is the most explicit plan in the product: it composes, tags, publishes, prunes, and rebuilds dependents, and a plan is what finally makes "all-or-nothing" a property of a value rather than of a control flow.

**Tech Stack:** Rust, `anyhow`, `serde`, `thiserror`, `chrono`, `GitOperations` plumbing, `HitchTestFramework`. **No new dependencies.**

**Spec section:** §2.3 (one source of truth for planning), §3 and §16 (release all-or-nothing), §5 (declaration/actual), §6 (approvals), §7 (plan), §8 (receipts), §9 (planner/executor separation), §30.3 (plan-vs-apply invariants), §32 (remote races), §33 (error design), M5 (promote/demote), M6 (release).

---

## Global Constraints

- **`file:lines` refs below were resolved against `313189d` on `explainable-ux`.** Re-resolve if the tree has moved.
- **Scope every cargo invocation to `-p hitch`.** A bare `cargo build`/`check`/`clippy`/`test` compiles the whole workspace including the broken `hitch-desktop` crate.
- **`crates/` is untouched.** Standing check at the end: `git diff --name-only main..HEAD -- crates/` is empty.
- **P5 changes no merge or publish behaviour.** `compose_environment`, `pin_environment_inputs`, `merge_tree_compose`, `commit_tree`, and `publish_branch` keep their contracts. The oracle is `test_merge_tree_compose_matches_real_merge_across_scenarios`; if a merge or publish test needs editing to make this refactor pass, the refactor is wrong. `create_release_tag`'s two collision tests (`src/commands/release.rs`) are release's own oracle and must not change.
- **The oracles that must stay green untouched:** `crash_recovery_tests.rs`, `release_crash_recovery_tests.rs`, `resolve_crash_recovery_tests.rs`. Release's crash-fuzz suite is the strongest single guard on the all-or-nothing claim, because it aborts at `journal-written`, `ref-moved`, and `resync-done` and asserts convergence against an oracle run.
- **Release keeps all-or-nothing semantics** (spec §3, §16). A conflict is decided inside the planner and returns `Err`; no tag, no ref move, no metadata edit, no prune. The prune decision is *also* made at plan time, and the plan reports it — a plan that will prune but does not say so is exactly the kind of plan this program exists to eliminate.
- **Every new planner owes an unconditional anchor release in a `finally`.** `refs/hitch/build/*` and `refs/hitch/release/*` are both unpruned live-leak families (`cleanup`'s prunable set is deliberately `["backup", "prev"]`). P4 learned this the hard way: the `?` on `validate_plan` is the exit path most likely to be missed and is invisible on a green run. Copy P4's structure literally — one `discard_plan`-shaped call wrapping the inner apply, not a numbered step.
- **A plan carries a decision, not a predicate.** `commit-tree` stamps wall-clock time, so re-composing at apply time yields a different SHA. Every composed commit in P5 (release's target merge chain) is composed at plan time, anchored, and landed verbatim.
- **`PlanPurpose` has exactly three suppressing properties: synchronise, lock, anchor.** Promote and demote have no composed commit to anchor and take no lock from the planner, so for them `PlanPurpose` exists *only* to be handed down to the nested rebuild. Say so at the call site; do not invent a fourth purpose to make it look load-bearing.
- **The nested rebuild is planned *after* the declaration edit lands**, not with the outer plan's fingerprint. The outer plan's `metadata_sha` is validated before the write; the inner plan is built against the new tip. Reusing the outer fingerprint for the inner plan would refuse every single promote.
- **Never sort a promoted branch list.** Declaration order is composition order and is load-bearing (master plan Constraint 8). The prune set *is* sorted today (`update_release_metadata_and_prune`) and must stay sorted, because it is a set of independent environments, not a composition order.
- **The approval gate is a plan-level fact, not a precondition.** The planner reports it as a blocking warning; the executor creates the approval requests. Request creation is a metadata mutation and must not happen inside a planner.
- **Preserve exit codes.** Approval requested → 0. Policy refusal → 1. Release conflict → 1. `hitch rebuild`'s exit-2 contract is untouched.
- **Manual end-to-end check for user-visible changes** — build the binary and exercise it in `/tmp` (master plan Constraint 12). Promote, demote, approval-gated promote, conflicting promote, and a release with a prune and a dependent rebuild.

---

## Deviations from the spec, and why

1. **Promote and demote share one planner.** Spec §9 says "one planner per operation". They are not two operations on the wire: `hitch promote b dev` and `hitch demote b dev` are the same declaration edit with opposite direction, and they share the approval gate, the lock, the auto-stash, the rollback, the sibling-conflict check, and the dependent rebuild. Two planners would mean two copies of that machinery that have to agree forever — the exact failure class the program is removing. So `src/operations/declaration.rs` exposes `plan_promote` and `plan_demote` as *named entry points* (a reader grepping for "the promote planner" finds one immediately) over a shared `plan_declaration_change`, and `apply_declaration_plan` is the single executor. "One planner per operation" still holds in the sense that matters: there is exactly one place each decision is made, and no operation has a second path.
2. **`PlanWarning.blocking: bool` becomes `PlanWarning.kind: PlanWarningKind`.** P5 is the first phase with two *different* kinds of blocking plan, and a bool cannot tell them apart: an approval gate must produce `OperationOutcome::ApprovalRequested` and exit 0, while a policy refusal must produce `PlanApplyError::PolicyBlocked` and exit 1. Both are "the plan does not apply". A boolean would force the executor to string-match the message to tell them apart, which is the "one fact, two representations" bug in miniature. `PlanWarningKind::{ApprovalRequired, PolicyRefusal, Advisory}` is the honest vocabulary, and `is_blocking()` replaces `blocking`.
3. **`PlanApplyError::PolicyBlocked`'s message stops assuming a conflict policy.** P4 wrote it as "To override this environment's conflict policy: {remedy}", because its imagined only producer was the halt. Its first real producer is the pre-promote sibling-conflict check, whose remedy is not a conflict-policy override at all. The variant keeps its shape and gains a generic "To proceed: {remedy}" line; the remedy is supplied by whoever raises it. This is the reason `PolicyBlocked` has a producer at all — had the message stayed conflict-specific, the model would have forced the promote refusal to be something it is not.
4. **`command_hint` gains an `argument`.** P4's `command_hint(environment)` was adequate for `rebuild` and `release` and meaningless for the branch-taking operations. A stale-plan remedy must be the *exact* command to re-run, and the exact command is the one the user typed — including the environment-name-expands-to-branches form, where re-running with the resolved branch list is a different command that does the same thing but is not what they ran. So the plan records the positional `argument` alongside the resolved branches and builds the remedy from it.
5. **Release's dependent-environment preflight is removed from the decision path.** `rebuild_dependent_environments` currently runs `preflight_compatibility_merge_tree` and *skips* a dependent environment whose composition would conflict. That is a second merge engine making a second merge decision, one layer above the one `hitch rebuild` uses — the P1 bug class, still live in `release` because P1 only touched `rebuild`'s dry-run. Under P5 the nested rebuild plans and composes, and a conflict becomes *holds* in the receipt rather than a silent skip. This is a deliberate behaviour change and it is the correct one: `preflight_compatibility_report` remains a legitimate *display* preflight, and this was a decision, not a display.
6. **`PromotionPrune` is decided at plan time against the *planned* target commit, not the live one.** The prune predicate is "is this promoted branch now contained in this environment's base", and for every environment whose base is the released target the answer changes *because of this release*. Evaluating it before the publish (as the planner must) would answer "no" for exactly the branches the release just integrated. `is_branch_merged_into` runs `merge-base --is-ancestor`, which takes any rev, so the planner evaluates envs based on the target against the *composed* commit. The alternative — deciding the prune after the publish, in the executor — would put a decision in the executor, which is the thing this program is built to prevent.
7. **`PlanPurpose` is dropped from the declaration planner's signature.** The plan above predicted `plan_declaration_change(.., purpose, ..)` with a Global Constraint insisting `PlanPurpose` be passed "only to be handed down to the nested rebuild". On writing it, the parameter promised a property the function could not enforce: a declaration plan composes no commit, so `anchors()` is `true` for `Confirm` and would be ignored either way, and the nested rebuild does **not** take the outer plan's `purpose` — it calls `rebuild_environment`, which is unconditionally a `Confirm`. Passing a parameter that is read once and thrown away teaches the next reader that the function honours a distinction it does not. `plan_release` keeps `purpose` because it *does* branch on `synchronizes()` and `anchors()`.
8. **`ReleaseReport` is not introduced.** It appears in Task 4's interface list and nowhere else — no step used it, because `ExecutionReceipt` already carries every field it would have: `OperationOutcome` for the verdict, `effects` for the writes, `warnings` for the owed work, `resulting_state` for the post-condition. A second struct that paraphrases the receipt is a second source of truth for the same run, which is the failure this whole program exists to remove. The CLI's progress printing in P6 renders the receipt; it does not build a report on the way.
9. **`DependentRebuild` gains `because: String` and `base_environment: Option<String>`** beyond the `{ environment }` the plan above specified. `because` was required anyway — Task 4's own effect list says the step label names the reason — and the plan doc simply omitted the field it demanded. `base_environment` is the load-bearing one: the old `rebuild_dependent_environments` skipped a dependent when **its base environment** was not rebuilt, not when the base was merely *attempted*. A locked or preflight-excluded base is excluded from the attempted set, so the two differ, and folding them would silently rebuild an environment in the wrong order against a base that had not been rebuilt. `base_environment: None` means "built on a real branch, not another environment", which is what makes the check expressible at all.
10. **A dependent the planner can prove will be skipped is excluded from the plan, not declared and then skipped.** The old code ran the preflight at apply time and logged a skip. Under P5 the planner runs it too (deviation 5) and, on failure, emits an `Advisory` and leaves the environment out of `dependents` — "the plan predicts; the receipt observes". `DependentRebuildOutcome::Skipped` is therefore for the *runtime* skips only (a base environment that failed its own rebuild, a `--no-rebuild-dependents` release). Both shapes need to exist: a plan that declares a rebuild it knows cannot happen is a lie, and a receipt with nowhere to put a genuine skip is a lie of the other kind.
11. **A failed dependent rebuild is an owed effect, not an error — for promote, demote, *and* release.** The plan specified this for the declaration executor and only half-argued it for release. It is the same rule: the declaration edit (promote/demote) or the merge-and-tag (release) is the operation's durable effect, and `rollback_metadata_changes` restores a *whole-config snapshot*, which is the wrong instrument for a nested rebuild that may already have moved the environment branch before failing. Contract, identical in all three: exit 0, the declaration persists, the environment is left unbuilt, and the message names `hitch rebuild <env>`. Rollback stays reachable for the one failure it can actually repair — the metadata write itself.
12. **Release's `current`/`proposed` describe the *target*, not the released environment.** A release's ref effect is on `refs/heads/<target>`; the environment branch moves only via the nested dependent rebuild, which is a *declared* effect rather than a predicted one (nobody can predict a composition's content at plan time — that is what the nested plan is for). Describing `dev` in `current`/`proposed` would claim a proposal to an environment the release does not directly edit, and `status`-style readers would report the release as having changed something it did not. The released environment's own rebuild appears as a `DependentEnvironmentRebuild` like any other — and release happens to declare it, because the target moving is precisely why it needs one.
13. **`target_sha_before` is read with `rev_parse_opt("refs/heads/<target>")`, not `get_branch_commit_sha`.** The latter falls back to `refs/remotes/origin/<target>` when the local branch is absent, which would hand the CAS an `expected_old` belonging to a *different* ref. A release into a target that exists only on the remote is a legitimate state, and this is the one place where reading it through a remote fallback would be silently wrong rather than merely unusual.

---

## Tasks

### Task 1 — The model, extended

**Files:** `src/operations/model.rs`, `src/operations/rebuild.rs` (two `PlanWarning` construction sites)

**Interfaces:**
- Produces: `OperationKind::{Promote, Demote, Release}`; `OperationIntent::{PromoteBranches, DemoteBranches, ReleaseEnvironment}`; `PlanWarningKind`; `PlannedEffect::{DependentEnvironmentRebuild, PromotionPrune, TagCreation}` and the matching `AppliedEffect` variants; `DependentRebuildOutcome`.
- Changes: `PlanWarning { message, kind }`; `PlanWarning::blocking`/`advisory`; `OperationPlan::blocked_by()`; `OperationKind::command_hint(environment, argument)`; `PlanApplyError::stale_plan(kind, environment, argument, changed)`.

- [x] Add `Promote`, `Demote`, `Release` to `OperationKind` (`:27`) and extend `Display` (`:42`) and `command_hint` (`:37`). `command_hint` becomes `(self, environment: &str, argument: &str) -> String`; `Rebuild` and `Release` use the environment only and ignore `argument`, which is why the parameter is documented rather than hidden behind a second method.
- [x] Add `PromoteBranches { environment, branches }`, `DemoteBranches { environment, branches }`, `ReleaseEnvironment { environment, target }` to `OperationIntent` (`:57`) and its `Display` (`:63`).
- [x] Replace `PlanWarning.blocking: bool` with `kind: PlanWarningKind` (`:270`) and add:
  ```rust
  pub enum PlanWarningKind { ApprovalRequired, PolicyRefusal, Advisory }
  ```
  with `PlanWarning::blocking(msg)`, `PlanWarning::advisory(msg)`, and `is_blocking()`. Doc each variant with the outcome it produces: `ApprovalRequired` → `OperationOutcome::ApprovalRequested`, exit 0, requests created; `PolicyRefusal` → `PlanApplyError::PolicyBlocked`, exit 1, nothing written; `Advisory` → the operation proceeds. The reason a bool was not enough is recorded in the type's doc comment, because a future agent will otherwise "simplify" it back.
- [x] Add `PlannedEffect::DependentEnvironmentRebuild { environment, because, refname }`, `PlannedEffect::PromotionPrune { environment, branches, refname }`, and `PlannedEffect::TagCreation { name, target_sha }` (`:192`), plus `AppliedEffect` mirrors (`:488`). Extend both `refname()` methods. Update the doc comment on the enum to record the new invariant: **`refname()` is a grouping key, not a uniqueness constraint.** A release declares both a `MetadataChange` on `hitch-metadata` and a `PromotionPrune` on the same ref, and a promote declares a `MetadataChange` on `hitch-metadata` and a `DependentEnvironmentRebuild` on the environment's own branch. Matching an applied effect back to its prediction is the executor's business, because only the executor knows the correspondence.
- [x] Add `DependentRebuildOutcome { Rebuilt, Skipped(String), Failed(String) }`. The three-way split is the point: a skip is neither success nor failure, and flattening `Skipped` into `Failed` would report a deliberate `--no-rebuild-dependents` as an error, while flattening it into `Rebuilt` would report a locked environment as rebuilt.
- [x] Change `OperationPlan::blocked()` to `blocked_by() -> Option<&PlanWarning>` filtered on `is_blocking()` (`:446`), keeping the single-blocking-warning rule and updating its doc comment to name both producers.
- [x] Change `PlanApplyError::stale_plan` (`:640`) to take `(kind, environment, argument, changed)` and build the remedy with `kind.command_hint(environment, argument)`. Generalise the `PolicyBlocked` `#[error]` text (`:577`) from "override this environment's conflict policy" to "To proceed", and say why in a comment.
- [x] Update the two `PlanWarning { .. }` sites in `src/operations/rebuild.rs` (`:450` held-branch warnings → `advisory`) and the `validate_plan` → `stale_plan` call (`:613`).
- [x] Extend `src/operations/model.rs`'s unit tests for every new variant: each `OperationKind` renders a remedy ending in a runnable command; each `PlanWarningKind` is/isn't blocking; `blocked_by` returns the blocking warning for `ApprovalRequired` and `PolicyRefusal` and `None` for `Advisory`; `refname()` on each new variant returns what its doc comment says; `stale_plan` for a promote names the promote command and the user's original argument.
- [x] `just format && just lint && cargo test -p hitch --lib`.

### Task 2 — `src/operations/declaration.rs`: the promote/demote planner and executor

**Files:** `src/operations/declaration.rs` (new), `src/operations/mod.rs`, `src/lib.rs` (no change — `operations` is already registered)

**Interfaces:**
- Produces: `DeclarationChange`, `DeclarationPlanOptions`, `DeclarationPlanDetail`, `plan_promote`, `plan_demote`, `plan_declaration_change`, `discard_declaration_plan`, `validate_declaration_plan`, `apply_declaration_plan`.
- Consumes: `plan_rebuild`/`apply_rebuild_plan` (the nested rebuild), `rebuild_environment` (`src/utils/prelude.rs:895`), `check_pre_promote_conflicts` (`src/utils/prelude.rs:2479`), `create_approval_requests_for_operation` (`:2182`), `build_state_snapshot`.

- [x] `pub mod declaration;` in `src/operations/mod.rs`, and extend the module header to say what P5 added: the same one-planner discipline, and that promote/demote share a planner because they are one declaration edit.
- [x] Define the direction as an enum so the two operations cannot be confused with a both-empty edit:
  ```rust
  pub enum DeclarationChange { Add(Vec<String>), Remove(Vec<String>) }
  ```
  with `branches()` and `kind()` accessors.
- [x] Define `DeclarationPlanOptions { no_rebuild: bool }` and `DeclarationPlanDetail`, whose fields are each justified in a doc comment:
  - `environment: String` — the declaration being edited.
  - `argument: String` — **the positional argument the user typed**, not the resolved branch list. A stale-plan remedy must reproduce the command that was run; for the environment-name-expands-to-branches form those differ.
  - `added: Vec<String>` / `removed: Vec<String>` — the resolved edit, in the order given.
  - `proposed_branches: Vec<PinnedBranch>` — the declaration *after* this plan, in declaration order, new branches pinned to the SHAs the plan read. This is what the nested rebuild will compose, so it is computed here rather than re-read later.
  - `read_branches: Vec<PinnedBranch>` — every branch tip the plan consulted, which is what the sibling-conflict check and the nested rebuild both consume.
  - `rebuild: bool` — `false` under `--no-rebuild`. A declaration-only change *intentionally* leaves the environment branch stale, and the plan has to say so rather than quietly omit the consequence.
  - `approval_requests: Vec<String>` — populated only by the executor, which is where the requests are actually created.
- [x] `plan_promote` / `plan_demote` — thin wrappers constructing `DeclarationChange::Add`/`Remove` and delegating. The branch argument reaching them is the *original* one, so both the expansion and the remedy happen in one place.
- [x] `plan_declaration_change(context, environment, change, argument, options, purpose, on_step) -> Result<OperationPlan<DeclarationPlanDetail>>`:
  - read the declaration; environment must exist; must not be locked; for `Add`, no branch may already be promoted (today's error text verbatim);
  - pin every branch tip the edit touches **and every existing promoted branch tip** (the sibling check needs them, and a plan that fingerprints a ref it read is the only kind that can be validated);
  - for `Add` with existing siblings, run the conflict simulation and, on failure, record a `PolicyRefusal` **blocking** warning whose message is today's error text — including the literal "compatibility check failed", the `<branch> conflicts with <base>` line, and the conflicted file list, because `test_promote_blocked_by_sibling_conflict` asserts all three. Refactor `check_pre_promote_conflicts` (`src/utils/prelude.rs:2479`) into a reason-returning form (`pre_promote_conflict_reason`) that `check_pre_promote_conflicts` still wraps, so no other caller loses the error. Then the simulation happens *in the plan*, where its verdict is visible, instead of in a pre-check that intercepts;
  - if the environment requires approval, record an `ApprovalRequired` **blocking** warning and *stop*: no proposed declaration, no composition, `current == proposed`, effects empty except the approval requests the executor will create. A gated plan that also proposed a declaration would be a plan for an operation that will not happen;
  - build `current`/`proposed` `EnvironmentProjection`s, effects, `unaffected`, and the fingerprint (`metadata_sha` + every pinned branch ref). Fingerprint `refs/heads/<env>` too: promote rewrites the environment's own branch through the nested rebuild, so a concurrent rebuild would invalidate this plan;
  - under `--no-rebuild`, add an `Advisory` warning naming the exact remedy (`hitch rebuild <env>`) and emit no `DependentEnvironmentRebuild` effect.
- [x] `discard_declaration_plan` — a no-op today, and deliberately so. It exists because "a plan that will not be applied releases its resources" is a property every planner owes, and a declaration plan currently owns nothing. Its doc comment says that, so the next agent does not read the emptiness as a bug and remove the function. When a declaration plan later gains a composed commit, this is where its anchor goes.
- [x] `validate_declaration_plan` — a structural sibling of P4's `validate_plan`, over `metadata_sha` + `refs`. **Factor the shared body** into a helper in `src/operations/model.rs` (`pub fn changed_inputs(fingerprint: &PlanFingerprint, git: &GitOperations) -> Vec<ChangedInput>`) and have P4's `validate_plan` call it, adding only the resolution-existence arm on top. Two copies of the same diff loop is how a plan starts refusing for a reason the other does not.
- [x] `apply_declaration_plan(context, plan, on_step) -> Result<ExecutionReceipt>`:
  - `if let Some(warning) = plan.blocked_by()`: match `warning.kind()`.
    - `ApprovalRequired` → create the approval requests for the plan's resolved branches in one transaction (the mid-batch-failure reasoning at `src/commands/promote.rs:215` still holds and moves with the code), display each exactly as today, and return a receipt with `outcome: ApprovalRequested` and one `MetadataChange` effect naming the requests. No declaration edit, no lock, no rebuild.
    - `PolicyRefusal` → return `PlanApplyError::PolicyBlocked { environment, reason, remedy }` with the remedy the warning names. Nothing is written; this is the exit-1 path.
  - otherwise `on_step("Updating environment declaration")`, then a single `modify_metadata` that applies the plan's `added`/`removed` verbatim — **not** by re-deriving them from the config. Re-deriving is a second decision point, and the whole point of the edit being in the plan is that it is decided;
  - unless `!plan.detail.rebuild`: `on_step("Rebuilding '<env>'")` and call `rebuild_environment` (`src/utils/prelude.rs:895`) — the P4 wrapper, which plans and applies internally. This is the nesting path on purpose: the dependent rebuild gets a plan and a receipt of its own, and reusing the wrapper rather than re-implementing plan-then-apply here is what keeps that true;
  - assemble the receipt by reading the declaration back from `hitch-metadata` and confirming the plan's `proposed_branches` is what is now declared. **Read the effects back, never copy them from the plan** — the same rule P4's `assemble_receipt` follows;
  - a failed nested rebuild is an `ExecutionWarning { owes_effect: true }`, not an error. The declaration change has landed and is durable; the rebuild not having happened is owed work, and reporting the whole promote as failed would tell the user to re-run something that has already applied.
- [x] Unit tests in the module for: `DeclarationChange` accessors; `proposed_branches` order preserved (never sorted) for a three-branch add; a gated environment's plan is `blocked_by() == ApprovalRequired` with `current == proposed` and empty composition; the conflict refusal is a `PolicyRefusal` warning; `--no-rebuild` omits the dependent-rebuild effect and carries the advisory.

### Task 3 — Rewire `promote` and `demote`

**Files:** `src/commands/promote.rs`, `src/commands/demote.rs`

**Interfaces:**
- Consumes: `plan_promote`, `plan_demote`, `apply_declaration_plan`, `blocked_by`.

- [x] `promote::run`: keep the preamble (repo pre-check, the "Resolved '<env>' → N branch(es)" line) and replace `validate_preconditions` with a plan. The new shape:
  ```rust
  let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut |s| { ... })?;
  ```
  Branch resolution moves *into* the planner, because the expansion result is part of the plan (the effect list, the proposal, the fingerprint all depend on it) and doing it in the command would mean two places know the answer. Note there is no `PlanPurpose` here (deviation 7) — this planner anchors nothing, so there is no distinction for it to honour.

  **The plan is built *inside* `with_locked_env`, not before it.** Forced, not stylistic: `with_locked_env` commits the lock to `hitch-metadata` *before* running its closure, so a plan built outside the lock is stale the moment it is validated — `metadata_sha` no longer matches. Measured directly: 13 of 17 promote tests failed with `The plan for 'dev' is no longer current: hitch-metadata: d3da09e → 82658aa` when the plan was built first. The consequence for the *planner* is that it must not check `is_locked()` (by the time it runs, the lock is the command's own), so the human-lock refusal moves into the command, ahead of the lock, in the same place `commands/rebuild.rs` puts it. `ensure_environment_exists` moves into the command for the same reason — from inside the lock a missing environment is indistinguishable from a lock conflict, and two tests were getting the wrong message.

  ```rust
  let result = with_auto_stash(context, || with_locked_env(context, &args.env_name, || {
      // capture_config_state *after* the lock: capturing before would record
      // a pre-lock declaration, and rolling back to that would undo the lock
      // commit too.
      let mut rollback_info = RollbackInfo::new();
      capture_config_state(context, &mut rollback_info)?;
      let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut on_step)?;
      log_resolved_branches(&plan, &args.env_name);
      apply_declaration_plan(context, &plan, &mut on_step)
  }));
  ```
- [x] Route the blocked/unblocked decision explicitly, and only take the lock when the plan will actually mutate:
  ```rust
  let result = if plan.blocked_by().is_some() {
      // Nothing about the environment changes on this path, so taking the
      // environment lock would be locking a repo we are not about to touch.
      apply_declaration_plan(context, &plan)
  } else {
      with_auto_stash(context, || with_locked_env(context, &args.env_name, || {
          rollback_info.previous_config = capture_config_state(context)?;
          apply_declaration_plan(context, &plan)
      }))
  };
  ```
  `capture_config_state` stays *inside* the lock, after it: capturing before would record a pre-lock declaration, and rolling back to that would undo the lock commit too.
- [x] Keep the success/error output, including the `Err` branch's "show the error first, then roll back" ordering and the `CRITICAL: Failed to rollback metadata changes` message. `rollback_metadata_changes` must still fire for a failed apply — the declaration edit is the thing it undoes, and the plan does not make it unnecessary.
- [x] Demote: the identical shape. Its precondition set is smaller (no sibling-conflict check, and the "not currently promoted" case is `remove_branch`'s job) but the *planner* owns those checks, not the command.
- [x] Preserve the `--no-rebuild` message verbatim (`promote_demote_tests.rs:760`, `:878` assert on "Skipping rebuild").
- [x] Preserve `test_promote_blocked_by_sibling_conflict`'s three assertions by construction — that is the whole reason the refusal text moves rather than being rewritten.

### Task 4 — `src/operations/release.rs`

**Files:** `src/operations/release.rs` (new), `src/operations/mod.rs`

**Interfaces:**
- Produces: `ReleasePlanOptions`, `ReleasePrune`, `ReleasePlanDetail`, `plan_release`, `discard_release_plan`, `validate_release_plan`, `apply_release_plan`. ~~`ReleaseReport`~~ — not introduced; deviation 8.
- Consumes: `merge_tree_compose`, `commit_tree`, `publish_branch`, `push_branch_with_deploy_key_if_configured`, `push_tag`, `is_branch_merged_into`, `topological_environment_order`, `rebuild_environment`, `with_locked_env`.

- [x] `pub mod release;` in `src/operations/mod.rs`.
- [x] Move `create_release_tag` (`src/commands/release.rs:462`) and its two unit tests **into this module unchanged**, and move `build_conflict_error` (`:409`) with it. The tag-collision handling is release's planner/executor logic now, and its two tests — same-content retry reuses the tag, a genuinely different second release in the same second does not overwrite — are exactly the oracle for the "the executor may land a different tag name than the plan predicted" rule.
- [x] Define `ReleasePlanOptions { squash: bool, no_prune: bool, no_rebuild_dependents: bool }` and `ReleasePlanDetail`:
  - `environment`, `target: String` — resolved before planning by the command, because `confirm_release` needs it first.
  - `released: Vec<PlannedBranch>` — each promoted branch pinned to the SHA the composition consumed, in declaration order.
  - `squash: bool` — part of the plan, because it changes the parent count of the commit that is about to exist.
  - `result_sha: String` — the composed tip, already in the object database.
  - `anchor_ref: Option<String>` — `refs/hitch/release/<target>/<timestamp>`, `None` for a preview. **Keep the `release/` family**, not `build/`: the two are different lifecycles (a rebuild's anchor is one commit; a release's is a published-and-anchored tip) and renaming it would orphan every anchor a half-finished release left behind.
  - `backup_timestamp` is **absent**: release passes `backup_timestamp: None` to `publish_branch` on purpose, because the release tag is its rollback anchor. Do not add it.
  - `tag_name: String`, `tag_message: String` — the *base* name. The executor may land a disambiguated variant, and the receipt records what it got.
  - `remote_target_sha_before: Option<String>`
  - `prunes: Vec<ReleasePrune>`, `dependents: Vec<DependentRebuild>` — see below.
- [x] Define the two decision records, each with a doc comment saying what a `Skipped` reason means:
  ```rust
  pub struct ReleasePrune { pub environment: String, pub branches: Vec<String> }
  pub struct DependentRebuild {
      pub environment: String,
      pub because: String,
      pub base_environment: Option<String>,
  }
  ```
  Both extra `DependentRebuild` fields are deviation 9, and `base_environment` is the load-bearing one: the old `rebuild_dependent_environments` skipped a dependent when **its base environment** was not rebuilt, not when the base was merely *attempted*. A locked or preflight-excluded base is excluded from the attempted set, so the two cases differ, and folding them would silently rebuild an environment against a base that had not been rebuilt. `None` means "built on a real branch, not another environment", which is what makes the check expressible at all.
- [x] `plan_release(context, environment, target, options, purpose, on_step) -> Result<OperationPlan<ReleasePlanDetail>>`:
  - environment must exist and have at least one promoted branch. **`Err` with today's "No branches promoted … nothing to release" text.** `release::run` keeps its own early `return Ok(())` for that case, which is what preserves the exit-0 behaviour; the planner refusing guarantees a degenerate plan (an empty release that proposes a tag on the target tip) is unreachable from any other caller.
  - `on_step("Synchronizing branches")` and, when `purpose.synchronizes()`, `synchronize_branches(branches + target)`. Every promoted branch must exist locally afterwards, else today's error text verbatim.
  - pin each promoted branch tip; read the target tip (the CAS old value) and the remote target tip, both *before* composing.
  - compose the chain exactly as today: `merge_tree_compose` per branch, `commit_tree` with one parent under `--squash` and two otherwise, the `squash && tree unchanged → skip the commit` case preserved. Any conflict returns `Err` via `build_conflict_error` — **before** the anchor, the tag, or anything else. All-or-nothing is a property of where this line sits.
  - anchor the composed tip when `purpose.anchors()`.
  - compute `prunes`: for every environment other than the released one, every promoted branch it shares with the released set, pruned when it is an ancestor of that environment's base — **evaluating a base equal to the release target against `result_sha`, not the live target ref** (deviation 6). Every environment that is *not* pruned gets an `Advisory` warning saying so and why: locked by another operation, base missing, branch missing, or not yet merged. Today those are `log_warning`/`log_verbose` lines emitted during the mutation; as plan facts they are the reader's only chance to see them before anything is written.
  - compute `dependents`: the current topological closure of "base moved" ∪ "pruned" ∪ "base is one of those", in `topological_environment_order` — which moves into this module.
  - effects: `LocalRefUpdate{target, old, new}`, `TagCreation`, `MetadataChange{anchor}` when anchored, `MetadataChange{hitch-metadata, "'released_at' stamp for 'dev'"}`, `RemoteRefUpdate{refs/remotes/origin/<target>}` **only when `context.should_push()`** (a plan that predicts a push the apply will not make is the false "fully synced" P4 forbids), a `PromotionPrune` per entry in `prunes`, and a `DependentEnvironmentRebuild` per entry in `dependents` (with `because` naming the reason: "its base is the released 'main'", "its declaration was pruned").
  - fingerprint: `metadata_sha`, every released branch ref, the target ref, and `refs/remotes/origin/<target>`. Not the pruned environments' branch tips — this plan does not read them, and the *nested* plan for each dependent fingerprints its own.
  - `confirmation`: `required` with a reason naming the merge into the target, matching what `confirm_release` already tells the user. The command's existing prompt is untouched — P6 owns the UX.
- [x] `discard_release_plan` — deletes the `refs/hitch/release/*` anchor. Same `finally` obligation as P4's.
- [x] `validate_release_plan` — over the fingerprint, using the shared `changed_inputs` helper from Task 2.
- [x] `apply_release_plan(context, plan, on_step) -> Result<ExecutionReceipt>`, structured exactly as P4's (validate → work → `discard` in a `finally` around the inner function):
  1. `validate_release_plan`, converted with `into_anyhow` so the typed error survives.
  2. `create_release_tag` — the actual name, possibly disambiguated.
  3. `publish_branch(target, result_sha, &[], None, retry_hint, push_remedy, push_closure)`. **`extras: &[]` and `backup_timestamp: None` are load-bearing and stay** — release has no truthful input for a build record (its included-branch list is the thing a human assembled by promotion) and its tag is its own rollback anchor. This is the documented asymmetry with rebuild, and the comment at `src/commands/release.rs:321-333` moves here with the code.
  4. Push the tag, keeping today's careful gating: only when `context.should_push()` **and** `refs/remotes/origin/<target>` reads the new tip, because `publish_branch` swallows a branch-push failure and reaching this point does not mean the branch push landed. The warning texts move verbatim.
  5. Stamp `released_at` and apply the plan's prunes in one `modify_metadata`, removing exactly `plan.detail.prunes` — not re-running the predicate.
  6. Rebuild each dependent in topological order, with the plan's `because` as the step label and `rebuild_environment` (or `with_locked_env` + `rebuild_environment` for anything but the released environment, which is already locked by the outer release). A skip is `Skipped(reason)`; a failure is `Failed(error)` and an `ExecutionWarning { owes_effect: true }`; neither fails the release, because the release merge and tag have already landed and reporting failure would send the user to re-run a release that succeeded.
  7. Assemble the receipt: read the target tip back and confirm it is the plan's `result_sha` (P4's internal-consistency check, for the same reason); record the tag under the name the executor actually got; record each prune by the branches it actually removed; record each dependent by its outcome; `resulting_state` from `build_state_snapshot`. Push `PushOutcome::Failed`/`Declined` become owed-effect warnings with the same wording as P4's, adjusted to the target branch.
  - Outcome: `Applied`. Holds are not a release concept — release composes all-or-nothing, so a conflict never reaches an apply.
- [x] Unit tests in the module, but **not all five, and the split is the point.** What stayed in `src/operations/release.rs` is what can be tested without a real hitch repo: `create_release_tag`'s two collision tests (moved verbatim, the oracle for "the executor may land a different tag than the plan predicted") and `a_branch_merged_by_the_release_is_not_an_ancestor_of_the_live_target_yet` (the differential half of the `result_sha` rule, in a `git init -q -b main` scratch repo). The other four — empty environment refuses, conflict refuses and writes nothing, the dependent closure is the topological set, no `RemoteRefUpdate` when `should_push()` is false — need a real `hitch-metadata`, a real origin, and a real build record, and a real hitch repo is too heavy to stand up in a `src/` unit test even though `GlobalContext::new_at_path` exists. They are in `tests/integration/plan_apply_tests.rs` (Task 7) instead, where the same claims are made against the real thing.

### Task 5 — Rewire `release`

**Files:** `src/commands/release.rs`

**Interfaces:**
- Consumes: `plan_release`, `apply_release_plan`, `ReleasePlanOptions`.

- [x] `perform_release_core` becomes: `plan_release` → `apply_release_plan`. Everything it did inline (synchronise, pin, compose, anchor, tag, publish, tag-push, prune, dependent rebuilds) is now either in the planner or in the executor. The function keeps its name and its signature minus the composition detail, because `run` is its only caller and the two `with_locked_env`/`--force` arms should not have to change.
- [x] Delete the now-relocated helpers: `build_conflict_error`, `create_release_tag` (+ its `#[cfg(test)] mod tests`), `update_release_metadata_and_prune`, `rebuild_dependent_environments`, `topological_environment_order`. Leave `resolve_target_branch`, `validate_preconditions`, `confirm_release`, and `perform_release_core` in place. The file goes from 963 lines to roughly 300, and the residue should be only the parts that genuinely belong to the command.
- [x] Keep the confirmation prompt and the `--force` arms byte-identical. `confirm_release` runs *before* planning, so it must not depend on anything the planner computes — it reads the declaration itself, which it already does.
- [x] Keep `"Releasing N promoted branches from environment 'X' to 'Y'"` and the per-merge `"Merging '<b>' into '<t>'..."` lines, now emitted as `on_step` callbacks from the planner. They are the user-visible account of what is being composed and they must survive the move.
- [x] Keep the trailing `log_success("Environment 'X' released successfully to 'Y'!")` in `run`, outside `perform_release_core`, as today.

### Task 6 — `approvals/approve.rs`, and `rebuild_environment_opts`'s remaining callers

**Files:** `src/commands/approvals/approve.rs`, `src/utils/prelude.rs`, `src/operations/rebuild.rs`

**Interfaces:**
- Consumes: `plan_promote`-shaped planning for the approve path.

- [x] ~~Plan the declaration change for every approved request in one plan.~~ **Not done as written; the bug it was aimed at is worse and different.** `hitch approve` takes a single `request_id` (`ApproveArgs { request_id, comment }`), so there is no "every approved request" to plan together — one invocation, one request, one branch. Batching was never available here.

  What the structure inspection *did* find is a live correctness bug. `execute_approved_operation` wrapped `with_locked_env` → `modify_metadata` → `execute_operation_based_on_request`, and the last of those called `rebuild_environment` **inside** the `modify_metadata` closure. That closure runs *before* its transaction commits — it is handed a `&mut HitchConfig` and the file is written afterwards — while the rebuild reads the declaration back off `refs/heads/hitch-metadata` (`read_file_from_branch` is `git show hitch-metadata:hitch.json`, and `begin_branch_write` only sets up a scratch index). So the approve path composed the environment from the **pre-approval** declaration: the approved branch landed in `hitch.json`, and the environment branch was rebuilt without it, and the two disagreed until something unrelated triggered another build. The regression test is the `cat-file` assertion in `test_automatic_application_on_threshold`; it fails on the pre-P5 code, and a manual `/tmp` run confirmed both directions (`FAIL: production:gated.js MISSING` before, `PASS` after).

  The fix is a **time** change, not an architecture change: the declaration edit and `mark_request_applied` stay one `modify_metadata` transaction, and the rebuild moves to *after* it, still inside the lock. `execute_operation_based_on_request` becomes `apply_declaration_change` — a pure metadata edit — and a new `rebuild_after_approval` does the build, reporting a failure as a warning naming `hitch rebuild <env>` and leaving the exit code at 0, for exactly the reason in deviation 11. Routing the whole approve path through `plan_promote`/`apply_declaration_plan` was considered and rejected: it would re-run `check_pre_promote_conflicts` at approval time on a request that was already vetted when it was created, and it would split the declaration edit from `mark_request_applied` into two transactions, which is strictly worse atomicity for a change that is one unit of intent. P6 can re-plan this path if a receipt ever needs to describe it.
- [x] **Keep `rebuild_environment_opts` (`src/utils/prelude.rs:919`).** After P5 its callers are exactly `commands/rebuild.rs` (twice: the normal and `--force` arms) and `rebuild_environment` (`:895`), which is itself the nesting entry point for the dependent rebuilds in `apply_declaration_plan` and `apply_release_plan`. That is its remaining job — *composed rebuilds for callers that do not render a plan* — and deleting it would push a plan-then-apply reimplementation into every nested call site. Update its doc comment to say so, and update the P4 plan's "What P5 inherits" claim that it serves promote/demote/approve/release, which is no longer true.
- [x] `rebuild.rs`'s two `rebuild_environment_opts` calls stay. They are the byte-identical output path P4 established, and P5 is not the phase to change what `hitch rebuild` prints.

### Task 7 — Integration tests

**Files:** `tests/integration/plan_apply_tests.rs` (extend), `tests/integration/promote_demote_tests.rs` (extend), `tests/integration/release_tests.rs` (extend), `tests/integration/mod.rs` (no change — `plan_apply_tests` is registered)

**Interfaces:**
- Consumes: every public item from Tasks 1–6.

- [x] Reuse P4's `plan_apply_tests.rs` helpers (`context_for`, `plan`, `apply`, `git_plain`, `rev`, `all_refs`, `sibling_path`, `init_bare_origin`). Add a promote section:
  - a promote's plan proposes the new declaration and a dependent rebuild, and its receipt reports the declaration edit read back from `hitch-metadata` — not copied from the plan;
  - **stale refusal**: plan, promote something else, apply → `PlanApplyError::StalePlan` naming both changes, and `hitch-metadata` still holds only the first promote's branches;
  - same inputs → same plan id; a moved promoted-branch tip → different id;
  - `--no-rebuild` leaves `refs/heads/dev` absent and produces an advisory naming `hitch rebuild dev`;
  - a gated environment: the plan is `blocked_by() == ApprovalRequired`, `current == proposed`, and the apply creates requests and returns `ApprovalRequested` **with `refs/heads/dev` still absent and the declaration unchanged**;
  - a conflicting sibling: the plan is `blocked_by() == PolicyRefusal`, the apply returns `PolicyBlocked`, and nothing is written.
- [x] A demote section: the declaration loses the branch, the environment is rebuilt against the shorter list, and a stale plan refuses after a second demote.
- [x] A release section:
  - the plan names the tag, the target move, the prunes, and the dependents; the receipt names the tag the executor actually created;
  - **all-or-nothing**: a conflicting release plan returns `Err` and `refs/heads/<target>`, `refs/tags/`, and `hitch-metadata` are all byte-for-byte unchanged — assert each, not "nothing happened";
  - a release whose prunes remove a promoted branch from a second environment, asserting the prune appears in *both* the plan and the receipt with the same branch list;
  - a release whose dependent environment conflicts: the receipt records `Failed`, `has_owed_effects()` is true, and the release itself still succeeded (target advanced, tag created);
  - `all_refs` shows no `refs/hitch/release/` entry after every release path, including the refused one;
  - a denied push (P4's `pre-receive` hook in a sibling bare origin) produces an owed-effect warning naming `hitch push <target>` — release's push is a plain fast-forward, so the remedy is *not* `-f`;
  - a release under `--no-push` predicts **no** `RemoteRefUpdate`, asserted on the plan object.
- [x] Run the full existing suites and treat every edit as a red flag: `promote_demote_tests`, `release_tests`, `release_crash_recovery_tests`, `rebuild_tests`, `approval_workflow_tests`, `crash_recovery_tests`. If one needs its *assertions* changed rather than its fixtures, stop and work out why the plan disagrees with the behaviour it replaced.

### Task 8 — Manual check, gates, and documentation

**Files:** `AGENTS.md`, `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`, this plan

- [x] `cargo build -p hitch` (**debug** — the crash-recovery abort hook is `#[cfg(debug_assertions)]`-gated and a release binary silently ignores it) and drive a throwaway repo in `/tmp`: `hitch add dev`, `hitch promote feat-a dev` (clean, plan-backed), `hitch promote feat-a dev` again (already-promoted error), `hitch promote feat-conflict dev` (policy refusal, exit 1, no metadata change), `hitch demote feat-a dev`, `hitch promote --no-rebuild feat-a dev` (advisory + "Skipping rebuild"), `hitch set requires_approval dev true` then `hitch promote feat-a dev` (requests created, exit 0, no build), `hitch approve` (rebuild happens once, from the final declaration), and `hitch release dev` with a second environment to prune and a dependent to rebuild. Confirm `git for-each-ref | grep refs/hitch` is clean of `build/` and `release/` entries after every one of them.
- [x] `just format`, then `just format-check && just lint`, then `just test`. All three clean, zero `#[ignore]`d.
- [x] `AGENTS.md`: replace the P4 "What P5 inherits"-flavoured claims that promote/demote/approve/release go through `rebuild_environment_opts` with what is true after P5; add a gotcha for the declaration-plan shape (a plan that fingerprints `refs/heads/<env>` is invalidated by the nested rebuild it causes, which is why the inner plan is built after the outer edit lands) and for the prune-against-`result_sha` rule; extend the P4 block on plans rather than replacing it, since the anchor and `discard` obligations still stand.
- [x] This plan: an "As executed" section (deviations taken, what surprised us, non-vacuity evidence, the manual transcript) and a "What P6 inherits" section.
- [x] `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`: update the commit list, and mark P5 complete with P6 next.
- [x] Commit code and docs together. Then verify `git rev-parse main` is still `5d81fb2` and `git diff --name-only main..HEAD -- crates/` is empty. Do not push.

---

## As executed

**Status: COMPLETE.** All 8 tasks landed. Full suite green: 94 lib + 418
integration + 1 `no_args_help`, ~140s, zero `#[ignore]`d. `just format-check`
and `just lint` clean. `main` untouched at `5d81fb2`; `crates/` untouched.

Thirteen deviations are recorded above (1–6 from the plan, 7–13 found while
writing it). The ones that changed code rather than documentation:

### The approval gate was silent about itself

The first implementation of `plan_declaration_change` computed the gate as
`declared.requires_approval_check()`, pushed **no** warning, and returned the
plan. Seven promote tests failed with the plan applying anyway, because
`blocked_by()` found nothing to block on. The fix is
`approval_gated = refused.is_none() && declared.requires_approval_check()`,
followed by pushing the `ApprovalRequired` warning. The order is the lesson: a
policy refusal *outranks* the approval gate, because a request for approval on
an operation that will be refused is a request for nothing. This is recorded
here rather than in the code because the code reads as though the `&&` were
obvious, and it is not.

### Planning must happen inside the lock

Covered in Task 3. Worth restating as a shape, not an incident: **any planner
whose fingerprint includes `metadata_sha` must be built after every metadata
write that precedes it, including writes hitch makes on its own behalf.** For
promote/demote that write is the lock. A planner cannot be handed a pre-lock
plan and be asked to validate it against a post-lock repository, and no amount
of care in the command's ordering makes that work.

### A failed dependent rebuild no longer rolls anything back

Deviation 11, and the largest behaviour change in the phase. Previously
promote/demote/approve/release rolled the declaration back when a nested rebuild
failed. That was never safe — `rollback_metadata_changes` restores a
*whole-config snapshot*, and a nested rebuild can fail *after* moving the
environment branch, at which point the snapshot describes a branch that is in
the branch. The rollback's *own* test fixture does this deliberately
(`test_promote_whose_rebuild_fails_keeps_the_declaration` makes a dependent
rebuild fail with a push error, which lands the env branch first). Two promote
tests and one demote test were rewritten to the new contract: exit 0,
declaration persists, environment left unbuilt, message names
`hitch rebuild <env>`. Rollback remains reachable for the one failure it can
actually repair — the metadata write itself.

`approve.rs` had to be brought along manually, and that is where the phase's
only live bug turned up (Task 6): its rebuild ran *inside* the `modify_metadata`
closure and so composed from the pre-approval declaration. Confirmed both ways
before and after the fix with the debug binary against a throwaway repo —
`FAIL: production:gated.js MISSING` on the old shape, `PASS: … GATED CONTENT` on
the new one — and pinned by the `cat-file` assertion in
`test_automatic_application_on_threshold`.

### Release's output gained four lines

The plan required the two existing progress lines to survive the move. They
did, and the `on_step` plumbing added more, because the command passes a
*logging* closure rather than a silent one:

```
ℹ️ Tagging 'main'
ℹ️ Publishing 'main'
ℹ️ Updating release metadata
ℹ️ Rebuilding 'dev' — its declaration was pruned
```

All four are genuinely new information — the first three are steps that used to
be silent, the fourth is the `because` field deviation 9 added — and none of
the 20 existing `release_tests` assertions broke, because they assert on the
command's own lines, not on the absence of others. Recorded anyway, since
"no test broke" is not the same as "no output changed".

### Things that had to be pinned down, in a way the plan left open

- **`prunes` and `dependents` are computed in environment *name* order,**
  because `HitchConfig::environments` is a `HashMap` and iteration order is
  arbitrary. Two releases of the same input must produce the same plan, and a
  plan whose effect list reorders between runs is not a plan. `plan_dependents`
  iterates `topological_environment_order` and `plan_prunes` sorts by name;
  `a_release_plan_names_the_tag_the_target_move_the_prunes_and_the_dependents`
  asserts the exact order.
- **A thread-local `COMPOSED` smuggle was written and then removed.**
  `compose_release` originally parked the composed tip in a `static` so a deep
  helper could see it. That is a hidden channel between two functions in the
  same module, which is strictly worse than passing a `&mut [PlannedBranch]`
  out-parameter. The out-parameter is what shipped.
- **Release's own unit tests stayed in the module; its plan-level claims did
  not.** Task 4's unit-test list assumed a real hitch repo could be built in a
  `src/` unit test. It cannot. The three that could be pure stayed
  (`create_release_tag`'s two collision oracles and the `result_sha`-vs-live-tip
  differential); the four that need a real `hitch-metadata` moved to
  `tests/integration/plan_apply_tests.rs`, where they are stronger anyway.

### Non-vacuity evidence

- The approval bug's first regression assertion was written as
  `.expect("…")` on `env.git.run(&["cat-file", "-e", …])`. `run` returns
  `Ok(result)` for a non-zero exit, so the assertion only fired if git could not
  be spawned — it passed against the *buggy* code. Rewritten to inspect
  `.success()`, and it then failed against the buggy code exactly as it should.
  Worth carrying forward: `assert!(...run(...)?...)` reads like a real assertion
  and is not one.
- `a_conflicting_release_writes_nothing_at_all` asserts the target SHA, the
  `hitch-metadata` SHA, the full `show-ref` listing, and an empty `tag --list`
  *separately*. "Nothing happened" is three different claims, and the reason a
  release is all-or-nothing is the specific line the composition sits on — so
  the test has to be able to distinguish which of the three broke.
- `a_release_that_owes_a_dependent_rebuild_still_releases` deletes a
  dependent's promoted branch *after* planning. That works because
  `plan_release`'s fingerprint deliberately omits the pruned environments'
  branch tips (the plan does not read them; each nested rebuild fingerprints its
  own inputs), which is documented on the planner. It is the one place in P5
  where a fingerprint gap is load-bearing, so it is a test rather than an
  accident.
- `assert_no_release_anchors` is called on the success, the owed, *and* the
  refused release paths. `refs/hitch/release/*` is unpruned, so "the anchor is
  gone" is only worth asserting where an anchor would plausibly have been
  created.

### Manual transcript (`cargo build -p hitch`, throwaway repo)

`hitch release dev main --yes`, with `qa` holding `[feat-a, feat-b]` and `dev`
holding `[feat-a]`:

```
ℹ️ Releasing 1 promoted branches from environment 'dev' to 'main'
ℹ️ Synchronizing branches for release...
ℹ️ Merging 'feat-a' into 'main'...
ℹ️ Tagging 'main'
ℹ️ ✓ Created release tag 'hitch-release-dev-to-main-2026-09-26T10-31-42Z'
ℹ️ Publishing 'main'
ℹ️ Updating release metadata
ℹ️ Post-release: pruning promoted branches now in their base...
ℹ️ Post-release: pruned promoted branches from 2 environment(s): dev, qa
ℹ️ Post-release: rebuilding 2 affected environment(s)...
ℹ️ Rebuilding 'dev' — its declaration was pruned
ℹ️ Rebuilding 'qa' — its declaration was pruned
✅ Environment 'dev' released successfully to 'main'!
```

Verified afterwards: `main:a.txt` present; declarations `dev: []`, `qa: [feat-b]`;
the tag exists; `git for-each-ref refs/hitch/` shows only `backup/`, `prev/` and
`state/` — no `build/`, no `release/`. `hitch status` reports both environments
up to date. Exit 0.

An approval-gated promote → `hitch approvals approve <id> --yes` on the same
shape, from a repo whose git identity is the approver:

```
ℹ️   ✓ Branch 'gated' added to environment 'production'
ℹ️   ⏳ Rebuilding environment...
ℹ️ [2/3] Rebuilding environment 'production' - Merging 'gated'
```

`git cat-file -e production:gated.js` → present, containing `GATED CONTENT`.
The same scenario against the pre-P5 code printed the same two lines and left
the file absent, which is the bug. The unauthorized-approver path was exercised
too and still rolls back correctly (`✓ Restored previous environment state`,
exit 1) — rollback is intact where it can actually repair something.

---

## What P6 inherits

- **Three operations now produce a plan and a receipt; none of them is
  rendered.** P6 is the display phase, and it has more to work with than P5
  planned: `ExecutionWarning { owes_effect }` is already distinguished from a
  warning, and `OperationOutcome` already separates `Applied` from
  `AppliedWithHolds` from `ApprovalRequested`. The renderer should read the
  receipt, not re-derive any of it, for the reason `state_model.rs` gives.
- **P5 added output but P6 owns wording.** Four new release progress lines exist
  and are unstyled; the approval warnings name their remedies; the three
  "Skipping rebuild"/"will not be rebuilt"/"will be left stale" advisories are
  plain sentences. All of it is P6's material.
- **`preflight_compatibility_report` is still a decision point, in `resolve`
  only.** P1 removed `rebuild`'s second merge path and P5 removed release's; the
  two `preflight_compatibility_report` calls at `src/commands/resolve.rs:131`
  and `:180` still choose between Mode A and Mode B and refuse when there is no
  conflict at all. A preflight/composition disagreement there picks the wrong
  resolution mode. This is the last one, and P6 should not add dependants.
- **`hitch approve` produces no plan.** It is the one mutating command left
  without one, deliberately (Task 6). If P6 wants a receipt for it, the honest
  route is a `plan_declaration_change` for the *approved request's* change
  applied after the approval commits — not a re-run of `check_pre_promote_conflicts`.
- **`src/commands/resolve.rs` is the only remaining un-planned mutation.** P6
  or P7 gives it a planner, or the "one planner per operation" claim in the
  master plan stays an overstatement.
