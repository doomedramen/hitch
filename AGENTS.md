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
  Every git primitive is a named method (`merge_tree_compose`, `commit_tree`,
  `update_ref_cas`, ...). All of them build their subprocess through
  `git_command`, which forces `LC_ALL=C`/`LANG=C` (several call sites match
  English stderr substrings), `GIT_TERMINAL_PROMPT=0`, `stdin(Stdio::null())`
  (see the gotcha below), and the hardening flags `core.hooksPath=/dev/null`
  and `core.fsmonitor=false` — hitch runs under a deploy key that bypasses
  branch protection, so anything repo-local config can make git execute
  inherits those rights. `run_git_plumbing_command` adds
  `GIT_CONFIG_NOSYSTEM=1` and is for object-database-only calls; network calls
  must not use it, because on macOS the system config is where
  `credential.helper` lives. `clippy.toml` denies `std::process::Command::new`
  outright, so a new spawn point must carry an explicit
  `#[allow(clippy::disallowed_methods)]` — that annotation is the review
  signal. The `git2` dependency is present but effectively dead — only used
  for repo discovery, never for merges.
- `src/utils/gh.rs` — same pattern for the GitHub CLI (`gh`), used by `pr`,
  `doctor`, `setup`, and `pr_status`.
- `src/utils/pending_resync.rs` — crash recovery for the one window
  publishing cannot make atomic (ref moved, checkouts not yet updated).
  Records intent at `refs/hitch/pending-resync/<branch>`; `recover` runs from
  `main.rs` for mutating commands only, so it is always under the repo lock.
  It repairs a checkout only when that tree is *provably* exactly the old tip
  — never on the dead process's say-so — so an edited tree is reported, not
  reset.
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
- **Publishing an environment branch is always a CAS `update-ref`**
  (`publish_environment_build` in `prelude.rs`), never a rename-and-recreate
  dance. Reuse that function for anything that produces a new environment
  branch commit; don't hand-roll the publish step again.
- **Moving a branch ref means resyncing every checkout attached to it.**
  `scan_checkouts_on_branch` before the `update-ref`, `resync_checkouts`
  after — both in `prelude.rs`, both already wired into
  `publish_environment_build` and `hitch release`. See the gotcha below for
  why the scan cannot be folded into the resync. Wrap the pair in a
  `pending_resync::record` / `clear`, so a crash in between is recoverable.
- **Compare checkout paths with `GitOperations::same_checkout_path`, never
  `==`.** `git worktree list` reports fully resolved paths; on macOS the temp
  dir and plenty of real project paths sit behind symlinks, so a string
  comparison silently never matches and whatever it gated is quietly skipped.
  This has already caused one bug in `pending_resync`'s recovery.

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

The composition path (`merge_tree_compose`) sidesteps this entirely by *not*
passing `--merge-base` — git computes it, including the virtual base for
criss-cross histories, exactly as a real merge does. Don't "helpfully" add an
explicit `--merge-base` there. The gotcha applies to the preflight callers
(`merge_tree_write_tree_name_only`) that do pass one.

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
