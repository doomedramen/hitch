# Changelog

All notable changes to Hitch are recorded here, in the style of
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased — explainable UX

Every mutation now explains itself as `INTENT → PLAN → APPLY → RESULT`, and every
display of "what is in this environment" reads one model instead of re-deriving
it. See [`docs/architecture/explainable-operations.md`](docs/architecture/explainable-operations.md).

### Added

- `hitch why` — explains why a branch or environment is in its current state,
  in three forms: `why <branch>`, `why <branch> <environment>`, `why <environment>`.
- `hitch log` — what happened to your environments: who promoted, rebuilt,
  released, locked and approved what. Filters `--env`, `--branch`, `-n/--limit`.
  Derived from `hitch-metadata` history; nothing new is stored.
- `--json` on fourteen commands (`rebuild`, `promote`, `demote`, `release`,
  `lock`, `unlock`, `set`, `add`, `remove`, `cleanup`, `approvals approve`,
  `status`, `why`, `log`): one document on stdout, everything else on stderr.
  Schema in [`docs/architecture/json-schema.md`](docs/architecture/json-schema.md).
- `--dry-run` on `add`, `remove`, `set`, `promote`, `demote`, `rebuild` and
  `release`, which print the plan and change nothing; `hitch cleanup` previews
  by default and deletes only with `--apply`.
- Plans and receipts: every mutating command prints what it will change, what it
  will not, and what it did, and refuses an apply whose inputs moved since the
  plan was made.
- `hitch status` is now a feature × environment matrix (included, held, in base,
  needs rebuild, actual unknown, missing). The previous per-environment view is
  `hitch status --environments [NAME]`.
- Build records at `refs/hitch/state/<env>`: what an environment's last build
  actually contained, written atomically with the branch move.
- `Environment.lock_purpose` in `hitch.json`, so Hitch's own lock/unlock around an
  operation is told apart from a person's `hitch lock`.

### Changed

Human output only; exit codes are unchanged.

- Held branches now name their true conflict partner (the base, or the peer they
  collide with) instead of the last branch composed; `resolve` picks Mode A for a
  base conflict behind a clean peer, and a refused `promote` names the right rebase.
- **All mutating commands**: output is a plan, then a receipt (`Applied`,
  `Result`), instead of a step-by-step transcript. The `[1/6] Synchronizing
  branches` style narration is gone by default; `--verbose` still shows
  mechanism. A cause is now printed once.
- **`hitch status`**: "needs a rebuild" is decided by comparing the SHAs a build
  used with the branches' current tips, not by commit dates. A rebased branch, a
  `--no-rebuild` promotion and a removed branch are now all noticed. The
  per-branch "already in base" hint reads the snapshot. The cleanup block no
  longer asks the remote about each branch, so the command is offline end to end.
  An environment with no build record reads "actual unknown" and still exits 0.
- **`hitch rebuild`**: shows the composition (included, held, with the partner and
  files and the `git rebase` to run). `--dry-run` is the same plan without the
  apply and without taking the environment lock. Exit code 2 still means "applied
  with held branches" and is now also what a `--dry-run` that would hold returns.
- **`hitch promote` / `demote`**: a refused promote names the branch it actually
  conflicts with (a peer, not always the base) and the rebase to run, and no longer
  narrates a rollback it did not perform. A promote into an approval-gated
  environment says what confirming will do (file a request). A failed dependent
  rebuild no longer rolls the declaration back: the edit stays, the environment is
  left unbuilt, and the message names `hitch rebuild <env>`.
- **`hitch promote` beside an already-held sibling** is now allowed: only the new
  branch is judged, so a branch already held in an environment no longer blocks an
  unrelated promotion.
- **`hitch release`**: the plan names the tag, the target move, the branches pruned
  and the dependent rebuilds. The "Will not change" list no longer repeats itself
  or lists environments the release rebuilds. A conflict's remedy names a rebase
  onto the target rather than repeating the failing command. A failed dependent
  rebuild leaves the release in place and names `hitch rebuild <env>`.
- **`hitch cleanup`**: only plans deletions `git branch -d` will accept. Unmerged
  branches and branches checked out in any worktree are kept and named, with the
  `git branch -D` for you to run. A delete that fails anyway is reported after the
  receipt, with exit 1.
- **`hitch set`, `add`, `remove`, `lock`, `unlock`**: plan and receipt, with the
  environment's "after" state. `set --base` shows `old → new`.
- **`hitch approvals approve`**: no longer narrates each lookup. A vote below the
  threshold prints one line; one that meets it prints the promotion's plan and
  receipt.
