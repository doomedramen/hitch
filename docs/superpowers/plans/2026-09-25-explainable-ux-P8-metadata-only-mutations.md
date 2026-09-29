# P8 — Metadata-only mutations: the last commands without a plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every significant mutation in the CLI shows a plan before it changes anything and a receipt after, in the one vocabulary `src/core/render.rs` already owns — and `hitch approve`, the last command that narrates itself, stops narrating.

**Architecture:** Two new planner modules, no changes to the plan/receipt *shape* beyond one field's type and one effect variant. (1) `src/operations/metadata.rs` holds the four operations that edit an environment's own declaration record and compose nothing: lock/unlock (one operation, two directions), `set`, add/remove (one operation, two directions). (2) `src/operations/cleanup.rs` holds the prune sweep, which deletes branches and archive refs rather than metadata fields, and is the only P8 operation whose effects can fail independently of each other. (3) `hitch approve` gets a plan for the declaration change its approval authorises, by adding a third `DeclarationChange` variant rather than a fifth planner — the approval bookkeeping commits first, the promotion is then a normal plan/apply, and both `attempt_*_rollback` helpers and the last `StepNarration::Log` call disappear.

**Tech Stack:** Rust, `serde`, `serde_json`, `clap`, `anyhow`, `colored`. **No new dependencies.**

**Spec section:** §7.1 (`OperationPlan`, `OperationKind` — all eleven variants), §7.3 (planned effects), §7.4 (fingerprint), §10.1 (semantic plan), §10.2 (`--yes`), §10.3 (`--json`), §10.4 (dry-run), §11.1 (Desired/Actual/Proposed), §17 (human terminology), §18 (semantic vs diagnostic channels), §19 (structured events — deferred to P9).

---

## Global Constraints

1. **`src/core/render.rs` remains the only place allowed to choose words.** P8
   *adds to* it and adds no sibling vocabulary. Every new headline, effect
   description and label for a new `OperationKind` is written there, not in the
   planner and not in the command. A planner that formats a description string
   is a second copy waiting to drift from the first four operations'.

2. **A plan is a decision, and a metadata plan's decision is the resolved
   edit.** The plan carries the *actual* delta, computed from the declaration as
   it is right now — not the flags as typed. `hitch set dev --add-approver
   a@b.c` where `a@b.c` is already an approver produces a plan with **no**
   `AddApprovers` change and a `NoChange` outcome, because the decision is "do
   nothing". An executor that re-derives the edit from the clap args is a
   second decision point, and the `apply_rebuild_plan` / `apply_declaration_plan`
   precedent is that the executor applies the plan verbatim.

3. **A metadata operation needs no rollback, and must not grow one.** Its whole
   effect is one `modify_metadata` closure, and `modify_metadata_impl` runs the
   closure at `src/utils/prelude.rs:414` *before* `write_file`/`commit_branch_write`
   — so a closure `Err` commits nothing. That is why the two `attempt_*_rollback`
   helpers in `approvals/approve.rs` are, on every path that reaches them,
   restoring a snapshot onto a ref that already holds it: a no-op commit
   wrapped in a narrative about a repair. Promote and demote keep their
   rollback because their nested rebuild can fail *after* the edit commits;
   nothing in P8 has that shape.

4. **A `PlanWarning` is a prediction and belongs in the plan only.** If the fact
   also holds afterwards, it belongs in `resulting_state`, read from
   `core::state`. This narrows what P8's planners may put in
   `ExecutionReceipt::warnings` to exactly one case: a cleanup delete that
   failed. `set --base`, `add --force`, `remove --force` and a declined
   confirmation are all consequences of the user's own flags, decided before the
   apply began, so none of them is an owed effect.

5. **A plan that composes nothing has no `CompositionPlan`, no holds, and a
   fingerprint over `metadata_sha` alone.** `PlanFingerprint` already
   `Default`s to exactly that (`src/operations/model.rs:493-516`), and the
   whitelist design means a lock does not go stale because someone else pushed
   an unrelated branch. Do not add refs to a metadata plan's fingerprint to
   "be safe" — that is the failure mode the field's doc comment names.

6. **A new `OperationKind` forces a decision in every `match`, because every
   `match` over it is total.** `declaration.rs` currently gets away with
   `match kind { Promote => …, _ => … }` in three places
   (`src/operations/declaration.rs:271-274`, `:447-450`, `:474-477`); those
   wildcards are the defect, and they become explicit arms in this phase. A
   wildcard that silently routes a new kind to the "demote" wording is a plan
   that describes the wrong operation.

7. **The `Current`/`Proposed` pair renders by a three-arm rule, on the model,
   never on the prose.** Both `Some` and equal → render nothing (a metadata
   operation's composition is unchanged, and "dev = main" twice says that twice
   while saying nothing about the lock). Both `Some` and different → render
   both. Exactly one `Some` → render that one, under its own title; that arm
   exists because `hitch add qa` has no `qa` to project before it runs and
   `hitch remove qa` will have none after, and `Option` is the only honest way to
   say that.

8. **Do not change exit codes.** The full inventory is in Task 11. A declined
   confirmation is **0** and writes nothing; a policy or validation refusal is
   **1**; `hitch approve` is **0** whether or not the promotion it authorises
   could be built. `hitch rebuild`'s exit 2 for holds is untouched by this phase.

9. **Every narration line a command loses is deleted, not relocated.** The
   `📋 Environment Update Preview` block in `set.rs:143-221`, the `⏳ Locking` /
   `⏳ Updating` / `⏳ Rebuilding` lines in `approve.rs`, the `Deleting N
   branch(es)…` / `Done: N deleted` pairs in `cleanup.rs`, and the
   `operation_info`/`operation_success` helpers become history. A command that
   keeps a hand-rolled preview *beside* `render_plan` is showing one operation
   in two vocabularies, which is the bug this whole program exists to remove.

10. **A `log_*` message must not carry its own glyph.** `log_success` prefixes
    `✅`; `commands/push.rs:36`, `commands/remove.rs:30` and
    `approvals/approve.rs:100-105` all pass a leading `✓` and currently print
    `✅ ✓ …`. Fix on sight, not in a follow-up.

11. **Nested operations narrate nothing.** Every new planner takes
    `StepNarration::Suppressed` by *omission*. The `StepNarration` argument stays
    on `rebuild_environment_opts` only for `approvals/approve.rs:388`, and this
    phase is what deletes that last `Log` call — after which the enum's `Log`
    variant has no caller and is deleted with it.

12. **`crates/hitch-desktop` is untouched**, and `main` stays at `5d81fb2`.

