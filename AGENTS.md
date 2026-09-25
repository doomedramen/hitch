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
independently of this program. P0, P1, and P2 are authored and complete; P3 is
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
  `command_is_mutating` if it's read-only).
- `src/commands/*.rs` — one file per CLI command/subcommand, thin: arg
  parsing (`clap::Args` struct) + orchestration. Business logic belongs in
  `src/utils/prelude.rs` or a dedicated `src/utils/*.rs` module, not here.
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
  free). It replaces the old state layer's inference-from-timestamps. The
  record's `RefEdit` rides `publish_branch`'s existing transaction, so
  "environment tip moved" and "record describing that tip now exists" are
  applied all-or-nothing. Its reader, `read_state`, is the sole input to
  `src/core/state.rs`'s Actual side.
- `src/core/` — read-only view builders (workspace/status/state models).
  - `state.rs` is **the** authority on "does this environment need a
    rebuild, and what actually moved". `build_state_snapshot` returns a
    `RepositoryStateSnapshot` — Desired read live from refs, Actual read
    from the build record, Health classified by comparing the two — and
    every display of that verdict must read it from here rather than
    re-deriving it. `status.rs` is now a *pure projection*:
    `build_status_model(&snapshot)` takes no context, opens no repo, and
    cannot disagree with the snapshot it was handed. `commands/status.rs`
    reads the same snapshot and only formats it.
  - `workspace_index.rs`'s `build_workspace_index_model`/`WorkspaceIndexModel`
    have no CLI command caller — they're consumed by `crates/hitch-desktop`'s
    Tauri backend (`src-tauri/src/main.rs`), not `src/commands/*.rs`.
    `details.rs` and `status.rs` are the other two view builders.
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
  step is a worse experience than the rest of the CLI.
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
  one(s) a new mutating operation actually needs.
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
  `recover` on the next mutating command.
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
verdict: it has exactly one production caller left,
`core/timeline.rs:96`, which formats a date for display.

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
refusal — takes main's normal error path to 1. `--dry-run` uses the same
signal: `Ok(true)` means "would hold", so a dry-run preview exits 2 as well.
Collapsing `AppliedWithHolds` into a plain success would silently break every
consumer relying on that distinction. The `stdout().flush()` immediately
before the `process::exit` is load-bearing too — `process::exit` skips normal
shutdown, so buffered output is lost when stdout isn't a TTY, i.e. in CI.

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

**One composition, two callers — don't add a third.** `compose_environment`
(`src/utils/prelude.rs`) is the *only* place a conflict verdict is reached:
`rebuild_environment_opts` calls it, and `rebuild --dry-run` calls it over the
same `pin_environment_inputs` result. That is the invariant, and it was
expensive to establish: `--dry-run` used to short-circuit into
`preflight_compatibility_report`, a *tree-based* loop over
`merge_tree_write_tree_name_only` with an explicit `--merge-base`, while the
build was a *commit-based* loop over `merge_tree_compose` (ORT, no explicit
merge-base). Two doors into the merge engine, so two verdicts were possible,
and the live symptom was `rebuild <env> --dry-run --replay-resolutions`
reporting branches as **held** that the real build **composed** from the
recording — exit 2, "would hold", for a build that was going to succeed.
`preflight_compatibility_report` is **not yet** a display-only function, and
the remaining mutation that depends on it is `hitch resolve`, not `rebuild`.
`commands/resolve.rs:131` and `:180` use it to decide Mode A (rebase the branch
onto the base) versus Mode B (peer conflict), and to refuse outright when it
reports no conflict at all — so a preflight/composition disagreement would
there pick the wrong resolution mode, not merely print a stale preview. The
read-only callers are `conflicts.rs:44` and, via the offline
`preflight_compatibility_report_local`, `status.rs:238` and `tree.rs:138`; those
are legitimate, since a display preflight is allowed to approximate. Routing
`resolve`'s mode selection through the shared primitive is **not yet done** —
P1 scoped itself to `rebuild`'s dry-run, and re-plumbing `resolve` belongs with
P4's planner. Do not add further dependants in the meantime.
Two tests hold the `rebuild` half: `test_dry_run_agrees_with_real_build_about_replayed_resolutions`
(resolve_tests) and `test_dry_run_and_real_build_agree_on_held_branches`
(rebuild_tests). Both compare the *verdict*, not the rendered prose, on purpose
— the two paths legitimately word the same event differently.

**`compose_environment` must stay side-effect-free, and the dry-run's
non-mutation is a separate, narrower guarantee.** Purity is what makes the
preview safe: no ref moves, no locks, no checkout, no network. It's tested
(`compose_environment_is_pure_and_deterministic`, which snapshots every ref
before and after). But the dry-run's *ref* guarantee comes from
`pin_environment_inputs(.., synchronize: false)`, not from composition —
`synchronize_branches` fetches, fast-forwards local branches, and creates
remote-only ones, so a preview that synchronized would move the user's
branches. The real build passes `true`. That asymmetry is deliberate and
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
tested, strictly richer than what replaced it, and a plausible input for P7's
display paths) and documented as a deletion candidate; nothing should call it
to decide a mutation's outcome.

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
