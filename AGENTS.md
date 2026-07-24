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
active design doc for the conflict-handling system (worktree-isolated
rebuilds, eject-and-continue policy, `hitch resolve`) — check its
"Implementation status" section before assuming a phase is done or before
starting related work.

## Build, test, lint

Always use `just` recipes (see `justfile`) — they're what CI and pre-commit
hooks run, so using anything else risks passing locally and failing there.

```bash
just build              # cargo build
just format             # cargo fmt
just format-check       # cargo fmt --check  (CI gate)
just lint                # cargo clippy -p hitch --all-targets -- -D warnings  (CI gate)
just test               # cargo test -p hitch  (full suite)
just test-file <name>   # cargo test --test <name>
```

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
  functions. Large file; read the doc comment on the specific function you
  need rather than the whole file.
- `src/utils/git_operations.rs` — the *only* place that shells out to `git`.
  Every git primitive is a named method (`squash_merge`, `add_worktree`,
  `update_ref_cas`, ...), all via `run_git_command` (which forces
  `LC_ALL=C`/`LANG=C` since several call sites match English stderr
  substrings, and `stdin(Stdio::null())` since `Command::output()` otherwise
  leaves stdin inherited from the caller — see the gotcha below — never
  bypass either by calling `git` directly from a command module). The `git2`
  dependency is present but effectively dead — only used for repo discovery,
  never for merges — real work always shells out to the `git` binary. Don't
  introduce new git2 usage without a specific reason.
- `src/utils/gh.rs` — same pattern for the GitHub CLI (`gh`), used by `pr`,
  `doctor`, `setup`, and `pr_status`.
- `src/utils/resolutions.rs` — phase-5 shared conflict resolutions:
  content-addressed by exact merge-stage blob OIDs (NOT git-rerere — see the
  module header for why that matters), stored as `refs/hitch/resolutions/*`.
  Consumed by `hitch resolve --record`, `hitch rebuild --replay-resolutions`,
  `hitch resolutions`, and `hitch doctor`'s debt SLA.
- `src/core/` — read-only view builders (workspace/status models) consumed
  by commands; `workspace.rs`'s `build_workspace_model` is currently
  `#[allow(dead_code)]` — not wired into any live command, don't assume it's
  in use.
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

## Known dead/aspirational code — don't build on these without reviving them

- `src/utils/git_error.rs` — a typed `GitError` enum, explicitly documented
  in its own header as not integrated; all live error handling uses
  `anyhow::anyhow!` with formatted strings.
- `src/utils/hooks.rs` — a lifecycle-hook/plugin system
  (`#![allow(dead_code)]`), zero call sites outside the file.
- `src/core/workspace.rs`'s `build_workspace_model` / `WorkspaceModel`.

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
- **The user's working tree is sacred**: `hitch rebuild` and `hitch resolve`
  compose in disposable `git worktree`s, never in the user's own checkout —
  this was a deliberate phase-1 redesign (see the plan doc) specifically to
  eliminate a whole class of "left the user on the wrong branch" or "left a
  dirty tree" bugs. Any new command that needs to build/merge things should
  follow the same pattern (`GitOperations::new_at_path` on a worktree), not
  `checkout`/build/`checkout back` in the real repo.
- **Publishing an environment branch is always a CAS `update-ref`**
  (`publish_environment_build` in `prelude.rs`), never a rename-and-recreate
  dance. Reuse that function for anything that produces a new environment
  branch commit; don't hand-roll the publish step again.

## Concrete gotchas, found the hard way

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

**Git subprocesses inheriting a real terminal's stdin.** `Command::output()`
captures stdout/stderr but leaves stdin at Rust's default, which is
*inherited* from the caller — not null. Any git subprocess hitch spawns can
therefore end up blocked reading from the actual terminal the test suite (or
hitch itself) was launched from, if git or anything it shells out to (GPG/SSH
commit signing, a pager, an editor) wants interactive input for any reason.
This surfaced as `cargo test` appearing to hang forever on a specific test
(`git rebase` inside `hitch resolve`'s Mode A) on one machine but not
another, purely because of that machine's global git config — not
reproducible by reading the code or running the same test elsewhere. Fixed by
explicitly setting `.stdin(Stdio::null())` on every git/gh subprocess hitch's
own automation spawns (`run_git_command`, `run_git_command_with_index`,
`gh_api`, `owner_repo_from_remote`) — hitch's own confirmation prompts always
go through the `Confirm` trait, never raw git/gh, so no automation call
should ever be able to wait on real input. The one deliberate exception is
`hitch resolve --tool` (`git mergetool`), which is supposed to be
interactive. If you add a new `Command::new("git")` or `Command::new("gh")`
call for anything other than a genuinely interactive flow, null the stdin.