## Deviations from the spec

1. **`hitch push` gets no plan and no receipt.** The spec says "push if its
   effects benefit from structured receipts", and they do not. `push` has
   exactly one effect, it has no decision to explain (the user named the branch),
   and its whole risk surface is a single line: pushed or not. A receipt whose
   `effects` list holds one row and whose plan half says nothing would be a
   two-document restatement of `✓ Pushed 'dev' to origin` — the exact
   double-printing P7 removed. What `push` *does* get is the glyph fix
   (Constraint 10) and a `--json` refusal that says so rather than printing
   prose into stdout.

2. **Four new `OperationKind` variants would be a fifth kind per command, so
   `add`/`remove` and `lock`/`unlock` are one operation each, not two.** Spec
   §7.1 lists `AddEnvironment` and `RemoveEnvironment` separately because it
   lists *commands*; `Change`'s `kind()` already collapses promote/demote to two
   variants with one planner, and doing the same here is the same argument
   (two planners for a symmetric edit is two copies of the lock, the gate, the
   fingerprint and the `modify_metadata` that have to agree forever). The
   variants the spec names still all exist; they just come in pairs.

3. **`hitch remove --force` now removes instead of erroring.** This is a
   deliberate behaviour change, and it is the *point* of the migration: today
   `remove.rs:52-59` refuses an environment that has promoted branches and
   tells the user to pass `--force`, which is a confirmation prompt expressed as
   an error message — and `--force`'s own doc comment ("Skip confirmation
   prompt") describes a prompt that does not exist. With a plan, `--force` means
   what it says everywhere else in the CLI: do not ask. A branch-bearing
   environment now goes `exit 0` under `--force` where it went `exit 1` before.
   Task 11 pins both halves.

4. **`cleanup`'s dry-run stays a mode, not a flag change.** `cleanup` already
   defaults to a dry run, which is better than the `--dry-run` the other four
   commands use, and P8 does not flip a default that works. The plan is what a
   dry run renders, so the hand-written listing goes and `--apply` means "apply
   the plan you just saw" rather than "switch modes and print something else".

5. **`hitch approve`'s approval bookkeeping keeps its own narration.** A
   human approving a request is not a metadata mutation and has nothing to
   predict; §10.1 is about *mutations*, and the approval record is a fact, not a
   change to a declaration. What P8 removes is the narration of the *second*
   step — the declaration edit and its rebuild — which today has neither a plan
   nor a receipt and therefore narrates itself. The approval line stays; the
   `⏳ Locking` / `⏳ Updating` / `⏳ Rebuilding` / `⚠ Rebuilt … with N held`
   quartet does not.

6. **`PlanWarning` gained an optional `remedy`, and the rule is that only a
   *refusal* may fill it in.** `PlanApplyError::PolicyBlocked` prints
   "Nothing was changed. To proceed:\n  {remedy}" from `OperationKind::command_hint`,
   which is the right answer for a *stale* plan — re-run the command and it
   recalculates — and the wrong answer for a refusal, where the reader's options
   are something else entirely. A refused `hitch lock dev` was told "To proceed:
   `hitch lock dev`", the exact command that had just failed, under a heading
   asserting that it would work. The field is on the warning rather than at the
   raise site because the *planner* is the only half that knows the reader's
   real next move, and the two raise sites spelling out their own
   `remedy.unwrap_or(command_hint)` would be two places for a future warning
   kind to be added and silently inherit the wrong one.

   The rule that came out of the manual check, and the reason it is stated here
   rather than just implemented: **the default is right more often than not.**
   `declaration.rs`'s promote-conflict refusal deliberately does *not* override,
   because there the reason text already names the rebase and re-running the
   promote afterwards genuinely is the command to run. Two tests assert
   `hitch promote branch-b dev` in the remedy and failed on a well-meant
   override. A remedy override is for the case where the default names a
   command that *cannot* work — and getting it the wrong way round is worse
   than not having the mechanism, because the wrong override is a confident
   instruction where the default was merely imperfect.

   A remedy may be a sentence rather than a command, and the two refusals that
   need one both use it: `hitch unlock dev` on an unlocked environment gets
   "nothing to undo — 'dev' is already unlocked" (not `hitch lock dev`, which
   would answer a question the reader did not ask), and an unlock of someone
   else's lock gets "ask someone@else.com to unlock it" (no command helps, and
   one that failed is worse than none).

---

### Task 1 — The model: an optional projection pair, and a delete effect

**Files:** `src/operations/model.rs`, `src/core/render.rs`,
`src/operations/declaration.rs`

**Interfaces:**
- Consumes: nothing new. `OperationPlan<I>` is already generic over its detail.
- Produces: `OperationPlan.current` / `.proposed` as `Option<EnvironmentProjection>`;
  `PlannedEffect::LocalRefDelete { refname }`; `AppliedEffect::LocalRefDelete { refname, old }`;
  `PlannedEffect::refname()` and `AppliedEffect::refname()` gaining arms;
  `render_plan`'s three-arm projection rule.

