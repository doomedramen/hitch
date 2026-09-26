# P6 — CLI renderers, `--json`, and `--dry-run` everywhere

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A plan shown before the mutation and a receipt shown after it, in one shared vocabulary; a global `--json` that emits a versioned document with stdout reserved for it; and `--dry-run` on every mutating command, rendered by the same planner the real run uses.

**Architecture:** One renderer module, `src/core/render.rs`, holding *pure* functions over `OperationPlan<I>` and `ExecutionReceipt`. It is the only place in the codebase allowed to choose words. The reason one function suffices for four operations is that the model *is* the vocabulary: `intent` names the operation, `current`/`proposed` are the projections, `compositions` carries per-branch state, `effects` is the change list, `unaffected` is the "will not change" list, and `warnings`/`confirmation` are the refusals. No per-operation renderer is needed because no per-operation *data* is needed — `detail` is the one field a shared renderer ignores. `--json` is `Serialize` derived on those same types plus a `schema_version`, not a hand-mirrored DTO (deviation 1). The confirmation gate is one helper, and it is the only thing that turns a plan into a mutation.

**Tech Stack:** Rust, `serde`, `serde_json`, `clap`, `anyhow`, `colored`. **No new dependencies** — `serde`/`serde_json` are already in `Cargo.toml` (`EnvironmentBuildRecord` is a persisted JSON document).

**Spec section:** §10.1 (mutating commands show a semantic plan), §10.2 (`--yes` behaviour), §10.3 (`--json`), §10.4 (dry-run semantics), §17 (human terminology), §18 (semantic vs diagnostic channels), §30.4/§33 (render human guidance at the edge). M5.

---

## Global Constraints

- **`file:lines` refs below were resolved against `9247a46` on `explainable-ux`.** Re-resolve if the tree has moved.
- **Scope every cargo invocation to `-p hitch`.** A bare `cargo build`/`check`/`clippy`/`test` compiles the whole workspace including the broken `hitch-desktop` crate.
- **`crates/` is untouched.** Standing check at the end: `git diff --name-only main..HEAD -- crates/` is empty.
- **The renderer is pure and total.** `render_plan` / `render_receipt` take a value and return a `String`. No `GlobalContext`, no `Result`, no git, no clock. This is the P3 `build_status_model` rule applied to plans: a renderer that can open a repository is a renderer that can disagree with the thing it is rendering. The only impure things in this phase are the two emitters (`emit_human`, `emit_json`) and the confirmation gate, and they take the already-rendered `String`.
- **A renderer never derives a verdict.** Every "will hold" / "still owed" / "changed since this plan was calculated" claim is read from the model (`plan.compositions[].branches[].state`, `receipt.warnings[].owes_effect`, `receipt.outcome`). If a word cannot be sourced from a field, it does not get said. The regression test is `the_renderer_never_mentions_a_thing_the_model_does_not_say`.
- **The terminology table (§17) is binding, and this module is the only place it is applied.** "held" not "ejected", "desired-state change" not "metadata mutation", "commit" not "SHA"/"OID" in prose, "changed since this plan was calculated" not "CAS", "publish" for a remote write. Short SHAs are allowed and expected (§17 says *show short SHA where useful*).
- **`--json` reserves stdout.** Every `log_*` call, every `StepLogger` line, and every error message goes to stderr in JSON mode, so a consumer can pipe stdout into `jq` unconditionally. Anything that would print to stdout on a P6 path must be routed through the sink. The guarantee is per-command and the list of commands honouring it is stated in the `--json` flag's doc comment (deviation 2).
- **`--yes` skips the prompt, never the plan** (§10.2). A future `--quiet` may suppress the *human* rendering; `--json` is not that flag and neither is `--yes`. Under `--yes` the plan is still printed.
- **Exit codes are unchanged.** P6 adds output and prompts, not outcomes. `rebuild` held → 2, `--dry-run` would-hold → 2, policy refusal → 1, conflict → 1, declined confirmation → 0, approval requested → 0, failed dependent rebuild → 0. The one genuinely new exit path is `promote/demote/release --dry-run`, which is 0 on success and 1 on refusal.
- **A dry run must not take a lock and must not write anything.** This is the one place the P5 "plan inside the lock" rule is deliberately inverted, and the inversion is safe for one specific reason: a preview plan is never applied, so nothing can be stale. `--dry-run` therefore plans *outside* `with_locked_env`. The planner not checking `is_locked()` (a P5 finding) is what makes this expressible.

