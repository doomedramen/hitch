# AGENTS.md

Instructions for AI coding agents working in this repository.

## Keep this file up to date

This file goes stale the moment it stops matching the code. When your work
changes something it documents — command registration steps, architecture,
conventions, dead-code status, gotchas — update the relevant section in the
same change, not as a follow-up. Concretely:

- Added/removed/renamed a module, command, or major function this file
  names? Fix the reference.
- Found a new sharp edge the hard way (a bug class, a footgun, a "this looks
  right but isn't")? Add it under a gotcha section, the way the merge-base
  bug below is recorded — future agents shouldn't have to rediscover it.
- Revived something listed as dead/aspirational code? Move it out of that
  section.
- Noticed an existing line is already wrong or misleading? Fix it on sight,
  regardless of whether it's related to your task.

Stale docs are worse than no docs — they cost the next agent (or human) more
time than they save. Keep entries terse and specific; don't let this file
grow into prose. If a claim can go stale silently (a file path, a test
count, a "currently"), prefer phrasing that's cheap to verify over phrasing
that's easy to trust blindly.

## What this is

Hitch is a Rust CLI (`src/main.rs`, library crate `src/lib.rs`) for Git
branch management in environment-based deployment pipelines. An environment
branch (`dev`, `qa`, `production`, ...) is declared as a base branch plus an
ordered list of promoted feature branches; `hitch rebuild` regenerates the
environment branch from that declaration rather than accumulating manual
merges. Config lives as `hitch.json` on a dedicated `hitch-metadata` branch.

A secondary crate, `crates/hitch-desktop`, is a Tauri + React desktop GUI —
out of scope unless a task explicitly touches it.

Read `README.md` for the user-facing model and `SKILL.md` for the condensed
agent-facing command reference. `docs/merge-conflict-handling-plan.md` is the
active design doc for the conflict-handling system (isolated rebuilds,
eject-and-continue policy, `hitch resolve`) — check its
"Implementation status" section before assuming a phase is done or before
starting related work.

`docs/superpowers/plans/2026-09-25-explainable-ux-program.md` is the master
plan for the explainable-UX program: turning mutations into
`INTENT → PLAN → APPLY → RESULT` so every one can explain itself. It
implements `docs/explainable-ux-spec.md` (a verbatim copy of the author's
original spec, sections 1–42) in ten phases, P0–P10. Read the master plan's
**Global Constraints** before touching any code under it — they encode which
line numbers are load-bearing and why. Two scope decisions differ from the
spec's own §29: `crates/hitch-desktop` (spec §20–§26, M9/M10/M11) is deferred
to a separate repair stream, and the broken-`main` CI repair is handled
independently of this program. P0–P8 are authored and complete; P9 is
next. Later phases are authored as they approach, because their `file:lines`
references go stale the moment the previous phase lands.

**The program lives on the `explainable-ux` branch, not `main`.** It forked from
`main` at `5d81fb2` and `main` is meant to stay exactly there for its duration —
don't rebase it forward, don't cherry-pick phase commits onto it, don't "just
land this one bit." The branch's "Where this work lives" section in the master
plan carries the commit list and the standing check that `crates/` is untouched.

## Build, test, lint

Always use `just` recipes (see `justfile`) — they're what CI and pre-commit
hooks run, so using anything else risks passing locally and failing there.

```bash
just build              # cargo build --release -p hitch   (RELEASE — see note)
just format             # cargo fmt
just format-check       # cargo fmt --check  (CI gate)
just lint                # cargo clippy -p hitch --all-targets -- -D warnings  (CI gate)
just test               # cargo test -p hitch  (full suite)
```

**`just test-file <name>` is broken for integration tests.** The
`tests/integration/*_tests.rs` and `tests/scenarios/*_tests.rs` files are
*modules* of a single `tests/mod.rs` test target, not separate targets — the
only integration targets cargo knows are `mod` and `no_args_help`. So
`just test-file rebuild` fails with `no test target named 'rebuild' in
default-run packages`, and the recipe is not `-p hitch`-scoped like the gates
above. Run one integration test with:

```bash
cargo test -p hitch --test mod -- integration::rebuild_tests::tests::<test_name> --exact
```

The full module path is required — a bare test *name* filter matches nothing,
because the module path is part of the test's identity.

**`just build` is a release build**, which matters for anything touching the
crash-recovery abort hook: `HITCH_TEST_ABORT_AFTER` is
`#[cfg(debug_assertions)]`-gated, so a release binary silently ignores it and
exits *successfully*. Use `cargo build -p hitch` (debug, `target/debug/hitch`)
for any manual verification of publish recovery — see the "Recovery is tested
by interruption" gotcha below.

**Before considering any change done**, run in this order and expect all
three clean: `just format`, `just format-check && just lint`, `just test`.
Clippy runs with `-D warnings` — any warning is a hard failure, not a
suggestion. The suite is ~300 tests and takes under a minute; there is no
excuse for skipping it.

For a change that's user-visible in the CLI (new flag, new command, changed
message), build the binary and exercise it against a throwaway git repo in
`/tmp` before calling it done — the integration test framework
(`tests/test_framework/`) is good but doesn't replace an end-to-end manual
check, and this session found a real correctness bug (see below) purely by
running the built binary against a hand-built scenario that no existing test
covered.

## Architecture map