- **`hitch tree`** and **`hitch status`** spell a composition the same way.
- **`hitch conflicts`, `status`, `tree`, `resolve` and the approval snapshot**
  predict conflicts through the same composition a rebuild runs. Where the old
  tree-based check and a real build disagreed, the build wins. In particular, an
  approval request's recorded `merge_conflicts` is now true for a collision between
  two promoted branches, not only with the base.
- **`hitch promote` conflict check no longer synchronises** branches first. A
  branch that exists only on a remote you have never fetched is skipped by the
  check, and may be held later, at the next rebuild. `hitch resolve` and
  `hitch conflicts` still fetch, as before.
- **`hitch release`'s dependent-environment planning** predicts through the same
  offline composition as everything else, so a dependent whose base cannot be
  resolved is now an error rather than silently treated as conflict-free (the old
  check synchronised branches first).
- The "already in base" fact `hitch status` shows is computed once, from the same
  pinned SHAs the verdict uses (the local branch first, then the cached
  `origin/<branch>`), rather than by a live check at display time.
- A refusal that is only "already so" (unlocking an unlocked environment) prints no
  "To proceed" line.

### Fixed

- `hitch status` could say an environment was up to date when a feature had been
  rebased onto older commit dates, when a promotion was staged with `--no-rebuild`,
  or when a branch had been removed from the declaration.
- `hitch rebuild <env> --on-conflict halt` takes effect inside the real build (it
  had worked only by accident, through a separate pre-check), and a halt prints one
  report instead of one of two different reports.
- `hitch rebuild --dry-run --replay-resolutions` no longer reports branches as held
  that the real build composes from a recorded resolution.
- `--json` without `--yes` now exits 1 with the reason on stderr; it used to exit
  0 having done nothing and printed nothing.
- `hitch release` no longer leaves an internal ref behind when a plan is declined or
  refused.
- `hitch approvals approve` composed the environment from the declaration as it was
  before the approval, so the approved branch landed in `hitch.json` and not in the
  environment branch.
- `hitch approvals approve --json` without `--yes` refuses before it records the vote.
- A refused `promote` or `demote` no longer leaves the environment locked or
  commits extra metadata changes.
- `hitch cleanup` read git's `+ ` marker (a branch checked out in a linked worktree)
  as part of the branch name, so it could suggest a command like
  `git branch -D + feat`. Names now come from `for-each-ref`, and a branch checked
  out in any worktree is kept rather than deleted.
- `HITCH_YES=1` (and `true`) is accepted. The documented spelling used to fail at
  argument parsing with `invalid value '1' for '--yes'`; `--yes` is unchanged.

### Removed

- The desktop-only adapters `core::timeline`, `core::details`,
  `core::workspace_index` and `core::workspace`. `crates/hitch-desktop` consumed
  them and **does not compile against the core until it is rebuilt** on
  `ActivityLog`, `RepositoryStateSnapshot`, `MatrixModel` and `WhyExplanation`.
  The CLI never called them.
- The tree-based `preflight_*` oracle and the step-transcript plumbing
  (`StepNarration`, `StepLogger`, `on_step`), `get_commit_timestamp`, and the
  unused `format_conflict_report`.

### Migration notes

- **`hitch.json` gains `lock_purpose`** on environments. It is serde-defaulted, so
  an older `hitch.json` still loads, and an older Hitch reading a newer one ignores
  it. History written before it exists falls back to a 60-second heuristic when
  `hitch log` decides what was Hitch's own lock.
- **`refs/hitch/state/*` is new and must not be pruned.** There is one live record
  per environment, overwritten in place; it is not an archive like `prev/` and
  `backup/`. `hitch cleanup` does not touch it, and nothing you script should.
- **`LegacyUnknown` persists until an environment's first rebuild.** An
  environment last built by an older Hitch has no build record, so `hitch status`
  shows "actual unknown" for it. This is expected and exits 0. `hitch rebuild
  <env>` writes the record. Releases and `hitch resolve` do not write one.
- **Scripts should use `--json`.** The human output has changed and will keep
  changing; it is not an interface. Read `{schema_version, plan, receipt}` for
  mutations and `{schema_version, status|why|log}` for views, and check
  `schema_version`.
- **Exit codes are unchanged.** 0 success, 1 failure, 2 `rebuild` applied with
  held branches (including a `--dry-run` that would hold). The one correction:
  `--json` without `--yes` is now 1, not 0.
- `hitch status` now prints the matrix; use `hitch status --environments` for the
  old per-environment layout.
- Any tooling built on `crates/hitch-desktop`'s use of the removed adapters must
  move to the typed models above.