---

## Deviations from the spec

1. **`--json` serialises the model directly; there is no separate JSON DTO.** A hand-mirrored mirror of ~15 structs and enums is a second source of truth for the same run, and a wire format that drifts from its own model is worse than prose that drifts. The spec's own answer to evolution is `schema_version: 1` (§10.3), and that only helps a consumer if the *shape* is knowable — so the stability guarantee is made checkable instead: a test pins the exact top-level key set and the exact variant names of every enum that appears in the document, and a `schema_version` bump is the only sanctioned way to change them. `ExecutionReceipt` already carries a `RepositoryStateSnapshot`, which brings `core::state` into the document — deliberate, and the reason the test pins keys rather than a full golden blob.
2. **`--json` is honoured per-command, not universally, in this phase, and the four commands are exactly the ones this phase rewired.** `rebuild`, `promote`, `demote` and `release` get it. `status` is *not* in this phase even though it is a natural fit — it is a read-only command with no plan and no receipt, and giving it `--json` is P7's material (P7 is the phase that teaches `core::state`'s snapshot to explain itself with no mutation behind it). The rest of the CLI still has bare `println!` in `commands/{tree,cleanup,resolve}.rs` and in the `log_macros!` family, and converting all of those is P8's. The flag's doc comment names the four, and a command without JSON support under `--json` says so rather than silently emitting prose. Asserting the list in the doc comment is what makes this honest rather than a silent partial.
3. **No `--quiet` flag.** §10.2 says "a *future* `--quiet` may suppress human plan rendering". Adding it now would be a second way to suppress output alongside `--json`, and two suppression flags is one too many for a phase whose whole job is one rendering path. Recorded as deliberately out of scope, not overlooked.
4. **The shared renderer replaces `rebuild --dry-run`'s bespoke prose, and `format_held_report` survives as the *effect* of a `Held` state rather than as a separate formatter.** §10.4 says dry-run "should become a renderer over the same rebuild plan" — P1 made it a renderer over the same *planner* but it still had its own words. P6 gives it the shared words. The `git checkout … && git rebase …` remedy per held branch is the one thing the shared renderer cannot derive, because it is a per-conflict *instruction* not a per-branch *state*; it is rendered from `PlannedBranchState::Held { conflicts_with, files }`, which does carry both, so the shared renderer does produce it. `format_held_report` is then dead and is deleted — after its three assertions in `rebuild_tests.rs` have been re-pointed at the shared wording.

---

## Interfaces

- **Consumes:** `OperationPlan<I>` / `ExecutionReceipt` / `OperationOutcome` / `PlanWarning` / `PlannedBranchState` (P4/P5), `Cli` (`src/cli.rs`).
- **Produces:**
  - `src/core/render.rs` — `render_plan<I>(&OperationPlan<I>) -> String`, `render_receipt(&ExecutionReceipt) -> String`, `plan_headline(&OperationPlan<I>) -> String`, `decide_gate(bool, bool, bool) -> GateDecision`, `confirmation_question(&ConfirmationRequirement) -> String`, `confirm_plan(&GlobalContext, &str, &ConfirmationRequirement) -> Result<bool>`, `emit_json<T: Serialize>(&T) -> Result<()>`, `emit_plan`, `emit_receipt`, `JSON_SCHEMA_VERSION: u32`, `JsonDocument`.
  - `GlobalFlags { verbose, no_push, assume_yes, json }` replacing the positional-bool constructors.
  - `DiagnosticOutputSink` in `src/utils/output.rs` — every level to stderr, no colour.
  - `--dry-run` on `PromoteCommand`, `DemoteCommand`, `ReleaseCommand`.

---

### Task 1 — `src/core/render.rs`, the plan renderer

**Files:** `src/core/render.rs` (new), `src/core/mod.rs` (register).

**Interfaces:** `pub fn render_plan<I>(plan: &OperationPlan<I>) -> String`; `pub fn plan_headline<I>(plan: &OperationPlan<I>) -> String`.