- [ ] Write the failing renderer tests first, in `src/core/render.rs`'s test
  module (where `plan_headline`'s already are):
  - both projections `Some` and equal → the output contains neither
    `Current` nor `Proposed`;
  - both `Some` and different → both appear, `Current` before `Proposed`;
  - `current: None, proposed: Some(qa)` → `Proposed` appears and `Current` does
    not, and vice versa;
  - `None`/`None` → neither, and the plan still renders its `Will change` section.

- [ ] Change `OperationPlan`'s two fields (`src/operations/model.rs:600-603`) to
  `Option<EnvironmentProjection>`, each documented as: `None` means *this
  operation has no environment composition on that side* — which is a claim
  about create and destroy, not a gap. The four existing planners pass
  `Some(..)`. `declaration.rs:884` and `:896` become `.current.as_ref()`, with
  the `expect` naming which invariant broke, because a declaration plan's
  `current` is always `Some`.

- [ ] Replace `render_plan`'s unconditional loop
  (`src/core/render.rs:82-86`) with the three-arm rule. No prose is invented for
  the arms: the titles are the existing `"Current"` and `"Proposed"`, and the
  body is the existing `describe_projection`. A plan that renders no projection
  at all is a legitimate output, and `render_plan` is not allowed to apologise
  for it.

- [ ] Add `PlannedEffect::LocalRefDelete { refname: String }` and
  `AppliedEffect::LocalRefDelete { refname: String, old: String }` with
  `predicted_value()` returning `None` on the plan side (a deleted ref has no
  predicted value) and arms in `PlannedEffect::refname`
  (`src/operations/model.rs:331-340`) and `AppliedEffect::refname`
  (`:803`). `render_applied_effect` (`:949`) renders it as
  `✓ <name>` / `deleted` — the plan half says `delete <name>`, which is a
  different sentence from `publish <name> → <sha>`, so the two are never
  confused in a two-column table.

- [ ] Confirm `is_transient_anchor` (`src/core/render.rs:255`) is unaffected:
  `cleanup` deletes `refs/hitch/backup/*` and `refs/hitch/prev/*`, which are
  archives, not the `build/*`/`release/*` anchors. A cleanup plan must not list
  them under "Held only until the publish lands".

- [ ] Fix the three glyph defects on sight (Constraint 10): `push.rs:36`,
  `remove.rs:30`, `approvals/approve.rs:100-105`.

- [ ] `just format && just format-check && just lint && just test`.

---

### Task 2 — The vocabulary: seven kinds, six intents, every match total

**Files:** `src/operations/model.rs`, `src/core/render.rs`,
`src/operations/declaration.rs`

**Interfaces:**
- Consumes: Task 1's optional projections.
- Produces: the `OperationKind` variants spec §7.1 names and this phase needs;
  the matching `OperationIntent` variants; `plan_headline` arms for all of them.

- [ ] Add to `OperationKind` (`src/operations/model.rs:28-33`), in §7.1's order:
  `Lock`, `Unlock`, `SetEnvironment`, `AddEnvironment`, `RemoveEnvironment`,
  `Cleanup`, `ApprovalApply`. Keep the doc comment's promise — "an unmigrated
  operation is *absent* from the model, never mislabelled" — true: every
  variant added here is one a planner in this phase actually produces.

- [ ] Make `command_hint` (`:35-52`) total, with a per-variant remedy that is
  *the command the user ran*:
  - `Lock`/`Unlock` → `hitch lock {env}` / `hitch unlock {env}` (ignoring
    `argument`, as `Rebuild` does);
  - `SetEnvironment` → `hitch set {env}` (the flags are in the *plan*, and
    reprinting a guess at which flags the user typed is worse than naming the
    command);
  - `AddEnvironment`/`RemoveEnvironment` → `hitch add {env}` / `hitch remove {env}`;
  - `Cleanup` → `hitch cleanup --apply`, because that is the command that does
    the work and the plan is only ever previewed without it;
  - `ApprovalApply` → `hitch approve {argument}`. **This is the whole reason the
    variant exists** and it is load-bearing: the approval is already committed
    and the request's status is `Approved`, not `Applied`, so a re-run resumes
    exactly at the plan. A remedy of `hitch promote <branch> <env>` would send
    the user to re-authorise something they already authorised.

- [ ] Make `Display for OperationKind` (`:59-67`) total with one lowercase word
  per variant, matching today's `rebuild`/`promote`/`demote`/`release`. These
  words are the plan's JSON `"kind"` value and the head of
  `declaration.rs:529`'s plan `id`, so they are a stability surface: they are
  lowercase single words.

- [ ] Add to `OperationIntent` (`:77-97`):
  `LockEnvironment { environment }`, `UnlockEnvironment { environment }`,
  `UpdateEnvironment { environment, changes: Vec<EnvironmentFieldChange> }`,
  `AddEnvironment { environment, base }`,
  `RemoveEnvironment { environment }`,
  `Cleanup { branches: Vec<String>, archive_refs: Vec<String> }`,
  `ApprovalApply { environment, branches, request_id }`.

  `EnvironmentFieldChange` is **declared in `model.rs`**, not in
  `operations/metadata.rs`, because `OperationIntent` needs it and `model.rs`
  is the vocabulary sibling that cannot depend on a planner. Its variants:
  `Base { from, to }`, `RequiresApproval { from, to }`,
  `MinApprovals { from, to }`, `AddApprovers { emails }`,
  `RemoveApprovers { emails }`, `SetApprovers { from, to }`,
  `OnConflict { from, to }` — every one carrying a `from`, because an
  environment-setting plan that cannot say what it is changing from is a plan
  that cannot be reviewed.

- [ ] Make `Display for OperationIntent` (`:100-122`) total.

- [ ] Add `plan_headline` arms (`src/core/render.rs:169-194`) for all seven, and
  make its match total. Wordings:
  `Lock dev` · `Unlock dev` · `Update dev (2 settings)` ·
  `Add environment 'qa' (base 'main')` · `Remove environment 'qa'` ·
  `Cleanup 3 branches, 12 archive refs` · `Apply approved 'feat-a' → dev`.
  The `(N settings)` form names a count rather than a list because the list is
  the plan's `Will change` section, one line per field, and repeating seven
  branches in a headline is the width problem §17 is about.

- [ ] **Close the three wildcards in `declaration.rs`** (Constraint 6): `:271-274`
  (`verb`), `:447-450` (`verb_past`), `:474-477` (`because`). Each becomes a
  match over the new variants with a word chosen for the *approval* case — the
  approval's declaration change is a promotion or a demotion that a human
  already authorised, so the words are the promotion/demotion words and
  `ApprovalApply`'s own arms exist so a *remedy* can be right, not so the
  description can be vague.

- [ ] A test that `OperationKind`'s `Display` is exactly one lowercase word per
  variant, and that `command_hint` for every variant contains the substring
  `hitch ` and names the environment — so a new variant cannot be added with a
  remedy that is not a command.

- [ ] `just format && just format-check && just lint && just test`.

---

### Task 3 — `src/operations/metadata.rs`: four operations, four planners

**Files:** `src/operations/metadata.rs` (new), `src/operations/mod.rs`

**Interfaces:**
- Consumes: Task 1's `Option` projections; Task 2's kinds and intents;
  `PlanFingerprint`, `validate_plan`, `PlanApplyError`, `ExecutionReceipt`,
  `PlannedEffect`, `AppliedEffect` from `model.rs`.
- Produces: `MetadataPlanDetail`, `plan_lock`/`plan_unlock`, `plan_set_environment`,
  `plan_add_environment`/`plan_remove_environment`, `apply_metadata_plan`,
  `validate_metadata_plan`, `EnvironmentChange` (the add/remove direction enum).

- [x] **Tests first**, in `tests/integration/plan_apply_tests.rs`, and failing:
  - a lock plan's `fingerprint.refs` is **empty** and its `metadata_sha` is
    `Some`;
  - a lock plan's `current == proposed` and `compositions` is empty;
  - a lock plan's `effects` is exactly one `MetadataChange` naming
    `refs/heads/hitch-metadata` and the *resolved* user email;
  - `hitch set dev --add-approver a@b.c` where `a@b.c` is already an approver
    yields `detail.changes` empty and `outcome == NoChange`, and **writes no
    metadata commit**;
  - `hitch set dev --requires-approval true` on an environment with
    `min_approvals: 0` yields **two** changes — the flag's, and
    `MinApprovals { from: 0, to: 1 }` — because `set.rs:336-338` auto-sets it;
  - `hitch set dev --base release/1.x` where `release/1.x` is *also* a promoted
    branch yields a `Base` change **and** a `RemoveApprovers`-shaped collateral
    entry, because `set.rs:272-273` drops it from the list and today's preview
    does not mention it at all;
  - `hitch set dev --requires-approval true --add-approver not-an-email` on an
    environment with a pending request produces a non-blocking `PlanWarning`
    naming `hitch approvals list --status pending`;
  - `hitch add qa` produces `current: None` and a `proposed` of `qa = main`;
  - `hitch remove qa` produces `proposed: None` and a `current` of `qa = main`;
  - each planner's `id` is stable across two runs against an unchanged
    repository, and differs when `metadata_sha` differs.

- [x] `MetadataPlanDetail` — as built, in `src/operations/metadata.rs`. Two
  changes from the sketch above, both because the sketch was shaped for a
  family that does not exist:

  ```rust
  pub struct MetadataPlanDetail {
      pub environment: String,
      pub argument: String,
      pub edit: MetadataEdit,          // stated, not inferred
      pub changes: Vec<EnvironmentFieldChange>,
      pub branch_absorbed_by_base: Option<String>,   // singular: Option, not Vec
      pub locked_by: Option<String>,
      pub base: Option<String>,
  }
  ```

  - **The collateral is an `Option<String>`, not a `Vec` of a one-variant
    enum.** There is exactly one kind of collateral and it is singular — at most
    one promoted branch can equal the new base — so `Vec<CollateralChange>` was a
    shape with no second member, and a reader holding it would have to check
    whether emptiness meant "no collateral" or "a collateral kind not yet
    implemented". `min_approvals` is *not* collateral: it is a second
    `EnvironmentFieldChange`, and listing it twice is what the plan forbids.
  - **The detail carries a stated `MetadataEdit`, not a shape the executor
    infers.** The first draft inferred the edit from "are there changes?", and
    that is wrong in a way a test would have found late: an `unlock` carries no
    field changes, and so does `hitch set dev --min-approvals 1` against a
    threshold that was already 1. An executor that guessed would unlock on one
    and write settings on the other, both silently. A discriminator the plan
    *states* is worth more than one it leaves to be inferred.
  - **`branches` is gone.** The sketch justified it as letting the executor
    rebuild the projection's `proposed` side, but the projection is already in
    the plan, in `proposed` — so the field would have been a second copy of a
    value the plan already carries, readable by only one function, and one
    refactor away from a second source of truth.

- [x] `EnvironmentChange { Create { base }, Destroy }` — one enum, one planner,
  two entry points, mirroring `DeclarationChange`.

- [x] `plan_lock` / `plan_unlock` — one implementation over
  `LockChange { Lock, Unlock }`, because the two differ in exactly one boolean
  and in which refusal they raise. Neither takes a `ConfirmationRequirement`
  beyond `not_required()`: a lock is one keystroke, and §10.2 is explicit that a
  prompt is not implied by showing a plan. The plans are still *shown* (there
  are no silent `hitch lock`s in this CLI), which is `decide_gate`'s `Proceed`
  arm, not a new behaviour.

- [x] `plan_set_environment` — reads the environment once, computes the
  resolved `changes` list, and raises **refusals as
  `PlanWarning::policy_refusal`** rather than as `Err`, so that the reader sees
  the plan *and* the reason it cannot apply. The approval-config validation
  (`set.rs:334-343`) moves here from the closure: a decision the plan can make
  at plan time belongs in the plan.

- [x] `plan_add_environment` / `plan_remove_environment` — `create_or_destroy`
  over `EnvironmentChange`. The `remove` refusals that are *not* about
  confirmation (no such environment) stay `Err`; the two that are *about*
  confirmation (branches promoted, environment locked) become a plan plus a
  `ConfirmationRequirement::required(reason)`, per deviation 3.

- [x] `validate_metadata_plan` — the same body as
  `operations::rebuild::validate_plan` minus the resolution loop, with the
  environment and argument read off `plan.detail`. **Do not call
  `rebuild::validate_plan`**: it is typed to `RebuildPlanDetail` and its
  resolution arm would be dead weight here. If the duplication becomes three
  copies, extract then — not before.

- [x] `apply_metadata_plan` — one `modify_metadata` closure that applies
  `detail.changes` **verbatim** and re-reads the environment afterwards to
  assemble `resulting_state` and the `MetadataChange` `AppliedEffect`. No
  `discard` is needed: a metadata plan anchors nothing, which is the one thing
  `apply_rebuild_plan`'s `finally` exists to guarantee and the reason a metadata
  planner has no equivalent to forget.

- [x] Register `pub mod metadata;` in `src/operations/mod.rs:62`, and extend the
  module header's "One planner per operation" paragraph to say that a
  *metadata* operation's rollback story is Constraint 3's, so the next planner
  author does not copy promote's.

- [x] `just format && just format-check && just lint && just test`.

---

### Task 4 — `hitch lock` and `hitch unlock`

**Files:** `src/commands/lock.rs`, `src/commands/unlock.rs`,
`src/utils/command_helpers.rs`

**Interfaces:**
- Consumes: `plan_lock`/`plan_unlock`, `apply_metadata_plan`, `emit_receipt`.
- Produces: nothing new. Two commands that plan, apply and emit.

- [x] **Test first**: `tests/integration/lock_unlock_tests.rs` gains
  (a) `a_lock_shows_a_plan_and_a_receipt_and_names_the_locker` — the plan's
  `Will change` row and the receipt's effect row both name `hitch-metadata` and
  the harness identity; (b) `hitch lock dev --json` emits
  `{"schema_version", "plan", "receipt"}` with `plan.kind == "Lock"`; (c)
  locking an already-locked environment still exits 1 with the existing message
  and **no** plan printed, because the refusal happens before the plan exists;
  (d) `hitch unlock dev` by a non-locker still exits 1 and writes no commit.

- [x] `lock.rs::run` becomes: pre-checks → `plan_lock` → `emit_plan` (or
  `confirm_plan`, which is a no-`Ask` `Proceed` here) → `apply_metadata_plan` →
  `emit_receipt`. Delete `operation_info`/`operation_success` calls
  (`lock.rs:18`, `:26`) and `validation_start` (`:32`).

- [x] `unlock.rs::run` likewise. Delete `log_info("Unlocking environment …")`
  (`:15`) and `log_success` (`:23-26`).

- [x] Delete `operation_info`, `operation_success` and `validation_start` from
  `src/utils/command_helpers.rs:86`, `:81`, `:76` — `lock.rs` was their only
  caller. Keep `validation_success` (`:71`), which `release.rs:159` still uses,
  and note in its doc comment that it is a `--verbose` line for a pre-plan
  check, which is the only thing that is allowed to be one.

- [x] Manual check against a throwaway repo: `hitch lock dev`, `hitch unlock dev`,
  `hitch lock dev` twice, and the same four under `--json`.

  The manual check found three defects the tests could not have, all of them in
  the *refusal* path, which is why it is a required step and not a formality:

  1. **A blocked plan still listed the effect it was refusing.** The plan's
     `Will change` read `lock 'dev' held by test@example.com` directly above a
     refusal that it cannot lock. `plan_declaration_change` already had the rule
     — "Effects. Empty for a blocked plan, because none of them will happen"
     (`declaration.rs:531`) — and this module was the first planner to be
     written without inheriting it. All three planners here now branch on
     `blocked`.
  2. **`⛔` sat under a heading reading "Needs your decision."** True for an
     approval request; the opposite of true for a policy refusal, whose whole
     content is that no decision available to the reader will let it through.
     `render_plan` now picks the heading from `any(w.is_blocking())` — "Why
     this cannot apply" — rather than from where it was called.
  3. **The refusal's remedy was the command the reader had just run.** A
     refused `hitch lock dev` printed "Nothing was changed. To proceed: `hitch
     lock dev`" — the one move guaranteed to refuse identically, printed under
     a heading that says it will work. Fixed at the model: `PlanWarning` gained
     an optional `remedy`, set by the planner (the only half that knows the
     reader's real next move) and read through `remedy_or(command_hint)`. See
     the entry in the Deviations section.

  A fourth thing the check confirmed *is* right: the promote-conflict refusal
  in `declaration.rs` deliberately does **not** override its remedy, because
  there the default is correct — the reason text already names the rebase, and
  re-running the promote after it is genuinely the command to run. Two tests
  assert `hitch promote branch-b dev` and would have failed on an override;
  getting a remedy override the wrong way round is worse than not having the
  mechanism.

---

### Task 5 — `hitch set`

**Files:** `src/commands/set.rs`

**Interfaces:**
- Consumes: `plan_set_environment`, `apply_metadata_plan`, `confirm_plan`,
  `emit_receipt`, `decide_gate`.
- Produces: `--dry-run` and `--yes` on `SetCommand` (`src/cli.rs`'s
  `commands::set::SetCommand` registration is already there — the flags are
  added to the struct).

- [ ] **Test first**: `tests/integration/set_tests.rs` gains
  (a) `hitch set dev --base release/1.x --dry-run` prints a plan naming the
  branch that will leave the promoted list, and writes nothing;
  (b) `hitch set dev --json` **without** `--yes` exits 1 with the gate's
  refusal, exactly as the four existing mutating commands do;
  (c) `hitch set dev --requires-approval true` with a pending request names
  `hitch approvals list --status pending` in the plan and nowhere else;
  (d) `hitch set dev` with no flags still exits 0 and warns — unchanged, because
  "no changes specified" is a usage message, not a plan;
  (e) `on_conflict` renders as `eject` / `halt`, never as `Eject` / `Halt` —
  `set.rs:162-165` and `:208-212` currently print `{:?}` of the Rust enum, and
  the plan is where that debug formatting has to die.

- [ ] Delete `show_changes` (`set.rs:138-230`) entirely, glyphs and all. It is
  the exact thing `render_plan`'s `Current`/`Proposed`/`Will change` sections
  exist to replace, in a second vocabulary, and it hand-rolls a confirmation on
  the way.

- [ ] `run` becomes: pre-checks → no-flags check → `plan_set_environment` →
  `emit_plan` / `confirm_plan` → `apply_metadata_plan` → `emit_receipt`. The
  `modify_metadata` closure disappears from `set.rs` entirely; its
  `log_verbose("✓ …")` lines are the plan's business now.

- [ ] `--base` gets a second `PlannedEffect::MetadataChange` row for the branch
  it silently demotes. The auto-set `min_approvals` gets its own row. Both are
  consequences no flag names, which is exactly what a plan is for.

- [ ] `hitch set dev --base main` where nothing else changes must still leave
  `dev` needing a rebuild, and say so **once**: as a non-blocking plan warning
  naming `hitch rebuild dev`, and then as `resulting_state`'s
  `⧗ dev needs rebuild` in the receipt. Two documents, one job each — the
  master plan's receipt-warning contract, Constraint 4.

---

### Task 6 — `hitch add` and `hitch remove`

**Files:** `src/commands/add.rs`, `src/commands/remove.rs`

**Interfaces:**
- Consumes: `plan_add_environment`/`plan_remove_environment`, `confirm_plan`,
  `emit_receipt`.
- Produces: `--yes` on `RemoveCommand`; `--dry-run` on both.

- [ ] **Test first**: `tests/integration/add_remove_tests.rs` gains
  (a) `hitch remove dev` on an environment with a promoted branch now **shows a
  plan and asks**, instead of exiting 1 — the deliberate behaviour change of
  deviation 3; (b) declining writes nothing and exits 0; (c)
  `hitch remove dev --force` on the same environment exits **0** and removes it
  — the other half of deviation 3, pinned so the change cannot be reverted by
  accident; (d) `hitch remove dev --json` without `--yes` exits 1;
  (e) `hitch add qa` succeeds and its receipt's only effect is one
  `MetadataChange` naming `hitch-metadata`;
  (f) `hitch add dev` for an existing environment still exits 1 with the
  existing message.

- [ ] Delete `remove.rs:52-59`'s refusal and the stale comment above it
  ("In a real implementation, we would prompt for confirmation here / For now,
  we'll require the `--force` flag") — the second half of that comment stops
  being true in this task, which is why it is in this task.
  `remove.rs:62-68`'s locked-environment refusal becomes a plan
  `ConfirmationRequirement`, not an `Err`; `--force` still overrides it.
  Delete `add.rs`'s and `remove.rs`'s `log_info`/`log_success`/`log_verbose`
  narration and both `validate_preconditions` `log_verbose` lines.

- [ ] `add.rs`'s `--base` default of `main` and its `validate_base_branch_exists`
  stay where they are: a base that does not exist is a *pre-check*, and a
  pre-check that a plan would have to re-do is a second merge path in miniature.
  It belongs in the plan only insofar as the plan's `proposed` projection
  carries the base it will use.

---

### Task 7 — `src/operations/cleanup.rs` and `hitch cleanup`

**Files:** `src/operations/cleanup.rs` (new), `src/commands/cleanup.rs`,
`src/operations/mod.rs`

**Interfaces:**
- Consumes: Task 1's `LocalRefDelete` effect pair.
- Produces: `plan_cleanup`, `apply_cleanup_plan`.

- [ ] **Test first**: `tests/integration/cleanup_tests.rs` gains
  (a) `hitch cleanup` (no `--apply`) prints the plan and deletes nothing;
  (b) `hitch cleanup --apply` with one unmerged branch produces a receipt whose
  `effects` carry the deleted refs **and** a `LocalRefDelete` for the branch it
  could not delete, with the receipt's `warnings` holding exactly one
  `ExecutionWarning { owes_effect: true }` naming `git branch -D <name>`;
  (c) exit code stays 0 in that case — a partial cleanup is a success;
  (d) `hitch cleanup --json --apply` emits a well-formed document, which it
    cannot do today: `cleanup.rs:129` and `:141` `println!` branch and ref names
    straight to **stdout**, so a `--json` run today interleaves prose into the
    stream. Both `println!`s are deleted;
  (e) the `state/` ref is never in the plan's delete list —
    `test_cleanup_does_not_prune_the_build_record_ref` must keep passing and
    the new plan must be checked against it.

- [ ] Move the candidate collection and `stale_archive_refs` from `cleanup.rs`
  into the planner **verbatim**, including `ARCHIVE_REF_RETENTION` and the
  `prunable` set semantics. A planner that re-derives "which branches are
  prunable" from a second place is how the `state/` exclusion was nearly
  wrong once already. `commands/cleanup.rs` becomes argument parsing plus
  orchestration, and its `envs_in_scope` and `stale_archive_refs` go with it.

- [ ] `plan_cleanup` fingerprints every ref it is about to delete (they are
  inputs to the decision, so a stale plan must notice they moved) **and**
  `metadata_sha`, because "is this branch promoted" is a declaration question.
  The archive refs are ordered chronologically (fixed-width timestamp suffix,
  `cleanup.rs:178`) — preserve that order, and document that two cleanups of the
  same repository must produce the same plan.

- [ ] `apply_cleanup_plan` deletes branch by branch and ref by ref, recording an
  `AppliedEffect::LocalRefDelete` per success and a single owed warning per
  failure. A failure is **not** an `Err`: a cleanup that deleted 11 of 12 has
  done most of its work, and aborting the rest would make it retry the same 11.
  This is the only place in P8 where `receipt.warnings` is legitimately
  populated, and the test in (b) is what keeps it that narrow.

- [ ] `--apply` keeps its meaning; `--yes` is added for the question. A
  `cleanup --apply` that deletes 40 branches should ask.

---

### Task 8 — `hitch approve`: a plan for the promotion, and two dead rollbacks

**Files:** `src/commands/approvals/approve.rs`,
`src/operations/declaration.rs`, `src/utils/rollback.rs`, `src/types.rs`

**Interfaces:**
- Consumes: `plan_declaration_change` via a new `DeclarationChange` variant;
  `apply_declaration_plan`; `emit_receipt`.
- Produces: `DeclarationChange::ApprovedApply { branches, request_id }`; the
  deletion of `attempt_approval_rollback`, `attempt_operation_rollback`,
  `RollbackInfo.previous_state`, and the last `StepNarration::Log` call site.

- [ ] **Test first**, in `tests/integration/approval_workflow_tests.rs`:
  (a) `an_approved_promotion_shows_a_plan_before_it_applies` — the plan's
  `kind` is `"ApprovalApply"`, its `intent` names the request, and its
  `Will change` row describes the promotion;
  (b) `an_approval_that_cannot_be_built_still_exits_zero` — the request reaches
  `Applied`, the branch is in the declaration, the environment is unbuilt, and
  the receipt carries one owed warning naming `hitch rebuild <env>`;
  (c) `a_failed_approval_writes_no_rollback_commit` — **the test that pins
  Constraint 3.** Force the approval path to fail (a request whose
  `rebuild_snapshot` no longer validates, say) and assert that `hitch-metadata`'s
  commit count is **unchanged** by the failure. Today it moves by one, because
  `attempt_approval_rollback` writes the config it already had;
  (d) `a_receipt_never_restates_a_plan_warning` still passes with the approval
  path added to the operations it sweeps.

- [ ] Add `DeclarationChange::ApprovedApply { branches: Vec<String>,
  request_id: String }`. `branches()` handles it like `Add`/`Remove`;
  `kind()` returns `OperationKind::ApprovalApply`; the planner's `match` on
  `change` (`:297-319`, `:354-370`, `:400`, `:532-540`) handles it by
  delegating to the `Add`/`Remove` arms, with two differences: the
  already-promoted check is a `log_verbose` rather than a refusal (a re-run
  after a partial apply is legal, and erroring would wedge it), and the
  approval gate is **not** applied, because the approval *is* the gate — asking
  for approval of an already-approved change is how a request deadlocks.

- [ ] Make `plan_declaration_change` reachable for it: a `pub` entry point
  `plan_approved_declaration_change(context, environment, change, argument,
  options, on_step)`. **Do not** add an `OperationKind` parameter — the kind
  comes from `change.kind()`, and a parameter that can disagree with the change
  is a second source of truth for the same fact.

- [ ] `execute_approved_operation` (`:217-297`) becomes: `with_locked_env` →
  `modify_metadata` recording the request as `Applied` → **return** →
  `plan_approved_declaration_change` → `confirm_plan` → `apply_declaration_plan`
  → `emit_receipt`. The plan is built *outside* the lock's closure on purpose
  (the AGENTS.md entry on `modify_metadata`'s closure running before its commit),
  and *inside* `with_locked_env` for a different reason: a declaration plan's
  fingerprint includes `metadata_sha`, and the approval's own commit is a
  `hitch-metadata` write, so a plan built before it would be stale the moment
  `validate_declaration_plan` looked. The two requirements are compatible
  because the approval commits inside the lock and the plan is built after that
  commit returns, still inside the lock.

- [ ] `rebuild_after_approval` (`:376-416`) is deleted. Its `StepNarration::Log`
  call (`:388`) is the last one in the codebase; its `⚠ Rebuilt … with N held`
  warning (`:391-403`) is superseded by the nested rebuild's effect carrying
  `held` on the receipt; its error arm (`:406-414`) is superseded by
  `apply_declaration_plan`'s own contract, which this path now shares verbatim.
  The `StepNarration::Log` variant then has no caller: delete the variant and
  the parameter, and record in the master plan that the enum exists solely to
  keep a nested rebuild quiet.

- [ ] Delete `attempt_approval_rollback` (`:418-431`) and
  `attempt_operation_rollback` (`:433-446`) — byte-identical but for one
  `log_warning` string. Their `log_info("✓ Restored previous environment
  state")` (`:427`, `:441`) fires **inside** a `modify_metadata` closure, i.e.
  before the commit it is announcing, and announces a restore of a value the ref
  already holds. Both `Err` arms that call them (`:205-213`, `:286-295`) become
  a plain `Err(e)`, so `main` reports the cause once.

- [ ] `RollbackInfo.previous_state` (`src/types.rs:175`, initialised at `:192`)
  now has no reader and no writer: delete the field and the two `#[cfg(test)]`
  tests in `src/utils/rollback.rs:116-130` that exercise it. `capture_config_state`
  and `rollback_metadata_changes` keep their promote/demote callers.

- [ ] Delete the `⏳ Locking` / `⏳ Updating environment metadata` /
  `⏳ Rebuilding environment` lines (`:240`, `:247`, `:378`) and the
  `✓ Environment '{}' locked` / `unlocked` pair (`:244`, `:282`). Keep the
  approval record's own lines — deviation 5.

- [ ] `just format && just format-check && just lint && just test`, then a
  manual approval walkthrough in a throwaway repo (the recipes are in
  `AGENTS.md`'s repro list: `hitch set dev --requires-approval true
  --add-approver <a second email>`, then `hitch promote` → `hitch approvals
  approve`).

---

### Task 9 — `--json`, `--yes`, and the flag inventory

**Files:** `src/cli.rs`, `src/commands/lock.rs`, `src/commands/unlock.rs`,
`src/commands/set.rs`, `src/commands/add.rs`, `src/commands/remove.rs`,
`src/commands/cleanup.rs`, `src/commands/approvals/approve.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: one accurate `--json` doc comment, and the flags each command needs.

- [ ] Add `--yes` and `--dry-run` where the plan needs them (`set`, `add`,
  `remove`, `cleanup`) and neither where it does not (`lock`, `unlock`).
  `ApproveArgs` gets `--yes`; `--dry-run` on `approve` is **not** added,
  because an approval is a human act and a preview of a human act is a way to
  make it look mechanical.

- [ ] Update the `--json` doc comment (`src/cli.rs:38-52`). It currently names
  six commands; P8 takes it to eleven. Rewrite the *sentence* too: "A command
  that does not support it says so rather than printing prose into a stream the
  caller is about to parse" is only true because under `--json` the
  `DiagnosticOutputSink` sends every `log_*` to stderr. `cleanup`'s two raw
  `println!`s are the exception that made the sentence worth auditing, and
  Task 7 removes them — so after P8 every non-supporting command really does
  emit an empty stdout.

- [ ] A test that walks the clap tree and asserts the `--json` doc comment's
  command list matches the set of commands whose `run` reaches `emit_json` or
  `emit_plan`. This is the sentence the comment already promises to be
  "asserted in prose deliberately", and prose is not a test.

- [ ] A `--json` run of each of the seven commands emits parseable JSON.
  Anything that does not is a `println!` that escaped the sink, and the grep for
  `println!` outside `render.rs` and `completion.rs` is the test.

---

### Task 10 — Tests

**Files:** `tests/integration/lock_unlock_tests.rs`,
`tests/integration/set_tests.rs`, `tests/integration/add_remove_tests.rs`,
`tests/integration/cleanup_tests.rs`, `tests/integration/approval_workflow_tests.rs`,
`tests/integration/plan_apply_tests.rs`, `tests/unit/metadata_plan_tests.rs` (new)

**Interfaces:**
- Consumes: all of the above.
- Produces: the suite that holds P8.

- [ ] `tests/unit/metadata_plan_tests.rs`: pure-function tests over plans built
  from a hand-written config — every `OperationKind` renders a headline, every
  `OperationIntent` renders a `Display`, every `EnvironmentFieldChange` has a
  `from`, and a plan's `id` is deterministic for a fixed `(kind, environment,
  argument, digest)`.

- [ ] Extend `a_receipt_never_restates_a_plan_warning` to sweep **all seven**
  operations rather than three. This is the test the P7 contract was written
  for, and a fourth through seventh planner is exactly the case that proves it
  generalises.

- [ ] One golden-output test per new command, asserting the *rendered* plan and
  receipt strings, not just their structure — the point of `render.rs` being
  the only word-chooser is that a word change is visible in a diff.

- [ ] Extend the exit-code inventory into a single test that asserts all of
  them in one place, so a future change to one cannot silently move another:
  `lock`/`unlock` success 0 · lock-already-locked 1 · unlock-not-locked 1 ·
  unlock-by-non-locker 1 · `set` no-flags 0 · `set` declined 0 · `set --json`
  without `--yes` 1 · `set` invalid-approval-config 1 · `add` success 0 ·
  `add` existing 1 · `remove` declined 0 · `remove --force` 0 · `cleanup`
  partial 0 · `cleanup --json` without `--yes` 1 · `approve` recorded-not-
  threshold 0 · `approve` applied 0 · `approve` applied-but-unbuilt 0.

- [ ] The teeth check, done the way P7's were: reintroduce each defect this
  phase fixes, confirm the named test fails, revert. The defects worth the
  ceremony are the no-op rollback commit (Task 8c), the restored `Current`/
  `Proposed` pair on a lock (Task 1), and the `set --base` collateral branch
  removal going unmentioned (Task 5).

---

### Task 11 — Manual verification, gates, and documentation

**Files:** `AGENTS.md`,
`docs/superpowers/plans/2026-09-25-explainable-ux-program.md`, this file.

**Interfaces:**
- Consumes: everything above.
- Produces: the documentation that would otherwise be wrong next week.

- [x] Build the **debug** binary (`cargo build -p hitch`, not `just build`) and
  drive all seven commands against a throwaway repo in `/tmp` with
  `--yes --no-push`: `lock`/`unlock` including both refusals; `set --base` with
  and without a promoted-branch collision, with and without `--dry-run`; `add`
  then `remove` then `remove --force`; `cleanup` with a clean tree, with an
  unmerged branch, and with `--json`; and the full approval round trip. Read the
  output as a user would: the question is whether each one is a *plan* and a
  *receipt*, or still a narration with a receipt bolted on.

- [x] Gates, in order, all three clean: `just format`, `just format-check &&
  just lint`, `just test`.

- [x] `AGENTS.md`: in the architecture map, add `src/operations/metadata.rs` and
  `src/operations/cleanup.rs`; correct the `src/operations/` entry's "three
  mutating commands share one shape" to seven; record the new gotchas this phase
  produced — at minimum, "a metadata operation has no rollback, because its
  whole effect is one closure that runs before its commit", and "a
  `DeclarationChange` variant is how a fifth direction joins an existing
  planner, not a fifth parameter". Fix on sight anything the diff made stale:
  the `StepNarration::Log` line in the `src/utils/prelude.rs` entry, the
  `OperationKind` variant list in the `src/operations/` entry, and the
  "six commands honour `--json`" list in the `src/cli.rs` entry.

- [x] The master plan: move P8 to **COMPLETE** with the test count and the
  deviations this plan recorded, add the commit list entries, and update "Where
  this work lives" plus the `P0–P7 are complete. P8 is next.` line at the top.
  P9's "What P8 hands P9" is authored **at** P9, not now, for the reason in the
  authoring note: its `file:lines` would be stale before it was read.

- [x] Add an `## As executed` section to this file recording what actually
  landed, every deviation from the plan above, and anything P9 inherits.

---

## As executed

**Commits:** `3b02105` (Tasks 1–2), `ef4f4ef` (3–4), `a32df5b` (5), `7348d7e`
(6), `5593aa0` (7), `2f9174c` (8), `33ea3f5` (9), `e217e6d` (10), `289c63f` and
`dd9aff7` (11 — the second is titled "Task 12" in error). Suite at completion:
169 lib + 577 integration + 1 `no_args_help`, all green; fmt and clippy clean.

**Tasks 1–10 landed as planned**, with deviation 6 (`PlanWarning.remedy`)
added during execution and recorded above. Task 11's manual walkthrough is
where the phase earned its keep: the suite was green and the CLI still had nine
defects, two of them serious. All fixed in `289c63f`, test-covered:

- **`approve --json` without `--yes` wrote before refusing** — the vote and a
  lock/unlock pair were committed, then the gate refused. The refusal now runs
  before any write (and, per the review, *after* the read-only lookups, so an
  already-applied request says so rather than "needs `--yes`").
- **`cleanup` planned deletions that `git branch -d` then refused**, listed them
  under Applied, re-listed them as ⧗ owed, and exited 0. The planner now uses
  git's own predicate (`GitOperations::branch_is_merged`: ancestor of upstream,
  else `HEAD`); an unmerged branch is *kept* with an advisory naming `git branch
  -D` for the human. HEAD and the upstream are in the fingerprint. A delete that
  still fails at apply time is an `ApplyFailure`, reported after the receipt
  with exit 1 — never "owed", because nothing will retry it.
- `set --base`'s Current line showed the proposed base; the effect now names
  `old → new`. `set`/`remove`/`cleanup` dry-runs end with one preview note
  (`emit_preview_note`).
- `remove --force` still headlined the promoted-branch fact as "Needs your
  decision". The warnings heading is now "Needs your decision" only when
  confirmation or approval is actually required, else "Worth knowing" — a
  change visible to every plan with advisories and no gate.
- `cleanup`'s receipt printed a Result block of every environment; it has no
  `resulting_state` now. `--json` always emits one document, including when
  there is nothing to clean.
- `approve` still narrated (`Approving request…`, `Fetching…`, `Request found`,
  blank `ℹ️` lines, `applied successfully!`). Gone; the one line left is a vote
  that did not meet the threshold, which has no plan to carry it.
- A refusal printed its cause in the plan and again in `PolicyBlocked`'s
  `Error:`. The cause is now said once, in the plan. **This amends deviation
  6:** a refusal that only means "already so" (`unlock` of an unlocked
  environment, `add` of an existing one) is `PlanWarning::with_nothing_to_do`,
  and `PolicyBlocked.remedy` is an `Option` — there is no `To proceed:` line
  followed by a non-action.

An independent review of `289c63f` found four more, fixed in `dd9aff7`:
`list_local_branches_with_prefix` leaked git's `+ ` worktree marker into
branch names (cleanup advised `git branch -D + feat`), so it now reads
`for-each-ref`; a branch checked out in any worktree is kept and named;
cleanup deletes through `delete_branch_strict` (plain `-d`, never the
worktree escalation to `-D --force` / `update-ref -d`); `approve --json --yes`
below the threshold emits `{schema_version, plan: null, receipt: null,
approval: {…}}` rather than nothing; and a failed delete appears in the JSON
document as `failures`, not only on stderr.

**Deviations from the tasks as written:** `emit_approval_recorded` /
`render_approval_recorded` and `emit_receipt_with_failures` are new emitters the
plan did not name, both in `render.rs` per the words-in-one-place rule.
`apply_cleanup_plan` returns `CleanupRun { receipt, failures }` rather than a
bare receipt. `StepNarration::Log` has no caller left, but it was **not**
deleted: removing it means removing the `on_step` plumbing through
`plan_rebuild` / `apply_rebuild_plan` / `compose_environment` and their tests,
which is P10's legacy removal, not this phase.

**What P9 inherits** (authored in full at P9, per the authoring note):

- Dead `StepNarration` / `on_step` plumbing, above.
- `with_locked_env` still prints `Environment 'x' locked by…` / `unlocked`
  around every plan — a narration voice P8 did not touch.
- Seen in passing, not P8's: an approval-gated `promote` says "requires
  approval" once before its plan and again inside it, and its plan shows ⛔
  "Why this cannot apply" before going on to file the request — a blocking
  glyph on an outcome that is not a refusal. `promote` of a missing or
  already-promoted branch prints only the lock/unlock lines around its error.
- `GitOperations::delete_branch`'s worktree escalation still exists; its only
  remaining callers force-delete hitch's own temp branches in `resolve.rs`, so
  it is not reachable with user work today.
- `remove --force --json` without `--yes` still exits 1: the `--json` gate
  refuses for every command regardless of `--force`. Left as is; `--force`
  answers the question, `--yes` authorises answering none, and conflating them
  is a decision for the flag inventory, not a fix.