- `src/cli.rs` — the single source of truth for the command tree (clap
  derive). `Commands` enum here, `commands::completion` generates shell
  completions straight from it, and `src/main.rs` dispatches on it. Adding a
  command means touching all three: `src/commands/mod.rs` (register the
  module), `src/cli.rs` (add the `Commands` variant), `src/main.rs` (add to
  both the `command_name` match and the dispatch match, and to
  `command_is_mutating` if it's read-only). The global flags live here too:
  `--json` is `global = true` and its doc comment **names the fourteen commands
  that honour it** (`rebuild`, `promote`, `demote`, `release`, `lock`, `unlock`,
  `set`, `add`, `remove`, `cleanup`, `approvals approve`, `status`, `why`, `log` —
  every mutating command plus the three read-only ones) and says that a command
  without support leaves stdout **empty** rather than printing prose. Both halves
  of that are load-bearing and the second is easy to get wrong in the direction
  that looks safer: "says so" implies a diagnostic on stdout, and the mechanism
  is the opposite — under `--json` the `log_*` sinks all go to stderr, so a
  command with no support says nothing there either. `tests/integration/json_support_tests.rs`
  parses the fourteen names out of the doc comment's own backticks and compares
  them as sets, both directions, against the command files in `src/commands/`
  that actually reach `emit_json`/`emit_plan`/`emit_receipt` — so neither a
  dropped command nor an invented one survives. The three read-only ones use a
  **one-half envelope**, `{"schema_version": 1, "<view>": …}`, not the
  mutations' `{"plan", "receipt"}` — a read-only view has no "after", and a
  `null` receipt would say "nothing happened", which is true and useless. Every
  enum in those envelopes is `snake_case`, and a shared collector (`pascal_case_tokens`, `tests/test_framework/json_helpers.rs`) walks the document and fails on any
  `PascalCase` token, so a new enum cannot forget the rename.
- `src/commands/*.rs` — one file per CLI command/subcommand, thin: arg
  parsing (`clap::Args` struct) + orchestration. Business logic belongs in
  `src/utils/prelude.rs`, a dedicated `src/utils/*.rs` module, or the
  `src/operations/<op>.rs` that owns the command's plan, not here. The mutating
  commands share one shape and it is worth copying rather than reinventing:
  pre-checks, then `with_auto_stash` → `with_locked_env` → plan →
  `confirm_plan` → apply → `emit_receipt` (or `emit_plan` alone for a
  `--dry-run`, which plans *outside* the lock and never applies). `emit_plan`
  and `emit_receipt` are separate because the gate already printed the plan;
  a combined emitter prints it twice. Two shapes in that sentence are *not*
  shared by every command and the departures are deliberate: a metadata
  operation (`set`, `add`, `remove`, `lock`, `unlock`) is a one-shot declaration
  edit, so it is planned and applied under `with_locked_env` but composed from
  `access_metadata_read_only` rather than from a pinned environment, and it has
  no anchor to discard because it composes nothing. `hitch cleanup` has no
  environment lock at all — it sweeps refs and branches across every
  environment, so the `with_locked_env` it would need is the set of all of
  them.
- `src/utils/prelude.rs` — the domain-logic hub: rebuild orchestration,
  metadata read/write transactions (`access_metadata_read_only`,
  `modify_metadata`), locking (`with_locked_env`), the conflict-preflight
  functions, and the shared composition pair
  `pin_environment_inputs` → `compose_environment` that both `rebuild` and
  `rebuild --dry-run` run (see the "One composition, two callers" gotcha —
  that pair is the invariant, not an implementation detail). Large file; read
  the doc comment on the specific function you need rather than the whole file.
- `src/utils/git_operations.rs` — the *only* place that shells out to `git`.
  Every git primitive is a named method (`merge_tree_compose`, `commit_tree`,
  `update_ref_cas`, ...). All of them build their subprocess through
  `git_command`, which forces `LC_ALL=C`/`LANG=C` (several call sites match
  English stderr substrings), `GIT_TERMINAL_PROMPT=0`, `stdin(Stdio::null())`
  (see the gotcha below), and the hardening flags `core.hooksPath=/dev/null`
  and `core.fsmonitor=false` — hitch runs under a deploy key that bypasses
  branch protection, so anything repo-local config makes git execute inherits
  those rights. This is NOT because repo-local config is reachable by push —
  `.git/config` is local-machine state, never transported by `push`/`clone` —
  the real risk is a prior CI job, a shared runner, or anyone with filesystem
  access to the checkout leaving config hitch's automation shouldn't trust.
  Note this is also `require_signed_resolutions`'s own root of trust
  (`gpg.ssh.allowedSignersFile` is read from this same repo-local config), so
  that feature's guarantee is bounded by control of the checkout, not by
  git's push/clone security model — see the doc comments on `HARDENING_ARGS`
  and `verify_signature_ssh`. `run_git_plumbing_command` adds
  `GIT_CONFIG_NOSYSTEM=1` and is for object-database-only calls; network calls
  must not use it, because on macOS the system config is where
  `credential.helper` lives. `clippy.toml` denies `std::process::Command::new`
  outright, so a new spawn point must carry an explicit
  `#[allow(clippy::disallowed_methods)]` — that annotation is the review
  signal. `git2` is used for the read-only plumbing primitives (`rev_parse`,
  `rev_parse_opt`, `cat_file_blob`, `read_blob`, `get_merge_base`) via the
  `Repository` handle `GitOperations` already opens in both constructors —
  no subprocess for those five. git2 bypasses `HARDENING_ARGS`/
  `GIT_CONFIG_NOSYSTEM` entirely — safe here because pure object reads
  execute nothing, so don't migrate a primitive to git2 where the hardening
  itself is the point (anything that could run a configured program, like a
  merge driver). Everything else, including every primitive touching the
  ORT merge engine (`merge_tree_compose` and siblings) and anything hitting
  a remote, still shells out to the real `git`/`gh` binaries; see the
  differential tests in `tests/unit/git_operations_tests.rs`
  (`*_agrees_with_git_cli`) for why that boundary is where it is.
- `src/utils/gh.rs` — same pattern for the GitHub CLI (`gh`), used by `pr`,
  `doctor`, `setup`, and `pr_status`.
- `src/utils/publish_journal.rs` — the record of what a publish still owes,
  for the effects git cannot make atomic. The publish itself is one ref
  transaction (see `publish_environment_build`); what remains outside it is the
  checkout resync and the push to origin. Both obligations are written to
  `refs/hitch/publish/<branch>` before the ref moves, inside the same
  transaction, and cleared as each completes; `recover` runs from `main.rs` for
  mutating commands only, so it is always under the repo lock. It repairs a
  checkout only when that tree is *provably* exactly the old tip — never on the
  dead process's say-so — *and* the branch's own reflog corroborates that
  history (fails open on an empty/missing reflog, closed only on an actual
  contradiction) — so an edited tree is reported, not reset, and an owed
  push is reported rather than performed. Legacy
  `refs/hitch/pending-resync/<branch>` records are still read so an upgrade
  mid-publish recovers.
- `src/utils/resolutions.rs` — phase-5 shared conflict resolutions:
  content-addressed by exact merge-stage blob OIDs (NOT git-rerere — see the
  module header for why that matters), stored as `refs/hitch/resolutions/*`.
  Consumed by `hitch resolve --record`, `hitch rebuild --replay-resolutions`
  (replay also requires `source_branch_head` lineage — the current tip must
  be that commit or a descendant of it, not just match on stage OIDs),
  `hitch resolutions`, and `hitch doctor`'s debt SLA.
- `src/utils/build_record.rs` — what an environment branch's last build
  *actually* contained, written as JSON at `refs/hitch/state/<env>` and
  written by `rebuild_environment_opts` (not by `commands/rebuild.rs`, so
  `promote`/`demote`/`approve`/post-release-rebuild get a current record
  free — since P5 each of those reaches it as the *nested* rebuild its own
  planner performs, which is the same set of callers by a longer path). Since
  P4 the `hitch rebuild` path writes it from a *plan* —
  `plan_rebuild` computes the `RefEdit` once at plan time and
  `apply_rebuild_plan` passes that same value through verbatim, because the
  record is a claim about *that* composition and recomputing it at apply time
  would be a second decision point. It replaces the old state layer's
  inference-from-timestamps. The
  record's `RefEdit` rides `publish_branch`'s existing transaction, so
  "environment tip moved" and "record describing that tip now exists" are
  applied all-or-nothing. Its reader, `read_state`, is the sole input to
  `src/core/state.rs`'s Actual side.
- `src/core/` — read-only view builders (workspace/status/state models).
  - `activity.rs` (behind `hitch log`) is a pure event model (`derive_events`
    diffs two configs into typed `HitchEvent`s) plus one reader,
    `build_activity`, walking `hitch-metadata` first-parent history. Lock
    brackets that hitch wrote around its own operation are collapsed out (only
    manual lock/unlock survive); a build record attaches to a rebuild entry only
    when `metadata_sha`..stamp provably identifies that one rebuild. `hitch log`
    takes no repo lock (read-only, in `command_is_mutating`'s `false` arm).
  - `state.rs` is **the** authority on "does this environment need a
    rebuild, and what actually moved". `build_state_snapshot` returns a
    `RepositoryStateSnapshot` — Desired read live from refs, Actual read
    from the build record, Health classified by comparing the two — and
    every display of that verdict must read it from here rather than
    re-deriving it. `status.rs` is now a *pure projection*:
    `build_status_model(&snapshot)` takes no context, opens no repo, and
    cannot disagree with the snapshot it was handed. `commands/status.rs`
    reads the same snapshot and only formats it.
  - Since P7, `status.rs` also holds the **feature × environment matrix**:
    `MatrixCell` (seven states, `classify` total over
    `(desired, actual, has_record)`), `build_matrix_model`, and
    `has_record_for` — which is *derived* (`has_record ⟺ !matches!(health,
    LegacyUnknown)`) rather than read from a second place, because a second
    read is a second thing that can disagree. Every cell is materialised and
    the summary counts are computed *from* the cells, so a count and a row
    cannot drift.
  - `why.rs` (new in P7) builds a `WhyExplanation` from the same snapshot and
    is the answer to "why is this branch in that state". `WhySubject` is
    resolved by `commands/why.rs`, not here, because resolution needs a
    repository and the model must not have one. `WhyMembership` is a
    *distinct* vocabulary from `ActualMembership` with a one-to-one
    `From<MatrixCell>`: `ActualMembership` describes an outcome, and §14's
    questions ("already in the base") are facts about a journey, which the
    outcome cannot answer.
  - `workspace_index.rs`'s `build_workspace_index_model`/`WorkspaceIndexModel`
    have no CLI command caller — they're consumed by `crates/hitch-desktop`'s
    Tauri backend (`src-tauri/src/main.rs`), not `src/commands/*.rs`.
    `details.rs` and `status.rs` are the other two view builders.
  - `render.rs` is **the only place in the codebase allowed to choose words**
    for a plan, a receipt, a status matrix, or a `hitch why` explanation.
    `render_plan<I>` / `render_receipt` / `render_matrix` / `render_why` /
    `render_equation` are pure and total — a value in, a `String` out, no
    `GlobalContext`, no `Result`, no git, no clock — which is the same rule
    `build_status_model` follows and for the same reason: a renderer that can
    open a repository can disagree with the thing it renders. One function
    serves all four *operations* because the *model* is the vocabulary;
    `detail` is the one field a shared renderer ignores. `render_equation` is
    shared too, and `describe_projection` and `commands/tree.rs` both go
    through it, so a composition has one spelling in the codebase. The impure
    half is `emit_json` / `emit_plan` / `emit_receipt` / `confirm_plan`, and
    all four take the already-rendered `String`. A new display path renders
    through here or through a sibling taking the same inputs; do not grow a
    second set of words.
- `src/operations/` — the plan → apply → receipt architecture, one operation
  at a time. `model.rs` is operation-agnostic (`OperationPlan<I>` generic over
  its per-operation detail, `PlanFingerprint`, `PlannedEffect`/`AppliedEffect`,
  `ExecutionWarning`, `ExecutionReceipt`, `OperationOutcome`, `PlanApplyError`,
  plus the shared `changed_inputs` diff helper every validator calls);
  `rebuild.rs` holds `plan_rebuild`/`apply_rebuild_plan` and `PlanPurpose`;
  `declaration.rs` holds the promote/demote planner+executor, plus
  `plan_approved_declaration_change` for `hitch approve`; `release.rs` holds
  release's; `metadata.rs` holds the five declaration-editing operations
  (`set`, `add`, `remove`, `lock`, `unlock`) behind one `MetadataEdit`
  discriminator and one `MetadataPlanDetail`; `cleanup.rs` holds the prune
  sweep. A new operation adds a `*PlanDetail` and a `plan_*`/`apply_*` pair,
  an `OperationKind` variant (append to `OPERATION_KINDS` — a test walks it),
  and an `OperationIntent` variant (a headline test walks that list too), and
  reuses the fingerprint, validation, and receipt assembly as shared
  machinery rather than re-deriving them. One planner per operation, on purpose
  — see the "one planner per operation" gotcha. `rebuild_environment_gated` in
  `src/utils/prelude.rs` is the one plan-then-apply *sequence*: it plans, hands
  the finished plan to a caller-supplied `FnOnce(&plan) -> Result<bool>` gate,
  and applies, returning `RebuildRun { plan, receipt }` with `Ok(None)` meaning
  declined. A sixth operation should reach for it rather than write its own —
  the ordering it gets right (the `finally` that discards the anchor, and the
  discard on *both* non-applying arms) is exactly the kind of thing a
  hand-rolled copy gets wrong. `validate_metadata_plan` and
  `validate_cleanup_plan` are *separate copies* of `validate_plan` rather than
  instantiations of it, because the fingerprint's meaning differs per operation
  and unifying them turned out to need a type parameter rather than a
  parameter; extract on the third copy, not before.
- `src/types.rs` — `HitchConfig`/`Environment`/`ApprovalRequest` etc., the
  schema persisted as `hitch.json`. Adding a field needs `#[serde(default)]`
  (or a default fn) so older configs still deserialize, and — if it should
  be settable via CLI — a matching `--flag` on `commands::set::SetCommand`.
- `tests/test_framework/` — the integration-test harness:
  `HitchTestFramework::new()` + `.with_test_environment(TestSetup::HitchInit,
  |env| { ... })` gives you a real throwaway git repo with `env.git` (raw git
  commands), `env.hitch` (runs the built binary), `env.fs` (file helpers).
  `tests/integration/*_tests.rs` is one file per command, `tests/unit/*` for
  pure-function tests. Follow the existing naming pattern:
  `test_<command>_<scenario>`.
- `docs/superpowers/plans/` — house-format implementation plans (header block,
  Global Constraints, `### Task N` with **Files**/**Interfaces** and checkbox
  steps). The explainable-UX program's master plan and its phase plans live
  here; P0's scenario-inventory table and behavioural-oracle tables are the
  reference for any refactor that claims to preserve existing behaviour.

## Conventions

- **Errors**: `anyhow::Result` everywhere in commands/utils; error messages
  for user-facing failures are multi-line, end with the exact command to run
  next (`git checkout {branch} && git rebase {base}`, `hitch rebuild {env}`,
  ...). Match that style for new errors — a bare error string without a next
  step is a worse experience than the rest of the CLI. Build that next step with
  `OperationKind::command_hint`, which already spells the full `hitch …`
  invocation: the `PolicyBlocked` arm used to wrap it in a second `"hitch {}"`
  and printed a remedy nobody could paste — `hitch hitch promote fc dev` — so
  treat "it names the command" and "it is the command" as two different claims.
- **Comments**: sparse, and only for *why*, not *what* — a hidden invariant,
  a workaround for a specific git quirk, a reason a naive approach doesn't
  work. Don't add comments restating what the code obviously does.
- **No premature abstraction**: three similar call sites are fine
  uncombined; don't introduce a trait or generic helper until there's a real
  second caller that needs the flexibility.
- **Locking discipline**: mutating commands take the repo-wide flock
  (`RepoLock`, acquired in `main.rs` via `command_is_mutating`) plus, for
  rebuild specifically, a per-environment flock (`RebuildLock`) and the
  persisted `Environment.locked` metadata flag — three separate mechanisms
  for three separate concerns (cross-process serialization, rebuild-specific
  serialization, human-facing "don't touch this env" signal). Know which
  one(s) a new mutating operation actually needs. The persisted lock also
  records *why* in `Environment.lock_purpose` (`Operation` from
  `with_locked_env`, `Manual` from `hitch lock`; `Environment::lock` takes it, so
  every caller must choose). `core::activity::build_activity` reads it to drop an
  operation's own lock/unlock bracket from history; pre-field history falls back
  to a 60 s heuristic (`LEGACY_OPERATION_LOCK_WINDOW`). Nothing else reads it.
- **The user's working tree is sacred**: nothing builds in the user's own
  checkout — no `checkout`/build/`checkout back` in the real repo, ever, and
  no command should ever be able to strand them on a branch they didn't ask
  for. This started as the phase-1 disposable-worktree redesign (see the plan
  doc) and has since gone further: `hitch rebuild` and `hitch release` compose
  with pure plumbing (`merge_tree_compose` + `commit_tree`) and create no
  worktree at all. `hitch resolve` is the only remaining worktree user,
  because a human editing conflicts needs files on disk — but it works in a
  *detached* worktree (`add_worktree_detached`) and lands the result with a
  CAS `update-ref` plus the standard checkout resync, so it works even when
  the branch it is rebasing is the one the user is standing on. New
  build/merge logic should use the plumbing path; reach for a worktree only
  when a human has to edit the result, and detach it.
- **`publish_branch` is the one atomic-publish core; `rebuild`, `release`, and
  `resolve` all land through it now.** `publish_branch` in `prelude.rs`,
  built on `GitOperations::ref_transaction`, moves `refs/heads/<branch>`
  under compare-and-swap, writes the publish-journal intent (including
  whether a push is owed), and — when given a `backup_timestamp` — archives
  the replaced tip in the same batch: no rename-and-recreate dance, no
  sequence of separate `update-ref` calls, all or nothing. Reuse it for
  anything that produces a new branch commit rather than hand-rolling the
  publish step again. `publish_environment_build` (used by `hitch rebuild`
  and `hitch resolve`'s Mode B peer-conflict path) is a thin wrapper around
  it that always passes a timestamp; `hitch release`
  (`src/commands/release.rs`) and `hitch resolve`'s Mode A rebase-landing
  path (`finish_mode_a` in `src/commands/resolve.rs`) call `publish_branch`
  directly with `backup_timestamp: None` — a rebase's or release's prior tip
  is already named elsewhere (the rebase's `from_sha`, the release tag), so
  neither needs the `prev`/`backup` archival refs. This was a deliberate,
  staged migration (`docs/superpowers/plans/2026-08-01-unify-publish-atomicity.md`):
  `release` and `resolve`'s old two-step sequence — `publish_journal::record()`
  followed by a separate `update_ref_cas` call, with an unconditional
  `clear()` immediately after the resync instead of surviving until the push
  resolved — is gone; no caller of the non-blob `publish_journal::record()`
  remains. Crash-fuzz coverage exists for all three at the three abort points
  that precede any push (`crash_recovery_tests.rs`,
  `release_crash_recovery_tests.rs`, `resolve_crash_recovery_tests.rs`; see
  the "Recovery is tested by interruption" gotcha below for why the fourth,
  `push-succeeded`, is untested for `release`/`resolve`). One asymmetry
  remains from `release`'s force-push tag creation being a second,
  non-`publish_branch` operation glued on afterward — see
  `src/commands/release.rs` for how that failure is reported separately
  rather than through `publish_branch` itself. `refs/hitch/prev/*` and
  `refs/hitch/backup/*` (both written only when `backup_timestamp` is
  `Some`, currently byte-identical to each other every publish — see the doc
  comment at the write site in `prelude.rs` for why both still exist) are
  written with unconditional-overwrite semantics
  (`RefEdit::Update { expected_old: Some(String::new()) }`, not `Create`) so
  that a same-second collision on the timestamped ref name can't fail the
  whole transaction — see the doc comment on `RefEdit`. The trade-off: two
  publishes of the same environment landing in the same second leave only
  the later tip archived under that timestamp; the earlier one is not lost
  from the object database, just no longer reachable via this ref. `prev/`'s
  "rollback is a one-ref flip" guarantee is therefore per-timestamp, not
  per-publish, in that rare case. The publish-journal ref edit inside that
  transaction uses `expected_old: Some(String::new())` (unconditional write),
  not `None` (which this codebase's `ref_transaction` maps to "must not
  exist") — a leftover publish-journal record is documented in
  `publish_journal`'s module doc as benign and recoverable, so it must never
  be able to fail this transaction. **Do not touch that `expected_old` value**
  when working near this code — see the module doc and the regression test
  `publish_survives_leftover_publish_journal_ref` in `prelude.rs`, added
  after this exact CAS was once wrongly set to `None` and turned an
  unparseable leftover record into a permanent, self-perpetuating publish
  wedge for that environment.
- **Moving a branch ref means resyncing every checkout attached to it.**
  `scan_checkouts_on_branch` before the ref transaction, `resync_checkouts`
  after — both in `prelude.rs`, both called from inside `publish_branch`
  itself, so every one of its callers (`publish_environment_build`, `hitch
  release`, `hitch resolve`'s Mode A) gets this for free. See the gotcha below for
  why the scan cannot be folded into the resync. The intent is written with
  `publish_journal::record_blob` so its ref update rides inside that same
  transaction; `publish_branch` keeps the record in place until
  every obligation it describes is actually settled — cleared immediately
  only if no push is owed, otherwise cleared (and the push obligation marked
  done via `publish_journal::mark_push_done`) once the push succeeds, cleared
  if the user declines the push prompt, and left in place on push failure, so
  a crash in any of those windows is recoverable or at least reported by
  `recover` on the next mutating command. Since P4 `publish_branch` returns
  `PublishOutcome { push, journal_cleared }` rather than `()`, because a caller
  building a receipt has to be able to tell "pushed" from "declined" from
  "failed and still owed" — see the gotcha below.
- **Compare checkout paths with `GitOperations::same_checkout_path`, never
  `==`.** `git worktree list` reports fully resolved paths; on macOS the temp
  dir and plenty of real project paths sit behind symlinks, so a string
  comparison silently never matches and whatever it gated is quietly skipped.
  This has already caused one bug in `publish_journal`'s recovery.

## Concrete gotchas, found the hard way

**`refs/hitch/state/*` is a live pointer, not an archive — never add it to
`cleanup`'s prunable set.** Every other family under `refs/hitch/` is either
an archive (`prev/`, `backup/`, pruned to `ARCHIVE_REF_RETENTION` in
`commands/cleanup.rs`'s `["backup", "prev"]`) or transient (`build/`,
`publish/`, `resolutions/`). `state/` is the odd one out: exactly one record
per environment, overwritten in place, and its whole job is to be the current
answer to "what is in this environment branch". They look alike because they
share the `refs/hitch/` root; they are not alike. `test_cleanup_does_not_prune_the_build_record_ref`
holds the line, and asserts a prune that *does* fire alongside it so it can't
pass vacuously.

**A missing build record is `LegacyUnknown`, not "probably fine" — and
`LegacyUnknown` is a normal state today, not a defect.** `read_state` returns
a value for every failure mode (`LegacyUnknown` / `Known` /
`ResultMismatch` / `Unreadable`) and `Err` only for genuine I/O failure,
because the alternative is a `hitch status` that cannot render a repo
containing one bad blob. Crucially, not every publish writes a record:
`hitch release`'s branch and tag landing, `resolve`'s Mode A, and `resolve`'s
Mode B all pass `extras: &[]` **on purpose**, because hitch has no truthful
input for a record there (Mode B's included-branch list is the thing a human
wrote by hand). So a reader finding no record must say "unknown", never
substitute a guess — and the follow-up is to have those paths track what they
composed, *not* to fabricate a record now. A corrupt, mismatched, or
future-schema record is likewise an explicit value, not an `Err`. Note
`read_state` probes `schema_version` **before** the full parse: deserializing
straight into `EnvironmentBuildRecord` would fail on the first unknown field,
so a newer hitch's record would be misreported as a corrupt blob instead of
as "written by a newer hitch".

P3 is the reader, and it renders this as a first-class state rather than as
missing work: `hitch status` prints "Actual unknown — no build record for this
environment", keeps its exit code at 0, and does **not** count it under "need
rebuild" (`is_actionable()` is false for `LegacyUnknown` — it is a different
signal, not a stale one), while still offering `hitch rebuild <env>` as the
thing that would make it known. `test_hitch_status_renders_legacy_unknown_and_still_exits_zero`
holds that, and
`test_an_environment_built_without_a_record_is_legacy_unknown` simulates a
pre-P2 repo by building for real, asserting `refs/hitch/state/dev` exists so
the test cannot pass vacuously, and then `git update-ref -d`-ing it. Note the
real-world driver of this state is a **repo last built by a pre-P2 hitch**, not
`hitch release` — a release prunes the integrated environment branch, so it
reads as `MissingBranch` (with `ActualComposition::LegacyUnknown` underneath),
and its default dependent rebuild writes a record for everything else. One
ordering rule in `src/core/state.rs`: a *declared branch with no ref anywhere*
is decided before the record is read, because a missing branch is missing
whether or not we happen to know what was in the last build.

**The state ref's `expected_old: Some(String::new())` is deliberate, and is
NOT the publish-journal CAS mistake described under `publish_branch` above.**
That entry's rule is about a *leftover record* being able to fail a
transaction; here the opposite is true and the ref **must** be
unconditionally writable. There is one live record per environment replaced in
place, so under `RefEdit::Create` semantics the *second* rebuild of every
environment would fail the whole batch — wedging publishing after exactly one
successful build. A CAS buys nothing extra: the edit rides inside the same
all-or-nothing batch as the branch move, which is itself CAS-guarded, so
"unconditional" cannot clobber a concurrent publisher.

**`metadata_sha` in a build record is a recorded fact and nothing more — it
is not a usable staleness signal, and it looks broken if you expect it to
equal `hitch-metadata`'s tip.** A rebuild brackets the commit it records on
*both* sides with its own metadata writes: `with_locked_env` commits the
lock before the declaration is read, and publishing commits the `rebuilt_at`
stamp (`update_rebuilt_timestamp_for_rebuild` → `modify_metadata`) and the
unlock after. So the recorded SHA is a commit that only ever existed as a
transient tip, is not observable from outside the command at all, and is
always a strict *ancestor* of the branch's final tip. On top of that it moves
when an unrelated environment is promoted and returns to a previous value
when a declaration edit is reverted. Staleness is decided by comparing
`desired_branches` against the live refs — see `RepositoryStateSnapshot` in
`src/core/state.rs` — never by this field.

**The staleness verdict is a SHA comparison. Reintroducing a timestamp
comparison is a regression, and the tests are built to catch it.** Until P3,
"does this environment need a rebuild?" was answered by asking whether the
newest commit among the environment's inputs was newer than a wall-clock
`rebuilt_at` read out of `hitch.json`. That is wrong in three independent ways,
all of which were live in shipped code: a rebased or cherry-picked branch
produces a commit whose *date* can easily be older than the last build while
its *content* is completely different, so a rebased feature read as up to
date; a `hitch promote --no-rebuild` moves nothing's date, so it read as up to
date too; and a clock skew or a backdated commit flips the verdict either way.
The check also could not see a *removed* branch at all. It is now
`EnvironmentHealth::NeedsRebuild { changed_inputs }` in `src/core/state.rs`,
comparing the record's pinned `desired_branches` SHAs and base SHA against
freshly pinned ones, and naming what moved as `previous → current`. The
regression tests are the point: `test_a_rebased_feature_is_needs_rebuild_even_though_its_commits_are_newer`
and `test_a_backdated_commit_still_counts_as_needs_rebuild` fail if the clock
is consulted, and `test_hitch_status_detects_base_branch_changes` /
`test_hitch_status_multiple_envs_with_changed_base` used to be `#[ignore]`d as
"timing-sensitive" for exactly this reason — they are live now and their
`sleep(2)` is gone. Do not reintroduce `get_commit_timestamp` into any
verdict: it has no production caller left (`core/timeline.rs` is now an
adapter over `activity.rs`, which takes dates from `list_first_parent_history`).

**`removed ⊆ changed_inputs` is an invariant of `health_from_record`, and the
Result block depends on it.** `health_from_record` walks the *recorded* pins
and compares them against live ones; a branch that has since left the
declaration resolves to no current SHA, so it necessarily appears in
`changed_inputs` as well as in `removed`. That is why `render_resulting_state`
can skip a changed input whose branch is in `removed` without a second
condition — printing both is the same fact twice, and the fact is the removal.
`added` is disjoint from `changed_inputs` by construction and needs no such
guard. If `health_from_record` ever starts comparing a *declared* list against
live refs instead of the *recorded* one, this stops being true and the dedupe
silently starts eating a real change.

**A display of a verdict must read the verdict, not re-derive it — and
`hitch status` used to derive it four times.** The old status command called
`determine_rebuild_state` in the per-environment renderer *and* in both summary
blocks, so one `hitch status` computed the same answer once per environment in
the top summary, again in the body, and again in the bottom summary. That is
the shape of bug where two of the three can drift. `build_status_model` is now
a pure function of the snapshot, `commands/status.rs` formats the snapshot
rather than deciding anything, and
`test_the_snapshot_and_the_status_model_never_disagree` plus
`test_hitch_status_renders_exactly_what_the_snapshot_reports` (both in
`tests/integration/state_model_tests.rs`) hold the two together. A new display
path should take a `&EnvironmentState`, not a `GlobalContext`.

**Two display projections of the same snapshot are functions whose *arm order*
is load-bearing, not style.** `MatrixCell::classify` and `reason_for` (in
`core/why.rs`) are the two: both answer "what is the most specific true thing I
can say?", and both get it wrong in the same way if a more general arm is
matched first. `classify` puts *not declared* ahead of everything, because a
branch nobody declared has no membership story to tell; `reason_for` gives the
branch itself first refusal, then the environment-level condition, then *this
branch's* staleness, then the **base's** (`BaseMoved`), then a catch-all. The
concrete trap is `NotDesired`: it is fully answerable from the declaration
alone (`DemotedSinceBuild` when the record's `removed` names the branch, `None`
otherwise), and if it falls through to the environment-level arms a *stale
environment* ends up explaining a branch that was never in it — a confident,
specific, wrong sentence. That is why the `NotDesired` arm is an early `return`
rather than a match arm. A new cell state, or a new `WhyReason`, has to answer
the same question before it is written: what is the *least* it can honestly
claim, and can anything more general get there first?

**A fact and a prediction must not share a glyph, a word, or a code path.** The
⛔ on a promoted branch used to always read "(conflicts with X — held on
rebuild)", but it was driven by `preflight_compatibility_report_local`, which
answers "would the *next* build hold this?" — a prediction, dressed as a
statement about the branch in front of you. Now the record's `held` list
(a fact about the last build) is consulted first and the preflight only fills
the gap, and the two are worded differently: "held in the last build" versus
"would be held on the next rebuild". Same glyph, because both are worth a
glance; different words, because they call for different urgency.
`test_status_distinguishes_a_held_branch_from_one_that_would_be_held` holds
both arms. The same rule is why `preflight_compatibility_report` is still a
*prediction* everywhere it is called from — do not promote a prediction into a
verdict just because one is more convenient to compute.

**`build_state_snapshot` is offline, and that is a guarantee not an
accident.** It resolves each declared branch with `rev_parse_opt
refs/heads/<b>` and falls back to the cached `refs/remotes/origin/<b>`; it
never calls `ls-remote` and never fetches. Two consequences worth knowing.
First, a branch that exists only on the remote and has never been fetched
resolves to `None` and is classified `MissingBranch` — correct, because
hitch could not have built from it either. Second, `hitch status` used to
call `branch_exists_anywhere` once per promoted branch inside its render
loop, which is a `git ls-remote --heads origin` per branch: a network round
trip per row, in a read-only command. The renderer now reads existence from
the snapshot it was handed, so the command is genuinely offline end to end.
If you add a per-branch check to a status-style path, check the snapshot
first.

**`hitch rebuild`'s exit code 2 is a CI contract, not a bug.** `rebuild` is
the only command in the CLI with a non-0/1 exit code: `rebuild::run` returns
`Ok(true)` when it succeeded *but held conflicting branches*, and `main.rs:122-129`
turns that into `exit(2)` so a pipeline can warn on holds without failing the
build. `Ok(false)` falls through to 0; any `Err` — including a halt-policy
refusal — takes main's normal error path to 1. Note what that makes the return
value useless for: `Ok(false)` is *both* a clean apply with no holds *and* a
declined confirmation, so a test of "declining changes nothing" has to assert on
the repository (no `refs/heads/<env>`, no `refs/hitch/state/<env>`, no anchor,
`rebuilt_at` still `None`), never on the bool. `a_declined_rebuild_exits_zero_
and_writes_nothing` is the shape, and note that it has to build its own context
with `assume_yes: false` and `no_push: false`: a rebuild only *requires*
confirmation when it owes a push, so a `--yes`-assuming context never reaches
the `Confirm` it was handed and the test would pass vacuously.

**`--json` without `--yes` exits 1, and it used to exit 0 — do not restore the
0.** The gate's refusal under `--json` is an `Err`, not a decline, because a
JSON consumer is a program and "asked nothing, wrote nothing, exited
successfully" is indistinguishable from success to one. `decide_gate` returns a
three-way `GateDecision` (`Proceed` / `Ask` / `Refuse(reason)`) rather than a
bool precisely so that refusal and decline stay separate arms: the first
version of `rebuild_environment_gated` matched `Ok(false) | Err(_)` together,
discarded the anchor, and returned "declined", so `--json` without `--yes`
exited 0 with an empty stdout and the "re-run with `--yes`" reason thrown away.

`--dry-run` uses the same exit-2 signal: `Ok(true)` means "would hold", so a
dry-run preview exits 2 as well. Collapsing `AppliedWithHolds` into a plain
success would silently break every consumer relying on that distinction. The
`stdout().flush()` immediately before the `process::exit` is load-bearing too —
`process::exit` skips normal shutdown, so buffered output is lost when stdout
isn't a TTY, i.e. in CI. Under `--json` that same flush is what keeps a held
rebuild's *document* on stdout across the exit, which is the whole reason the
flag and the exit code can coexist; `a_json_document_survives_the_exit_two_
that_it_exists_to_explain` holds it.

`OperationOutcome::AppliedWithHolds` is the *typed* expression of the same
fact — `apply_rebuild_plan` returns it, and `rebuild::run` still returns
`Ok(true)` for the same condition. The two are not interchangeable: the enum is
what a receipt records, the `bool` is what `main.rs` turns into exit 2. Adding
a third path that computes holds its own way is the drift to watch for;
`tests/integration/plan_apply_tests.rs` checks both the library verdict and the
CLI exit code in the same test, so they cannot quietly diverge.

**A plan is a decision, not a recipe — and the reason is the wall clock.**
`plan_rebuild` *composes* the environment and carries the resulting commit;
`apply_rebuild_plan` lands that commit rather than recomposing one. This is not
an optimisation. `commit_tree` stamps the ambient time with no
`GIT_AUTHOR_DATE` override, so re-composing the same inputs a second later
yields a **different SHA** — a re-planning executor would publish a commit other
than the one its own receipt promised, and the `^{tree}` content would be
identical, so nothing else would catch it. Three consequences follow, all
already bitten or already guarded:
- Two compositions of identical inputs must be compared on `^{tree}`, never on
  the commit. This is the same trap the crash-fuzz convergence check in
  `crash_recovery_tests.rs` already documents.
- The commit is unreachable until the CAS lands, so the plan **anchors** it
  under `refs/hitch/build/<env>/<sha>` for the window between planning and
  publishing, exactly as `rebuild_environment_opts` always did — moved earlier
  in the sequence, not a new mechanism.
- That anchor is a live leak, because nothing prunes `refs/hitch/build/*`
  (`cleanup`'s prunable set is `["backup", "prev"]` — see the `state/` entry
  above for the sibling trap). `apply_rebuild_plan` therefore calls
  `discard_plan` as an unconditional `finally` around its inner call, **not**
  as a numbered step: the exit path most likely to be missed is the `?` on
  `validate_plan`, which fires before anything else has run and is invisible on
  a green run. Do not "tidy" it into a step, and give every new planner the
  same `finally`.

**`PlanPurpose` is an enum because "a preview that mutates" must be
unrepresentable.** `Preview` differs from `Confirm` in *all three* ways that
make `--dry-run` safe — it does not synchronise branches, does not take the
environment lock, and does not anchor. Encoding them as one variant means a new
caller cannot opt into safety by omission, and P1's deliberate
`synchronize: false` asymmetry for previews is preserved by construction rather
than by a comment. `--dry-run` in `commands/rebuild.rs` is now a *renderer*
over `plan_rebuild(…, PlanPurpose::Preview, …)`, so it reads
`plan.detail.held` / `plan.detail.replayed` and never calls
`compose_environment` itself. A preview's only *real* asymmetry with a real run
is the lock: `with_auto_stash` turns out to be a no-op difference for all four
mutating commands, measured — none of them moves the user's `HEAD` — and it is
skipped in a preview anyway. A preview's missing `synchronize` is ordinary
staleness (a stale local branch makes the preview describe older content),
categorically weaker than the two-merge-engines bug P1 removed. If a dry run
ever needs to be *exactly* predictive the fix is to make sync a shared,
user-visible step, not to re-add a second merge path.

**Every arm that does not apply the plan owes a discard of its anchor —
decline *and* error.** This is the rule that `rebuild_environment_gated`
(`Ok(false) => { discard; None }` / `Err(e) => { discard; return Err(e) }`, with
the discard first on both so `?` cannot skip it) exists to make hard, because
the two failure modes look alike at the call site and only one of them was
written: `release` discarded on a decline and not on an `Err`, so a `--json`
release without `--yes` leaked `refs/hitch/release/*`, a family **nothing
prunes** — one permanently leaked ref per refusal, on a path that looks like it
did nothing. `promote`/`demote` escaped the same bug only because
`plan_declaration_change` composes nothing and therefore anchors nothing, which
is exactly why the rule is about *arms* and not about commands: a new planner
that anchors inherits the bug without inheriting the history.

**A `ConfirmationRequirement.reason` that no code path prints is a prompt that
says nothing.** All three planners carry the reason, and a unit test asserted
`render_plan` does *not* show it — which was right, and hid the gap, because the
field had no other reader. Every prompt in the CLI read `Apply this plan?`. The
worst case is promote into an approval-gated environment, where the plan's "Will
change" section is *empty*: confirming files an approval request instead of
editing the declaration, so the user was asked to authorise a plan that visibly
does nothing, with no statement of what the answer would do.
`confirmation_question(&ConfirmationRequirement) -> String` is now pure and
tested, and the approval reason is phrased as *what confirming will do* rather
than as a restatement of the warning. Generalise: a reason carried on a model
type is not documentation, it is a field someone has to print, and a field
nobody prints is a lie about what the code does.

**A halt is decided inside composition, so a *rebuild* plan can never report
one.** `OnConflict::Halt` returns `Err` from inside `compose_environment`, which
means the plan is never built at all. That is the right behaviour — the
operation refused rather than partially applying — but it makes
`PlanWarning.blocking` unreachable from `rebuild` and
`PlanApplyError::PolicyBlocked` unconstructed *there*. Do not "fix" the halt by
moving it after planning; that would introduce a plan for an operation that
never happens. A manual check confirms `--on-conflict halt` still exits 1 with
the single `format_compatibility_report_for_rebuild` report and its
`git checkout … && git rebase …` next step.

Both members became reachable in P5, from the *declaration* planner, which
refuses before composing rather than during: `PlanWarningKind::PolicyRefusal`
and `PlanWarningKind::ApprovalRequired` are both `is_blocking()`, and
`apply_declaration_plan` raises `PolicyBlocked` from `plan.blocked_by()`. So
the distinction is now drawn along a real line: a halt is decided by the merge,
and a policy/approval refusal by the plan. If you add a third kind of
pre-composition refusal, it belongs in the planner for the same reason — and
note that `approval_gated = refused.is_none() && declared.requires_approval_check()`,
because a policy refusal *outranks* the approval gate: asking for approval of
an operation that will be refused asks for nothing.

**`publish_branch` returns `PublishOutcome { push, journal_cleared }`, and
that is not optional detail.** A receipt cannot be assembled from `Ok(())`:
"published and pushed", "published, push declined", and "published, push
failed — the journal record is still owed" are three different truths, and the
`PushOutcome` is what distinguishes them. `a_failed_push_is_reported_as_owed_rather_than_as_fully_synced`
in `plan_apply_tests.rs` is the test that fails if a failed push is ever
flattened into a success; it uses a `pre-receive` hook in a bare origin so
`fetch` still works and the failure is unambiguously a *push* failure. A
declined push still fires `maybe_abort_for_test("push-succeeded")`,
`mark_push_done`, and `clear` — refusing to push still settles the obligation.

**A fingerprint is a whitelist of what a plan depends on, not a snapshot of
every ref.** `PlanFingerprint { metadata_sha, refs, remote_refs,
resolution_keys }` names the environment's base, its promoted branches, its own
ref, the remote-tracking refs it read, and the resolution keys it replayed.
Adding an unrelated branch does not make every plan stale, and that is
intentional. Resolution *keys* suffice without the blobs because a key is a
content hash — a key match **is** an identical-content match, so putting the
blobs in the fingerprint would buy nothing. A resolution that has *disappeared*
counts as a change, not a non-event: the replay would now miss, so the branch
would be **held** instead of composed, which is a materially different
operation. `PlanFingerprint::digest` is a git object hash over a hand-rolled
canonical encoding (via `hash_object_bytes`, the same mechanism as
`resolutions::resolution_key`) — deliberately not `serde_json`, whose output is
not stable enough for "same inputs → same digest" to be a testable property, and
deliberately not a new `sha2` dependency.

**One planner per *operation*, and promote/demote are one operation.**
`PlanApplyError` derives `std::error::Error` and crosses the boundary via
`into_anyhow` rather than being stringified, so
`err.downcast_ref::<PlanApplyError>()` still yields a `StalePlan` with its
`changed` list. "Refused *because it went stale*" has to stay a distinguishable
claim, not "something failed" — that distinction is the entire value of the
error type. `validate_plan` is `pub` so tests can assert a refusal with the
repository unmoved, which is what distinguishes "the validator said no" from
"the validator said no and the apply then stopped". The "one planner" side
means one *plan* per unit of intent, not one per command: `promote` and
`demote` are the same edit to a declaration in opposite directions, so they
share `plan_declaration_change`/`apply_declaration_plan` in
`src/operations/declaration.rs` and differ only in `OperationKind`,
`OperationIntent`, and which pure helper computes the proposed list
(`proposed_declaration`). Two planners for those would be two places to keep
the same three steps (edit, snapshot, rebuild) in agreement, and the agreement
is not checked by any compiler.

**A plan must be built after every metadata write that precedes it — including
the ones hitch makes on its own behalf.** `PlanFingerprint` includes
`metadata_sha`, and `with_locked_env` commits the environment's lock to
`hitch-metadata` *before* running its closure, so a plan built outside the lock
is stale the moment it is validated. This was measured, not reasoned: 13 of 17
promote tests failed with `The plan for 'dev' is no longer current:
hitch-metadata: d3da09e → 82658aa`. So `promote`/`demote`/`release` build
their plan *inside* `with_locked_env`, which has a knock-on the planner must
absorb: by then the environment is locked by the command's own hand, so
`plan_declaration_change` must **not** call `is_locked()`, and the human-lock
refusal lives in the command instead (the same place `commands/rebuild.rs` puts
it). `ensure_environment_exists` moved into the command for the same reason —
from inside the lock, a missing environment is indistinguishable from a lock
conflict, and two tests were getting the wrong message.

**`modify_metadata`'s closure runs *before* its commit — so nothing inside it
may read the config back off the ref.** The closure is handed a
`&mut HitchConfig` and the file is written afterwards, while
`read_file_from_branch` is `git show hitch-metadata:hitch.json` and
`begin_branch_write` only sets up a scratch index. So a closure that calls
`rebuild_environment` composes from the *pre-edit* declaration while the
declaration is being changed under it. This was live in
`approvals/approve.rs`: an approved branch landed in `hitch.json` and never in
the environment branch, and the two disagreed until something unrelated
triggered another build. Same bug class as the wrong-merge-base one above — a
shape that reads correctly and is not. Since P8 the approval's own
`modify_metadata` only records the vote; the promotion is then planned by
`plan_approved_declaration_change` and applied by `apply_declaration_plan`
*after* that transaction returns, still inside `with_locked_env`, so its nested
rebuild reads the committed declaration. Pinned by the `cat-file` assertion in
`test_automatic_application_on_threshold`. Generalise: if a new command wants to
"update the declaration and rebuild" as one step, that rebuild goes *after* the
transaction returns, not inside the closure.

**A metadata operation has no rollback, because its whole effect is one
closure that runs before its commit.** `set`, `add`, `remove`, `lock`, `unlock`
and `hitch approve` each make their edit inside one `modify_metadata` closure,
and `modify_metadata_impl` runs that closure *before* `write_file` /
`commit_branch_write` — so a closure `Err` has committed nothing. A rollback
there restores a snapshot onto a ref that already holds it: `approve.rs`'s two
`attempt_*_rollback` helpers did exactly that on every path that reached them,
costing an extra `hitch-metadata` commit per refusal and narrating a repair of
nothing. They are gone. Promote and demote keep theirs only because their
nested rebuild can fail *after* the edit commits. Do not add a "just in case"
rollback to a metadata operation; a refusal's cost is exactly the lock and
unlock commits, which the crash-recovery tests read.

**A `DeclarationChange` variant is how a new direction joins an existing
planner — not a new parameter, and not a new planner.** `hitch approve` needed
promote's plan with the approval gate already satisfied. A `skip_approval: bool`
on `plan_declaration_change` would be a promote with a hole in it that any
caller could open; a separate planner would duplicate the edit → snapshot →
rebuild steps that must stay in agreement. So it is
`DeclarationChange::ApprovedApply { direction, .. }`, and `kind()` is a method on
the change, so the plan's kind, headline and `command_hint` remedy (`hitch
approve …`, not `hitch promote …`) cannot disagree with the change they
describe. Direction is a *field*, because every downstream arm asks only "which
way" and "already authorised?", and a demotion can be approval-gated too.

**A failed dependent rebuild is an owed effect, not an error, and
`rollback_metadata_changes` cannot repair it.** Promote, demote, and release
all used to roll the declaration back when a nested rebuild failed. That was
never safe: `rollback_metadata_changes` restores a *whole-config snapshot*,
while a nested rebuild can fail *after* moving the environment branch — at which
point the snapshot describes a state that is not in the repository. (The
rollback's own new test fixture does this deliberately: a push error, which
lands the env branch first.) The contract now, identically in all three: exit
0, the durable effect persists (the declaration edit, or the merge-and-tag),
the environment is left unbuilt, and the message names `hitch rebuild <env>`.
Rollback stays reachable for the one failure it can actually repair — the
metadata write itself — and `test_rollback_...` in
`approval_workflow_tests.rs` exercises that. The typed form is
`ExecutionWarning { owes_effect: true }`, which is *not* the same as a plain
warning; P6 renders the difference.

**A receipt's warnings are what the *apply* learned, and copying a plan's
warnings into them is a prediction in the wrong tense.** All three executors
used to open with the same three lines — `plan.warnings`, filtered to
non-blocking, mapped into `ExecutionWarning { owes_effect: false }` — which put
every advisory in two documents, verbatim. It read as four separate double
prints, and they were worth separating because each said something slightly
different about the same rule:

- `hitch rebuild`'s hold advisory: **"will be held out of this build"**,
  re-printed inside a receipt whose subject is what already happened. The hold
  is decided by `compose_environment` at *plan* time — the apply learned
  nothing — and the fact that belongs in the receipt is already in
  `resulting_state` as `PartiallyRealised { held }`, read from the authority.
- promote/demote's `--no-rebuild` advisory: **"will be left stale until it is
  rebuilt"**, which the `Result` block also says, as `⧗ dev   needs rebuild`.
  This one also had a **fourth** copy: a `log_info` "Skipping rebuild for
  environment 'dev' (--no-rebuild flag set)…" sitting in the gap between the
  halves — the same second voice the nested-rebuild transcript was, for a
  decision the user had just made with a flag.
- release's advisories: `--no-prune`, `--no-rebuild-dependents`, and the
  per-environment stale skips. All consequences of flags, all decided before the
  apply began. Their facts are the *absence* of a prune effect and the Result
  block.
- the `ApprovalRequested` arm, which copied **all** warnings including blocking
  ones: so an approval-gated promote printed `⛔ Environment 'prod' requires
  approval before promotion` in the plan and then the identical sentence again
  as `⚠️` in the receipt. Same words, **different glyph** — which is worse than
  the duplication, because a blocking plan warning renders `⛔` and a non-owed
  receipt warning renders `⚠️`, so one fact wore two urgencies in one document
  pair.

The rule is now on the field (`ExecutionReceipt::warnings`): three documents
describe one operation and each has a job — the plan says what is *about* to
happen, `effects` says what happened, `resulting_state` says where things
stand, and `warnings` is the only one for a fact discovered *while* applying (a
push that failed, a nested rebuild that did not run — the plan could not have
known those, and the resulting state may look normal despite them). Every
current producer sets `owes_effect: true`, so the non-owed branch in
`render_receipt` is reachable only from a unit test; the field stays, because
the distinction is real, but a new *non-owed* receipt warning is a signal to
check whether the thing is a plan warning or a resulting-state fact wearing a
receipt's clothes. `a_receipt_never_restates_a_plan_warning` in
`plan_apply_tests.rs` asserts the generalisation across all three operations
rather than per site, because the failure mode is a *new* planner reintroducing
the shape — and it has teeth: re-adding the copy in `rebuild.rs` alone makes it
fail. **The cost is real and was accepted deliberately:** a `--json` consumer
reading only `receipt.warnings` loses the hold's *partner* and file count. It
loses the branch names, the typed `AppliedWithHolds`, and the whole plan half,
so nothing is lost from the document; and a consumer that wanted a prediction
was reading the wrong half.

**A rollback snapshot has to be captured on the *far* side of the lock, and a
rollback has to be armed only once the operation is about to write. Both halves
are load-bearing, and getting either wrong turns a refusal into a wedge.**
`capture_config_state` ran *inside* `with_locked_env` while
`rollback_metadata_changes` runs *outside* it, in the command's `Err` arm, by
which time the unlock has already committed. So the snapshot recorded
`locked: true` and restoring it put the lock *back*: every later promote refused
with `Environment 'dev' is currently locked by 'test@example.com'`, naming a
holder that had gone away, and the only way out was a manual `hitch unlock`. The
comment that justified the ordering ("capturing before would undo the lock's own
commit") treated the rollback as a history rewrite; it is a *later* commit, so
restoring the pre-lock value is what leaves the environment correct. The fix
moves the capture *before* `with_locked_env` — safe because nothing else writes
`hitch-metadata` in the window, as the planner composes and anchors nothing —
and arms `previous_config` only after the confirmation gate, immediately before
the apply. Arming matters as much as placement: an unconditionally-captured
snapshot is a snapshot of a repository the operation never touched, so every
refusal used to run a rollback that repaired nothing, cost two extra metadata
commits, and printed three lines narrating a repair that had not happened. This
bit on the *most ordinary* refusals there are — a planner refusal never reaches
the apply, so it never armed, and yet it rolled back. `a_refused_promote_leaves_the_environment_unlocked`
and `a_refused_promote_writes_no_metadata_commit` in
`tests/integration/promote_demote_tests.rs` hold both halves, and the armed path
separately (`a_rollback_that_fires_still_leaves_the_environment_unlocked`).
The one failure that genuinely reaches an armed rollback is the metadata write
itself, and the expected commit count on a refusal is therefore **2** — the lock
and the unlock, which the crash-recovery tests depend on for the visible-lock
signal. Do not "tidy" that constant to zero.

**A cause is reported once, by `main`, and a command that also reports it is
running a second voice on the same failure.** `promote` and `demote` both
`log_error`'d `Error: {e}` and then returned the same error, so `main` printed it
again as `Error: {e}` — one sentence, two prefixes, two lines on stderr. Every
other command in the CLI lets `main` do it, and that consistency is the reason the
extra line was pure loss. The rollback narration that used to sit between them
(`Operation failed, attempting automatic rollback…` / `Rolling back …` /
`You can now retry …`) is gone from the refusal path entirely, which removes the
ordering argument for printing the cause first; on the armed path the narration
now reads as the explanation *for* the error rather than a surprise after it.
Two smaller instances of the same class, both fixed: `rollback_metadata_changes`
logged two `CRITICAL:` lines and returned the error for the caller to log a third,
and passed `"✓ Automatic rollback completed successfully"` to `log_success`, which
already prefixes `✅` — the glyph printed twice. The `log_*` sinks own the glyph
(see `utils/output.rs`); a message must not carry its own.

**A refusal's cause is in the plan, and `PolicyBlocked`'s `Display` does not
repeat it.** The plan prints the blocking warning under "Why this cannot apply"
and then the apply raises `PolicyBlocked`, so a `Display` that restated `reason`
said one sentence twice (`lock` on a locked environment). It now says "for the
reason given in the plan above", then `Nothing was changed.`, then `To proceed:`
and the remedy — so tests assert the *cause* on stdout (the plan), not stderr.
A refusal that is only "already so" (`unlock` on an unlocked environment, `add`
of an existing environment on the same base) is `PlanWarning::with_nothing_to_do`,
`remedy: None`, and prints no `To proceed:` at all: a non-action under that
heading answers a question nobody asked.

**`hitch cleanup` plans only deletions `git branch -d` will accept, and a delete
that fails anyway is a failure, not an owed effect.** The planner asks git's own
question (`GitOperations::branch_is_merged`: tip is an ancestor of the upstream,
else `HEAD`) and puts a branch that fails it in `CleanupPlanDetail::unmerged` —
kept, named in an advisory with the `git branch -D` the *reader* can run; hitch
never forces, because deleting unmerged work is data loss. `HEAD` and the upstream
are in the fingerprint, so the prediction is one the validator can check. What
`apply_cleanup_plan` still cannot foresee (a stale ref lock) comes back in
`CleanupRun::failures`; the command prints the receipt of what applied and *then*
fails (exit 1). It is not `Still owed`: nothing retries a cleanup. The receipt has
no `resulting_state` — a `Result` block of every environment under a branch
deletion is noise. Under `--json` the failures also ride in the document as a
typed `failures: [{refname, cause}]` (`emit_receipt_with_failures`; the key is
absent when nothing failed). A branch checked out in *any* worktree is kept
(`CleanupPlanDetail::checked_out`, advisory names the path) whether or not it is
merged, and the apply uses `delete_branch_strict` — `delete_branch` escalates a
"used by worktree" refusal to `-D --force` and `update-ref -d`, so cleanup must
not reach it. Never parse `git branch --list` for names: it decorates `* ` and
`+ ` (worktree); `list_local_branches_with_prefix` uses `for-each-ref`.

**`approvals approve` gates after its read-only answers and before its first
write, and a vote below the threshold still emits a document.** The `--json`
without `--yes` refusal (`refuse_unconfirmable`) sits after the status checks so
"already applied" is not replaced by "needs --yes", and before `validate_and_approve`
because the vote is a committed write. A below-threshold vote has neither plan nor
receipt, so `emit_approval_recorded` prints `{schema_version, plan: null, receipt:
null, approval: {request_id, environment, approvals, required, threshold_met,
remaining_approvers}}` — same envelope keys as every mutation, and the words live
in `render_approval_recorded`.

**A decision the plan can make at plan time belongs in the plan, and a
decision about the release's own result must be evaluated against the planned
result, not the live ref.** Two instances. (1) `plan_dependents` runs
`preflight_compatibility_merge_tree` and leaves a provably-unrebuildable
environment out of `dependents` entirely, as an `Advisory` warning —
`DependentRebuildOutcome::Skipped` is then for *runtime* skips only (a base
that failed its own rebuild, `--no-rebuild-dependents`). A plan that declares a
rebuild it knows cannot happen is a lie; a receipt with nowhere to put a
genuine skip is the lie of the other kind. (2) The prune predicate is "is this
promoted branch now contained in this environment's base", and for every
environment based on the released target the answer changes *because of this
release* — so it is evaluated against the composed `result_sha`, not the live
target ref. Evaluated against the live ref it answers "no" for exactly the
branches the release just integrated, and prunes nothing, silently. Same
reason `target_sha_before` reads `rev_parse_opt("refs/heads/<target>")` and not
`get_branch_commit_sha`, whose remote fallback would hand the CAS an
`expected_old` belonging to a different ref.

**`prunes` and `dependents` are computed in environment *name* order, not map
order.** `HitchConfig::environments` is a `HashMap`, so iteration order is
arbitrary. Two releases of identical input must produce identical plans, and a
plan whose effect list reorders between runs is not a plan;
`plan_dependents` iterates `topological_environment_order` and `plan_prunes`
sorts by name. `a_release_plan_names_the_tag_the_target_move_the_prunes_and_the_dependents`
asserts the exact order.

**Wrong merge-base in `merge-tree` preflights.** `git merge-tree --merge-base
<X>` needs the *true common ancestor* of the two trees being compared —
computed with `get_merge_base(a, b)` — never a branch's own current tip.
Passing the tip makes `merge-tree` treat that side as unchanged since the
(wrong) merge-base and silently fast-forward instead of reporting a real
conflict. This bug existed in this codebase's preflight functions for a long
time, invisible because every existing test's conflict scenario had an
*unmoved* base (where the wrong merge-base happens to equal the right one) —
it only surfaced when a base-moved-after-branch-diverged scenario was
manually tested end-to-end. If you touch any `merge-tree` invocation, be
suspicious of this exact mistake and test the base-moved-independently case
specifically, not just the peers-diverged-from-an-unmoved-base case.

The composition path (`merge_tree_compose`) sidesteps this entirely by *not*
passing `--merge-base` — git computes it, including the virtual base for
criss-cross histories, exactly as a real merge does. Don't "helpfully" add an
explicit `--merge-base` there. The gotcha applies to the preflight callers
(`merge_tree_write_tree_name_only`) that do pass one.

**One composition per *kind* of composition — and release's is a different
kind, not a third caller.** `compose_environment` (`src/utils/prelude.rs`) is
the *only* place an **environment build's** conflict verdict is reached. Since
P4 that is `plan_rebuild` (`src/operations/rebuild.rs`) alone: a real rebuild
reaches it by way of a `Confirm` plan, and `--dry-run` by way of a
`Preview` one, both over the same `pin_environment_inputs` result. That is the
invariant, and it was
expensive to establish: `--dry-run` used to short-circuit into
`preflight_compatibility_report`, a *tree-based* loop over
`merge_tree_write_tree_name_only` with an explicit `--merge-base`, while the
build was a *commit-based* loop over `merge_tree_compose` (ORT, no explicit
merge-base). Two doors into the merge engine, so two verdicts were possible,
and the live symptom was `rebuild <env> --dry-run --replay-resolutions`
reporting branches as **held** that the real build **composed** from the
recording — exit 2, "would hold", for a build that was going to succeed.

P5 removed release's version of that bug, which is worth naming because it
looked nothing like the rebuild one: `rebuild_dependent_environments` ran
`preflight_compatibility_merge_tree` at apply time and *skipped* a dependent
environment whose composition would conflict. Release's own merge — the chain of
promoted branches into the target — was never a preflight, and could not be: it
is a different operation from an environment build. It merges N branches into a
branch that already has content, where `compose_environment` builds a fresh
environment from `base + branches` and resolves conflicts by *holding* a branch
rather than aborting. So `compose_release` (`src/operations/release.rs`) is a
second composition, deliberately, and the rule to carry is not "one composition"
but "**one composition per kind, and no kind reached by two doors**". A release's
merge chain is all-or-nothing (a conflict returns `Err` and nothing is written);
an environment build ejects and continues. Collapsing them would take the
eject-and-continue policy away from builds or the all-or-nothing property away
from releases.

`preflight_compatibility_report` is **not yet** a display-only function, and
the remaining mutation that depends on it is `hitch resolve`, not `rebuild`
and not `release`.
`commands/resolve.rs:131` and `:180` use it to decide Mode A (rebase the branch
onto the base) versus Mode B (peer conflict), and to refuse outright when it
reports no conflict at all — so a preflight/composition disagreement would
there pick the wrong resolution mode, not merely print a stale preview. The
read-only callers are `conflicts.rs:44` and, via the offline
`preflight_compatibility_report_local`, `status.rs:238` and `tree.rs:138`; those
are legitimate, since a display preflight is allowed to approximate. Routing
`resolve`'s mode selection through the shared primitive is **still not done**,
and it is now the *only* one left: P1 scoped itself to `rebuild`'s dry-run, P4
built the planner `resolve` would need to choose a mode from, and P5 closed
release's. Do not add further dependants in the meantime.
Two tests hold the `rebuild` half: `test_dry_run_agrees_with_real_build_about_replayed_resolutions`
(resolve_tests) and `test_dry_run_and_real_build_agree_on_held_branches`
(rebuild_tests). Both compare the *verdict*, not the rendered prose, on purpose
— the two paths legitimately word the same event differently.

**A nested operation that sits between a plan and a receipt must narrate
nothing, and the way to guarantee that is a suppressed-by-default parameter,
not a flag each caller remembers to pass.** Four commands (`promote`, `demote`,
`release`, and `hitch rebuild`'s nested calls) each build a plan above a nested
rebuild and a receipt below it, and each was separately printing the nested
rebuild's `StepLogger` transcript — `[1/6] Synchronizing branches`, `[2/6]
Merging 'feature/payments'`, `✅ Rebuilding environment 'dev'` — into the gap
between them. The in-tree comment on `rebuild_environment_opts` claimed the
nested path printed the transcript *instead of* a plan and receipt; it did not,
it printed it *in addition to* both, and then threw both away.
`StepNarration::Suppressed` is now the default, so a caller that has a plan and
a receipt of its own gets silence by omission. Since P8 gave `hitch approve` a
plan, **every** caller passes `Suppressed` and `StepNarration::Log` has no call
site at all — the variant, the enum, and the `on_step` callbacks threaded
through `plan_rebuild`/`apply_rebuild_plan`/`compose_environment` are dead
plumbing left for P10's legacy removal, not a hook to reach for. If you add a
mutating command, it has a plan and a receipt, so it narrates nothing. Generalise: a second voice for the same operation is a bug even
when it is individually accurate, and the parameters to thread are usually
better off inverted so silence is the default.

**An `Ok(_)` on a nested build's conflicts is a dropped fact, and it renders as
a false success.** `apply_declaration_plan` and `apply_release_plan` each ran a
dependent rebuild and discarded its `Vec<CompatibilityConflict>`, so a receipt
printed `✓ rebuild dev` for an environment that was in fact *holding a branch* —
the exact flattening `OperationOutcome::AppliedWithHolds` exists to prevent,
arriving through a different door than the one that was guarded. A nested
operation's outcome is part of the parent's outcome, so it has to ride on the
parent's `AppliedEffect`: `AppliedEffect::DependentEnvironmentRebuild` carries
`held: Vec<HoldPair>`, and the renderer's glyph ladder for that effect is `⧗`
if the outcome owes an effect, else `⚠️` if anything is held, else `✓`. The
middle rung is the argument — a hold *did* rebuild, so `✓` is true and useless,
and `⧗` is reserved for work owed, which a hold is not. Note the parallel to
`DependentRebuildOutcome::Rebuilt`, whose doc comment previously claimed the
holds were in the nested build's own receipt: they were, and that receipt is
discarded, which is the whole reason the effect has to carry them itself.

**`compose_environment` must stay side-effect-free, and the dry-run's
non-mutation is a separate, narrower guarantee.** Purity is what makes the
preview safe: no ref moves, no locks, no checkout, no network. It's tested
(`compose_environment_is_pure_and_deterministic`, which snapshots every ref
before and after). But the dry-run's *ref* guarantee comes from
`pin_environment_inputs(.., synchronize: false)`, not from composition —
`synchronize_branches` fetches, fast-forwards local branches, and creates
remote-only ones, so a preview that synchronized would move the user's
branches. The real build passes `true`. Since P4 that choice is made by the
`PlanPurpose::Preview` variant rather than at the call site, so a future planner
cannot get it wrong by forgetting an argument. That asymmetry is deliberate and
documented at the call site: a preview reflects current *local* refs while the
build syncs first, so a stale local branch can make the preview describe older
content. That is ordinary staleness, categorically weaker than the bug P1
removed (two merge engines disagreeing), but it is a real remaining difference
— if a dry-run ever needs to be exactly predictive, the fix is to make sync a
shared, user-visible step, not to re-add a second merge path.

**A pre-check that intercepts makes a flag look like it works when it never
reaches the code that matters.** `hitch rebuild <env> --on-conflict halt` was
handled entirely by a separate pre-check in `commands/rebuild.rs`; the
composition loop itself read `environment.on_conflict` from the config and had
no way to receive the CLI override, so the flag had **no effect on the code
that actually merged**. It worked by accident, through the duplicate. Deleting
the duplicate (correctly — it was a second opinion from a second merge path)
turned the flag into a silent no-op, which is how it was found. Generalised: if
a flag's effect is observable only via a check that runs *instead of* the real
operation, the flag is not wired to the operation. The override is now threaded
explicitly as `rebuild_environment_opts(.., on_conflict_override: Option<OnConflict>)`,
`None` meaning "use the environment's policy" — which is every caller except
that one flag.

The same duplicate was why a halt under `OnConflict::Halt` printed **two
different reports** depending on an unrelated flag: the pre-check emitted
`format_compatibility_report_for_rebuild`, while the composition's in-loop halt
emitted `utils::conflict_report::format_conflict_report`, and only
`--replay-resolutions` (which skipped the pre-check) ever reached the second.
Both paths now halt inside `compose_environment` and render the former, which
is why that formatter moved from `commands/rebuild.rs` into `prelude.rs` — the
decider has to be able to render its own refusal. Consequence:
`format_conflict_report` now has no production caller. It is kept (public,
tested, strictly richer than what replaced it) and documented as a deletion
candidate; nothing should call it to decide a mutation's outcome. P7 was its
last plausible consumer and did not take it: the hold it renders is already in
the plan's Composition section, with the branch, the partner, the file count and
the remedy, so a second, differently-worded rendering of the same fact was a
fourth copy rather than a richer view.

**Composition happens in the object database, and must stay merge-identical.**
`rebuild`/`release` build with `git merge-tree --write-tree -z` plus
`commit-tree`: no worktree, no index, no checkout. One parent gives squash
semantics, two gives the ancestry-preserving merge commit `--no-ff` release
mode needs. Ejecting a conflicting branch is just "don't advance `composed`" —
there is no merge state to abort. Two things to keep in mind:

- The load-bearing assumption is that ORT-via-merge-tree agrees with
  ORT-via-worktree. `test_merge_tree_compose_matches_real_merge_across_scenarios`
  is a *differential* test that runs both and compares — tree OIDs for clean
  merges, exact per-path stage OIDs for conflicts (which is what recorded
  resolutions are keyed on). Extend it, don't replace it, when you touch the
  merge path; rename-vs-modify and delete-vs-modify are where a shortcut shows.
- A `commit-tree` commit is unreachable until the publish CAS lands. Both
  callers anchor it under `refs/hitch/build/*` / `refs/hitch/release/*` for
  that window so a concurrent `git gc --prune=now` cannot collect it, and drop
  the anchor only after publish is attempted. Keep that ordering.

**Git subprocesses inheriting a real terminal's stdin.** `Command::output()`
captures stdout/stderr but leaves stdin at Rust's default, which is
*inherited* from the caller — not null. Any git subprocess spawned this way
can therefore end up blocked reading from the actual terminal (or CI job)
that launched the process, if git or anything it shells out to (GPG/SSH
commit signing, a pager, an editor) wants interactive input for any reason.

This bug existed in **two independent places** and both had to be fixed
before it was actually gone: `src/utils/git_operations.rs` (hitch's own
automation — `run_git_command`, `run_git_command_with_index`) *and*
`tests/test_framework/command_runners.rs` (the test harness's own
`GitCommandRunner::run` and `HitchCommandBuilder::execute`, which spawn git
and the `hitch` binary directly and are a completely separate code path).
Fixing only the first was not enough — a test that shells out to plain git
itself (e.g. `hitch resolve`'s Mode A integration test, which runs
`git rebase --continue` directly to simulate what a user does after hitch
hands off) can still hang via the *second* path.

It surfaced as `cargo test` (and CI) hanging indefinitely on one specific
test on some machines/runners while passing cleanly, repeatedly, on others —
purely because of that environment's git config or platform defaults, not
reproducible by reading the code or by running the same test where it
happened to work. Fixed by explicitly setting `.stdin(Stdio::null())` on
every git/gh subprocess spawned by hitch's own code *and* by the test
framework — hitch's own confirmation prompts always go through the
`Confirm` trait, never raw git/gh, so no automation call should ever be
able to wait on real input. The one deliberate exception is
`hitch resolve --tool` (`git mergetool`), which is supposed to be
interactive. If you add a new `Command::new("git")` or `Command::new("gh")`
call anywhere in `src/` *or* `tests/test_framework/`, for anything other
than a genuinely interactive flow, null the stdin.

This is now machine-enforced, not just documented: `clippy.toml` denies
`std::process::Command::new` crate-wide (`-D warnings` in `just lint` turns
it into a hard build failure), so *any* new spawn point — not only in
`git_operations.rs`/`gh.rs`, but anywhere in `src/` or `tests/` — must carry
an explicit `#[allow(clippy::disallowed_methods)]` plus a one-line reason
(piped stdin needed, deliberately interactive, test harness simulating a
user, ...). Treat the annotation as the review signal it's meant to be: if
you can't articulate the reason in one line, route the call through
`GitOperations::git_command`/`run_git_command` instead of blessing it.

**`update-ref` desynchronizes every checkout that has the branch attached.**
Git deliberately does not touch a checkout's index or working tree when a ref
moves underneath it. So publishing a rebuild/release onto a branch a human is
standing on leaves their `git status` showing the entire diff as uncommitted
*reverse* changes, with nothing explaining why. This bit `hitch release`
(no resync at all) and `hitch rebuild` (resynced only the *main* checkout,
via `get_current_branch()`, which cannot see linked worktrees). Anything that
moves `refs/heads/*` must go through `scan_checkouts_on_branch` /
`resync_checkouts`, built on `GitOperations::list_worktrees()` — never
`get_current_branch()`, which answers for one checkout out of N.

Two non-obvious constraints, both learned by getting them wrong:

- **Scan before the ref moves, resync after.** Cleanliness is only meaningful
  beforehand. Once the ref has moved, `git status` in an affected checkout
  compares an old working tree against the new tip, so *every* affected
  checkout reports dirty and a naive "skip if dirty" guard skips everything —
  which is exactly the bug it was meant to fix, now with a warning attached.
  This is why the scan/resync split exists; don't collapse it.
- **Detached HEAD is not affected.** Its HEAD names a commit, not the branch,
  so nothing moved underneath it. `checkouts_on_branch` filters these out on
  purpose — resyncing them would silently relocate the user's HEAD.

Dirty checkouts are warned about, never reset — `reset --hard` over someone's
uncommitted work is a worse failure than a stale tree. Related: hitch's
deploy-key pushes go to an explicit SSH URL rather than the `origin` remote,
so git does not update `refs/remotes/origin/<branch>` for them;
`record_pushed_tip` in `prelude.rs` does it explicitly, otherwise `git status`
reports a just-pushed branch as ahead of origin until the next fetch. Tests
creating linked worktrees must place them **beside** the repo, not inside it,
or they show up as untracked content in the repo's own `git status` (the same
reason `hitch resolve` puts its own worktree in a sibling directory).

**GitHub deploy key must bypass `hitch-protection` ruleset on push.** `hitch
setup` creates a GitHub repository ruleset that blocks all direct pushes
(`update`, `deletion`, `non_fast_forward`) to environment branches, with
bypass only for deploy keys. `hitch rebuild` and `hitch release` must
therefore push protected branches using the deploy key (`~/.ssh/hitch_*`),
not the user's default git credentials (HTTPS token / personal SSH key) —
otherwise the push fails with `GH013: Repository rule violations`. The
helpers `force_push_with_deploy_key_if_configured` and
`push_branch_with_deploy_key_if_configured` in `prelude.rs` detect whether
`hitch setup` was run and route the push through
`GitOperations::push_with_ssh_identity` /
`force_push_with_ssh_identity` accordingly. Any new command that pushes to
a branch that could be protected by a `hitch-protection` ruleset must use one
of these helpers, never a raw `push_branch` / `force_push_branch` /
`force_push_with_lease` against `origin`.

**Known, deliberately-open trust gap: custom merge drivers.** `merge_tree_compose`
runs ORT, and ORT honours a `merge=<driver>` attribute from an in-tree
`.gitattributes`. The driver's *command* comes from git config (`merge.<driver>.driver`),
not from the tree, so an attacker who can only push commits cannot by itself
choose a program to run — they need the victim's config to already define that
driver. The hardening in `git_command` does not close this (there is no
supported way to disable merge drivers while keeping ORT's real behaviour, and
faking it would break `test_merge_tree_compose_matches_real_merge_across_scenarios`,
which is the load-bearing correctness guarantee). Treat any repository that
configures a merge driver as one where composition executes that program, and
do not add config-based "mitigations" that silently change merge semantics.

**Default output may not name mechanism; `--verbose` may.**
`tests/integration/terminology_tests.rs` drives one scenario through the
common commands and fails on `sha`/`oid`/`ref`/`refs/`/`cas`/`eject`/`journal`/
`fingerprint`/`anchor`/`hitch-metadata`/... as case-insensitive words. New
output extends that scenario; a hit gets past only via its `ALLOWED` table,
each entry with a reason (pasteable `git …` lines are skipped outright).
`hitch-metadata` renders as `settings` in effect rows (`short_ref`). An
approval gate is a `Needs approval` / `⏳` section in `render_plan`, not the
blocking `⛔` heading, though the model still calls it blocking.

**Recovery is tested by interruption, not by inspection.**
`tests/integration/crash_recovery_tests.rs` runs the publish sequence with
`HITCH_TEST_ABORT_AFTER` set to each of four named steps —
`journal-written`, `ref-moved`, `resync-done`, `push-succeeded` —
`std::process::abort`s there, then re-runs the command and asserts the
resulting content equals what an uninterrupted run produces and that no
journal record is left behind. It is a differential test against an oracle
run, in the same spirit as
`test_merge_tree_compose_matches_real_merge_across_scenarios`. If you add a
step to a publish, add an abort point for it — a step with no abort point is
a recovery path with no test. `rebuild` (`crash_recovery_tests.rs`),
`release` (`release_crash_recovery_tests.rs`), and `resolve`'s Mode A publish
(`resolve_crash_recovery_tests.rs`) all go through the shared `publish_branch`
core (see the Conventions entry on it above) and are each exercised at the
three abort points that precede any push — `journal-written`, `ref-moved`,
`resync-done`. Only `rebuild`'s test additionally covers the fourth point,
`push-succeeded`: `release`'s and `resolve`'s crash-fuzz tests rely on the
test harness's default `--no-push`/`--yes` injection, under which
`context.should_push()` is `false` and `publish_branch` never calls the push
closure at all, so those two commands' push path — including their records'
`push_owed` field, which is real and journal-tracked via `publish_branch`,
not dead — remains genuinely untested by crash-fuzz for now. The abort hook
itself (`maybe_abort_for_test` in `publish_journal.rs`) is
`#[cfg(debug_assertions)]`-gated, so these five tests silently no-op (the
process exits successfully instead of aborting) whenever built with
`--release` — the `justfile`'s `release` recipe must run its pre-flight
`cargo test` without `--release` for this reason; this broke once already.
Two things learned writing the first ('rebuild') of these tests:

- The convergence check compares `<branch>^{tree}`, not the branch's commit
  SHA. `commit-tree` stamps the ambient wall-clock time with no
  `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE` override, so two rebuilds of
  identical inputs a second apart legitimately get different commit OIDs —
  comparing SHAs (especially across the separate `HitchTestFramework`
  instances the oracle and each loop iteration use) is not a meaningful
  "did it converge" check and would fail/flake for a reason unrelated to
  recovery. The tree is pure content and timestamp-independent.
- The "next `hitch` invocation" that triggers recovery needs `--force`:
  `with_locked_env` locks the environment before `rebuild_environment_opts`
  ever runs, and `maybe_abort_for_test` always fires deep inside that
  closure, so every abort point also skips the unlock-on-exit and leaves the
  environment persistently locked — a separate mechanism from the publish
  journal (see the locking-discipline entry in Conventions above). Note
  `--force` bypasses `with_locked_env` entirely (`rebuild.rs` calls
  `rebuild_environment_opts` directly rather than through it when
  `args.force`), so a `--force` recovery after a crash leaves the
  environment locked afterward too — clearing it needs a manual
  `hitch unlock`. This is arguably correct given `locked`'s documented role
  as a human-facing signal rather than an automatically-managed one, but it
  is a real interaction nobody had exercised before this test, and is worth
  a second look.

The `push-succeeded` abort point (added in Task 12, past what the original
plan specified) confirmed a real find: a record with `push_owed: true` does
not by itself mean the push never happened — the process can die between
`force_push_with_deploy_key_if_configured` returning `Ok(())` and
`mark_push_done`/`clear` running, and by that point the push has already
landed. The two cases ARE distinguishable: `record_pushed_tip` (documented
above, ~70 lines up) updates `refs/remotes/origin/<branch>` as part of a
successful push, before either of those calls, so `recover()` compares that
ref against the record's `to_sha` before deciding what to do. A match means
the push already landed — log it and drop the record silently, no warning.
A mismatch means the push is genuinely still owed — warn as before, but
leave the record in place (rather than deleting it) so the next mutating
command's `recover()` sees it and warns again, instead of the obligation
being reported once and then permanently forgotten.