- [x] Write the unit tests first, against hand-built `OperationPlan` values (the struct is all-public and `RebuildPlanDetail` has four `Default`-able fields plus a `record`, so a fixture is a dozen lines and needs no repository). The claims to pin:
  - The headline is derived from `intent`, one phrasing per variant: `Promote a, b → dev`, `Demote a → dev`, `Rebuild dev`, `Release dev → main`. A test per variant, because a headline that guesses is the first thing a user reads and there are only four.
  - `Current` and `Proposed` render as `env = base + b1 + b2` in declaration order — **never sorted**, since order is composition order (`EnvironmentProjection::branches` says so). A test that builds a projection as `[z, a]` and asserts the rendered line reads `+ z + a` is the guard.
  - `Composition` renders one line per `PlannedBranch` with a distinct glyph per `PlannedBranchState`: `Included` ✓, `ReplayedResolution` ♻️ (naming the resolution id), `AlreadyInBase` =, `Held` ⛔ with `because <conflicts_with>` and the file list, `Missing` ⚠️ with the words "accounted for nowhere" — a state that cannot occur today, and if it ever does a renderer that renders it as `Included` is a lie.
  - `Will change` renders one line per `PlannedEffect`, one phrasing per variant, and the ref name is shortened to its last path segment for display (`refs/heads/dev` → `dev`) while the full refname is still what the *plan* carries. `PromotionPrune` names the branches it removes; `DependentEnvironmentRebuild` names the `because` clause; `TagCreation` names the tag; `RemoteRefUpdate` is rendered as "publish rebuilt <name>" per §10.1.
  - `Will not change` lists `unaffected` by name. A plan with an empty `unaffected` omits the heading entirely rather than printing a heading with nothing under it.
  - Every `PlanWarning` renders, and the two blocking kinds are visually distinct from `Advisory` — the distinction is `kind.is_blocking()`, never a string match on the message.
  - `ConfirmationRequirement` is not rendered as a line; it is the caller's cue. A plan that requires confirmation and is rendered by `render_plan` still renders identically to one that does not, because whether to ask is `confirm_plan`'s question and not the renderer's.
- [x] Implement to make them pass, in this section order: headline, `Current`, `Proposed`, `Composition`, `Will change`, `Will not change`, `Needs your decision` (the warnings). Blank line between sections; two-space indent for entries; the whole thing one `String` with no trailing newline (the emitters add it).
- [x] Add the "never mentions a thing the model does not say" test: build a plan, render it, and assert every `format!("{sha}")` short SHA in the output appears in the plan's own SHAs, and that no `⚠`/`⛔` glyph appears when no branch is `Held`. This is the test that catches a renderer inventing a warning.
- [x] `mod render;` in `src/core/mod.rs`. No re-exports — callers import the path, so the module boundary stays visible.

### Task 2 — the receipt renderer, and the owed/succeeded split

**Files:** `src/core/render.rs`.

**Interfaces:** `pub fn render_receipt(receipt: &ExecutionReceipt) -> String`.

- [x] Unit tests, hand-built receipts:
  - The headline is the outcome, one phrasing per `OperationOutcome` variant: `Applied` → "Applied", `AppliedWithHolds` → "Applied with holds" (**never** plain "Applied" — the two carry different CI meaning and collapsing them is the exact bug the enum exists to prevent), `ApprovalRequested` → "Waiting for approval", `NoChange` → "Already up to date".
  - `✓` lines come from `effects`, and only from `effects`. An effect that did not happen must not be able to render as one, because `effects` is the observation.
  - **Owed effects are rendered in their own section, after the applied ones, and never as ✓.** `ExecutionWarning { owes_effect: true }` is neither a failure nor a success, and a receipt that files it under warnings with the same glyph is the bug. The section is named for what it is — still owed — and each line is the warning's own message, which already names its remedy (`hitch push <env> -f`, `hitch rebuild <env>`).
  - Plain `warnings` render as `⚠` in place, interleaved with the effects in the order the receipt lists them.
  - `resulting_state` is read, not recomputed: a receipt whose snapshot says an environment `NeedsRebuild` renders the words "needs rebuilding" and does not say "up to date". A test with a `LegacyUnknown` environment renders "unknown — no build record" rather than silence, because P3 made that a first-class state and a renderer that skips it is a renderer that reports no news as good news.
- [x] Implement. `resulting_state` handling goes through `core::state`'s own classification, never a timestamp or a re-read of the refs — the rule P3 established for `commands/status.rs`, and the reason this task is small is that `build_status_model` already does the reading.

### Task 3 — `GlobalFlags`, `DiagnosticOutputSink`, and the emitters

**Files:** `src/cli.rs`, `src/main.rs`, `src/commands/global_context.rs`, `src/utils/output.rs`, `src/core/render.rs`.

**Interfaces:** `GlobalFlags`, `GlobalContext::new(GlobalFlags, Arc<Logger>)`, `GlobalContext::new_at_path(&str, GlobalFlags, Arc<Logger>)`, `DiagnosticOutputSink`, `emit_json<T: Serialize>`, `JSON_SCHEMA_VERSION`.

- [x] `GlobalFlags { pub verbose: bool, pub no_push: bool, pub assume_yes: bool, pub json: bool }` in `global_context.rs`, with `GlobalFlags::defaults()` and a `#[cfg(test)] GlobalFlags::for_tests()` (the `{false, true, true, false}` shape ~15 unit-test call sites already spell out). Replace the positional bools in both constructors. Update all call sites — `main.rs:78`, `core/workspace_index.rs:211`, `utils/{prelude,publish_journal}.rs`, and the test helpers in `tests/integration/plan_apply_tests.rs` / `state_model_tests.rs`. Add `pub json: bool` to `GlobalContext`.
  Rationale for the struct rather than a fifth positional bool: four `bool`s in a row is a call site where `new_at_path(p, false, true, true, false)` and `new_at_path(p, true, true, true, false)` differ by one character and mean different things. All 15 existing call sites were the same four values, so `for_tests()` also *shrinks* them.
- [x] `--json` in `cli.rs` as `#[arg(long, global = true)]`, whose doc comment **names the four commands that honour it** and says a command without support says so rather than printing prose (deviation 2).
- [x] `DiagnosticOutputSink` in `utils/output.rs`: every level to `eprintln!`, no `colored` glyph, plain text. `main.rs` selects it and calls `colored::control::set_override(false)` when `--json` — an ANSI escape inside a JSON string is not "pretty", it is a parse hazard in some consumers and a display artefact in all of them.
- [x] `emit_json<T: Serialize>(value: &T) -> Result<()>`: `serde_json::to_string_pretty` → **one** `println!` → explicit `stdout().flush()`. The flush is load-bearing for the same reason `main.rs:127`'s is: `process::exit(2)` on a held rebuild skips normal shutdown, and a CI job that loses its JSON because the process exited is the failure this whole flag exists to prevent.
- [x] Derive `Serialize` on the model types the document contains: `OperationPlan`, `EnvironmentProjection`, `CompositionPlan`, `PlannedBranch`, `PlannedBranchState`, `UnaffectedResource`, `ResourceKind`, `PlannedEffect`, `PlanWarning`, `PlanWarningKind`, `ConfirmationRequirement`, `OperationKind`, `OperationIntent`, `PlanFingerprint`, `ExecutionReceipt`, `AppliedEffect`, `ExecutionWarning`, `OperationOutcome`, and the `core::state` types `resulting_state` reaches. `Deserialize` where it already exists and would be free; do not add it where it would be unused. `CompatibilityConflict` and `PinnedBranch` come along as part of that.
  Enum representation: **externally tagged** (serde's default), so `{"Included": null}`-style output does not appear and a consumer switches on a plain string tag. A test asserts that shape for one variant of each enum, because the *representation* is the wire contract and the derive does not document itself.
- [x] `JsonDocument` for mutating commands: `{ "schema_version": 1, "plan": {…}, "receipt": {…} | null }`. `receipt: null` rather than an omitted key, so a consumer reads one field and finds either an object or an explicit "there was none" — a missing key is indistinguishable from a version that didn't emit it.
- [x] Deriving `Serialize` must not disturb the build record's own on-disk format. `EnvironmentBuildRecord` is already a persisted JSON document at `refs/hitch/state/*`; derive only adds capability and changes no bytes. Add a test asserting the serialized record still round-trips through `serde_json` to the same value, so a future `#[serde(rename)]` added for the wire format is caught immediately rather than after someone has an old repo.

### Task 4 — the confirmation gate

**Files:** `src/core/render.rs`.

**Interfaces:** `pub fn confirm_plan(context: &GlobalContext, rendered: &str) -> Result<bool>`.

- [x] Unit tests with a `Confirm` stub that records prompts and returns a scripted answer (`StdinConfirm`/`AlwaysYesConfirm` already exist; add a recording one in the test module):
  - `assume_yes` → `Ok(true)`, **and the prompt is never constructed** — the prompt string is the rendered plan, and building it is the cost `--yes` is supposed to avoid. A test asserting zero recorded prompts is the guarantee that `--yes` skipped the *prompt*, and the integration suite separately asserts it did not skip the *plan*.
  - no `assume_yes`, answer yes → `Ok(true)`; answer no → `Ok(false)`; declined is not an error.
  - `--json` without `--yes` → `Err` naming `--yes`, **without prompting**. This is the §10.3 requirement ("non-interactive JSON mode should fail clearly unless `--yes` is present") and it is deliberately not "try to prompt and see": a JSON consumer is a program, and a program that hangs on a TTY read is the failure mode the flag is meant to remove. The error names the flag and the reason.
  - `--no-push` is not a prompt bypass and is not consulted here. A plan with no `RemoteRefUpdate` still asks; the gate is about *authorisation to mutate*, not about how far the mutation reaches.
- [x] The rendered plan is printed to the sink (stdout normally, stderr under `--json`) and the prompt is a bare `Apply this plan? [y/N]:` on the same channel, so `--json` sees a clean stdout and an interactive user sees the plan above the question. Do not embed the whole plan in the prompt string — `Confirm::confirm` prefixes an emoji and a level, and a multi-line plan inside it reads as a log message that swallowed a prompt.
- [x] `Ok(false)` must leave nothing behind for the caller to clean up, so the doc comment states the contract: a declined plan is a no-op and the caller returns `Ok(())` after printing a one-line "No changes made." A declined confirmation is exit 0.

### Task 5 — wire `rebuild` (the reference path)

**Files:** `src/commands/rebuild.rs`, `tests/integration/rebuild_tests.rs`, `tests/integration/dry_run_tests.rs` if it exists.

- [x] Route **both** halves through the shared renderer. `--dry-run` already calls `plan_rebuild(…, PlanPurpose::Preview, …)` (P1); the real run currently goes through `rebuild_environment_opts`, which plans and applies internally and returns a `RebuildOutcome` — **not** an `ExecutionReceipt`. So the real run cannot render a receipt without either (a) getting the receipt out of `rebuild_environment_opts`, or (b) having the command call `plan_rebuild`/`apply_rebuild_plan` itself and drop the wrapper. Take (b): the command plans under `with_locked_env`, confirms, applies, renders. `rebuild_environment_opts` stays as the wrapper promote/demote/release/approve call for a *nested* rebuild, which is exactly the role P4 gave it.
  This is the largest change in the phase and it is the point of it: a receipt that only a `--dry-run` can produce is not a receipt.
- [x] Preserve the exit-2 contract verbatim. `OperationOutcome::AppliedWithHolds` → the same `Ok(true)` → `process::exit(2)` path, and `PlanPurpose::Preview` with holds → the same. A test asserts the exit code, because the exit code is a CI contract and a refactor that made the wording better while quietly folding holds into success is exactly the regression the enum was added to prevent.
- [x] Re-point the three `format_held_report` assertions in `rebuild_tests.rs` at the shared wording, then delete `format_held_report` (deviation 4). The shared renderer's `Held` line must carry the same remedy — `git checkout <branch> && git rebase <conflicts_with>` — because losing it would be a real UX regression, so the test that replaces the assertion checks for the *remedy*, not for a sentence.
- [x] Under `--json`, emit the document and print **nothing** else on stdout. The progress lines ("[1/3] Rebuilding environment 'dev' - Synchronizing branches") go to stderr with the rest; they are the §18 *diagnostic* channel, and a rebuild's step counter is diagnostics, not result.

### Task 6 — `--dry-run` and the plan on `promote` / `demote`

**Files:** `src/commands/promote.rs`, `src/commands/demote.rs`, `tests/integration/promote_demote_tests.rs`.

- [x] Add `--dry-run` to both arg structs with `rebuild`'s help text as the model.
- [x] Restructure `run` into the shape the phase is about:
  ```rust
  // Dry run: no lock, no confirmation, no mutation. A preview plan is never
  // applied, so nothing can go stale — which is the *only* reason the P5
  // "plan inside the lock" rule is safe to invert here.
  if args.dry_run {
      let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut on_step)?;
      emit(render_plan(&plan));
      return Ok(());
  }
  ```
  then the existing pre-checks, then `with_auto_stash` → `with_locked_env` → `capture_config_state` → plan → `confirm_plan` → `apply_declaration_plan` → `render_receipt`.
- [x] **A declined confirmation returns before `apply_declaration_plan`, and the rollback is not attempted** — nothing was written, so a rollback would restore a snapshot of the state that is already current and print a scary "rolled back" line for a no-op. The `Err` path keeps the existing rollback entirely.
- [x] The `--no-rebuild` "Skipping rebuild" message becomes a plan warning rather than a post-hoc log line, because it is a fact about what the plan will not do and belongs in the "Will not change" neighbourhood. `promote_demote_tests.rs:760` and `:878` assert on it; re-point them at the rendered plan. **Do not** reword it — the string is a user-facing promise and two tests hold it.
- [x] Exit codes: dry-run 0; declined 0; policy refusal 1; approval requested 0 (unchanged). `--no-rebuild` + `--dry-run` together is not a contradiction — it is the plan that says "and nothing will be built", which is a more useful thing to be able to ask than either flag alone.
- [x] Existing behaviour that must not move: the `Resolved '<env>' → N branch(es)` line (it comes off `plan.intent`, so it is redundant with the rendered headline — **delete the log line and let the renderer say it**, and update the test that asserts on it), the `Successfully promoted …` success line (the rendered receipt replaces it; update the assertions), and the rollback-on-`Err` path with its `CRITICAL:` message.

### Task 7 — `--dry-run` and the plan on `release`

**Files:** `src/commands/release.rs`, `tests/integration/release_tests.rs`.

- [x] Add `--dry-run`. Here the planner *does* branch on purpose: `plan_release(…, PlanPurpose::Preview, …)` already exists and is what makes the preview offline, unanchored, and unsynchronised. Use it.
- [x] Replace `confirm_release` (currently `src/commands/release.rs:238`, which re-reads the config to build its own bespoke "DANGEROUS OPERATION DETECTED!" block) with the shared gate. The bespoke block is exactly the §10.1 shape the shared renderer already produces, from the plan, so the two cannot disagree. `release_tests.rs` has assertions on the current wording; re-point them at the shared renderer's lines and check each is still *present* in meaning — a test that says "the target branch is named" survives a reword, one that asserts the full sentence does not.
- [x] The prune and dependent-rebuild sections are `PlannedEffect::PromotionPrune` and `PlannedEffect::DependentEnvironmentRebuild`, so the shared renderer already renders them; the four `on_step` progress lines P5 added ("Tagging", "Publishing", "Updating release metadata", "Rebuilding … — its declaration was pruned") stay as **diagnostics** (stderr) and do not also appear in the rendered plan. Duplicating them would mean the same fact in two channels with two wordings, and the §18 rule is that semantic output and diagnostics are separate.
- [x] Preserve: the DANGEROUS framing is dropped by design (the spec's own §10.1 example has no such banner) but the *information* it carried — how many branches merge, which, and into what target — must all still be on screen, and one test per fact rather than one for the banner.
- [x] Exit codes: dry-run 0; declined 0; conflict 1; release of an empty environment 0. `assert_no_release_anchors` is called on the dry-run path too — a preview that anchored would leak under `refs/hitch/release/*`, which nothing prunes.

### Task 8 — integration tests

**Files:** `tests/integration/plan_apply_tests.rs` (renderer/JSON shape), `tests/integration/promote_demote_tests.rs` (dry-run + decline), `tests/integration/rebuild_tests.rs` (exit codes, held wording), `tests/integration/release_tests.rs` (dry-run).

- [x] `--json` on each of the four mutating commands: stdout parses as JSON, has `schema_version == 1`, has both `plan` and `receipt` keys, and **contains no ESC byte**. The last one is the check that matters and it is a byte scan, not a substring check — "no ANSI in JSON" is not a claim you can make by looking for `[34m`, because the `colored` crate can and does emit other sequences. Run the same commands without `--json` first and assert the ESC byte *is* present, or the first assertion passes vacuously.
- [x] `--yes` shows the plan: run `promote` with `--yes` and assert the rendered headline and at least one "Will change" line are on stdout. This is the §10.2 requirement and it is the test that would fail if someone ever made `--yes` mean "quiet".
- [x] `--json` without `--yes`: the command fails, stderr names `--yes`, and **stdout is empty** (not "not parseable as JSON" — empty, so a consumer that ignores the exit code gets nothing rather than gets garbage).
- [x] Declining changes nothing: `promote` with a declining `Confirm`, then assert the target SHA, the `hitch-metadata` SHA, and the full `show-ref` listing are all byte-identical to before — the same three-claims-separately discipline as `a_conflicting_release_writes_nothing_at_all`, because "nothing happened" is three different assertions and this is a second operation with that contract.
- [x] `--dry-run` and a real run agree: for promote, demote and release, capture the dry-run's rendered "Will change" list and the real run's receipt effects and assert the *ref names* match as sets. Compare ref names, not prose and not SHAs — the dry-run does not synchronize (a preview reflects current local refs) so the SHAs can legitimately differ, and a test that compared them would be asserting a property the phase explicitly does not have.
- [x] `rebuild --dry-run` and `rebuild` agree on holds and on exit code, as two separate assertions, for a conflicting-pair fixture.
- [x] A receipt for a `--dry-run` is `null`, not a `{}` and not a missing key.
- [x] `--yes` with a declining-capable `Confirm` never reaches stdin: assert the recorded prompt count is 0 via a test-only sink, so this cannot regress into a hang.

### Task 9 — manual check, then the four gates

- [x] `cargo build -p hitch` (debug) and drive a throwaway repo in `/tmp`: `hitch init`, `hitch promote` (see the plan, decline, promote for real), `hitch --json promote` without and with `--yes`, `hitch promote --dry-run`, `hitch release --dry-run` (real and preview), `hitch promote`/`demote --dry-run` and `--json`, `hitch rebuild --json` with and without `--yes`, and a promote into an approval-gated environment answered both ways at a real prompt. Read the output as spec §10.1 does: a user who has not read the README should be able to say what will happen, and after the fact, what happened and what is still owed.
- [x] Confirm the `git checkout <b> && git rebase <other>` remedy is present in the held output — it is the one piece of guidance the shared renderer reconstructs rather than copies, and the integration tests assert it but a human should read it once.
- [x] Confirm `hitch --json promote … | jq .receipt.effects[0]` works, and that `hitch --json rebuild dev | jq` on a held build still returns exit 2 with a parseable document.
- [x] `just format`; `just format-check && just lint`; `just test`. All three clean, no `#[ignore]` added.
- [x] `git diff --name-only main..HEAD -- crates/` empty; `main` still at `5d81fb2`; nothing pushed.

---

## Execution notes

Written after the code landed, so the next phase reads decisions rather than
intentions. Everything here is a fact about the code as committed; the reasoning
is in the diff and in `AGENTS.md`.

### Shape the plan did not predict

- **`rebuild_environment_gated` is the phase's real centre, not a helper.** The
  plan said "have the command call `plan_rebuild`/`apply_rebuild_plan`
  itself". What actually shipped is one gated sequence in
  `src/utils/prelude.rs` — plan, hand the plan to a caller-supplied
  `FnOnce(&plan) -> Result<bool>`, then apply — returning
  `RebuildRun { plan, receipt }`. `hitch rebuild` supplies
  `confirm_plan(...)` as the gate; the four nested callers supply
  `|_plan| Ok(true)`, because a dependent rebuild must not re-ask a question the
  outer plan already put to the user. `Ok(None)` means declined. This is
  *better* than the plan's shape: the "plan, then apply, then discard the
  anchor in a `finally`" sequence exists exactly once, so a fifth operation
  cannot get the ordering wrong.
- **A named `StepNarration` enum, not a bool.** `Log(Arc<dyn OutputSink>)` vs
  `Suppressed`, because the two arms differ in *kind* (narrating because
  nothing better will be shown, versus going quiet because something better
  already is) and a `bool` loses that at the call site.
- **The anchor is not a "Will change" entry.** `render_plan` partitions effects
  with `is_transient_anchor(refname)` — a prefix match on
  `refs/hitch/build/` and `refs/hitch/release/`, which is a *fact about hitch's
  ref layout* and never a match on a description string. Anchors render under
  their own heading, **"Held only until the publish lands"**, and remain in the
  JSON. `refs/hitch/state/*`, `prev/`, `backup/` and `publish/` are deliberately
  *not* matched: they are durable, and a plan that hid a durable effect would be
  lying about the post-state.
- **`emit_plan` and `emit_receipt` are separate, and there is no
  `emit_plan_and_receipt`.** The gate already printed the plan, so a combined
  emitter printed it twice on the apply path. Same reason, so: same fix.
- **`rebuild --dry-run` no longer synchronises, and that asymmetry is
  documented at the call site rather than papered over.** A preview reflects
  current *local* refs while a real build syncs first, so a stale local branch
  can make the preview describe older content. That is ordinary staleness,
  categorically weaker than the two-merge-engines bug P1 removed, but it is a
  real remaining difference and the honest fix is to make sync a shared,
  user-visible step — not to re-add a second merge path.
- **`StepNarration::Log` stays on the *nested* rebuilds.** The temptation was to
  suppress them as duplication, and it is wrong: the outer promote/release plan
  says "rebuild `qa`" as an effect and carries no composition detail for it, so
  the nested transcript is the only account of what `qa` will contain. A
  *direct* rebuild narrates the composition the plan just showed, which is the
  duplication `Suppressed` exists for.
- **A dry run's no-op guarantee is the missing lock, not the missing stash.**
  `with_auto_stash` turns out to be a no-op difference for all four commands
  (measured, not assumed: none of them moves the user's `HEAD`), and it is
  skipped in a preview anyway. The lock is the real half.

### Bugs the manual check found

Each of these was a shape that read correctly and was not. They are recorded
here because all three would have shipped silently — the test suite was green
throughout.

1. **A gate error was swallowed, and `--json` exited 0 having done nothing.**
   `rebuild_environment_gated` had `Ok(false) | Err(_) => { discard; None }`,
   so a `--json` run without `--yes` discarded the anchor, returned "declined",
   and exited **0** with no document on stdout and the "re-run with `--yes`"
   reason discarded. A CI consumer would have read success and an empty
   pipeline. Split into two arms, with the discard first on both so `?` cannot
   skip it, and `Err` propagates: **`--json` without `--yes` now exits 1** with
   the reason on stderr. This is a deliberate change to the exit table, and the
   one that matters most, because the old code was the bug.
2. **Release leaked its anchor on the error arm.** `if !confirm_plan(...)? {
   discard }` discards on a decline and not on an `Err`, so a `--json` release
   without `--yes` propagated and left `refs/hitch/release/*` behind — a family
   nothing prunes, one leaked ref per refusal. Now an explicit `match` that
   discards on both non-applying arms. `promote`/`demote` needed no equivalent
   fix because `plan_declaration_change` composes nothing and anchors nothing;
   that is the reason the rule is "every non-applying arm owes a discard"
   rather than "patch the three commands".
3. **Neither bug had a regression test, so both got one.** A unit test on
   `decide_gate` cannot catch either: the refusal itself was correct, and what
   was wrong was how the *caller* handled the `Err` and the `discard`. So
   `a_json_rebuild_without_yes_fails_loudly_and_leaves_nothing` and
   `a_refused_json_release_leaves_no_anchor_and_no_tag` go through the built
   binary, and each one carries a note about the harness flag that makes it
   non-vacuous (`with_no_push(false)`, because a rebuild only requires
   confirmation when it owes a push).
4. **The confirmation prompt never said what confirming would do.**
   `ConfirmationRequirement.reason` existed, was carried by all three planners,
   was asserted on by a unit test (`a_confirmation_requirement_does_not_change_
   how_a_plan_renders`) — and no code path ever printed it. Every prompt in the
   CLI read `Apply this plan?`. The worst case was promote into an
   approval-gated environment: the plan's "Will change" section is *empty*
   there (confirming files an approval request instead of editing the
   declaration), so the user was asked to authorise a plan that visibly does
   nothing, with no statement of what the answer would do. `confirmation_question`
   is now a pure function of the requirement, printed above the question, and
   the approval reason was reworded to describe the action ("confirming files
   an approval request for this promotion") rather than to restate the warning.
   Note what this is *not*: the P5 rule that a policy refusal outranks the
   approval gate is untouched. An approval gate **asks**; asking whether to file
   a request is a real question with a real answer.

### Carried forward, deliberately unfixed

- **`rollback_metadata_changes` can undo the lock release that happened after
  the snapshot was taken.** `capture_config_state` runs *inside*
  `with_locked_env` (deliberately — `promote.rs` says so, so the snapshot is
  not pre-lock), and the rollback runs *outside* it, after the lock has been
  released. Restoring a whole-config snapshot therefore restores
  `locked: true`, and the next `promote`/`rebuild` on that environment fails
  with "Environment 'dev' is currently locked by '<user>'" until someone runs
  `hitch unlock`. Reachable on any apply that fails after the lock — a pending
  approval request that already exists, an approver threshold that cannot be
  met. Found here, recorded in `AGENTS.md` under the existing rollback gotcha,
  **not fixed in P6**: it is a `rollback`/lock-ordering change, not a rendering
  change, and folding it into the P6 commit would have made the phase
  unreviewable. It needs a decision about the snapshot's shape (re-clear the
  lock after restoring, versus capture before the lock and keep the current
  ordering).
- **`hitch approve` has no receipt**, and `src/commands/resolve.rs` still
  decides its resolution mode through `preflight_compatibility_report`. Both are
  scoped notes for P7/P8, not P6 omissions.
