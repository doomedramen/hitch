# Explainable operations

How Hitch decides what an environment should contain, remembers what it
actually built, and describes both before and after it acts. This is the
newcomer's tour; `AGENTS.md` is the detailed, gotcha-by-gotcha version and
`docs/explainable-ux-spec.md` is the original specification.

The organising rule: **every mutation is `INTENT → PLAN → APPLY → RESULT`, and
every word a user reads about it comes from one place.**

## 1. Desired, Actual, Proposed

Three words, three different questions. Mixing them up is the commonest way to
write a wrong sentence about an environment.

| | Question it answers | Where it lives |
| --- | --- | --- |
| **Desired** | What does the declaration say this environment should contain? | `hitch.json` on the `hitch-metadata` branch, read live; carried as `RepositoryStateSnapshot` in `src/core/state.rs` |
| **Actual** | What did the last build really contain? | The *build record*, `refs/hitch/state/<env>`, written by `src/utils/build_record.rs`; read by `read_state` and carried in the same snapshot |
| **Proposed** | What would exist if this operation were applied? | The `proposed` projection on an `OperationPlan` (`src/operations/model.rs`), computed by the operation's planner |

```
            declaration (hitch.json)            build record (refs/hitch/state/<env>)
                     │                                       │
                     ▼                                       ▼
                  DESIRED ──────── compared by ───────────► ACTUAL
                     │          core::state (health)           │
                     │                                         │
                     └──── an operation edits / rebuilds ──────┘
                                      │
                                      ▼
                                  PROPOSED   (plan.current → plan.proposed)
```

Desired and Actual are both *facts about the repository*, read by
`build_state_snapshot` and compared by it; nothing else is allowed to decide
whether an environment is up to date. Proposed is a *prediction* made by a
planner, and it is never stored as if it were a fact: a receipt reports what
happened, and the next snapshot reports where things stand.

`build_state_snapshot` is offline on purpose. It resolves each declared branch
from `refs/heads/<b>`, falling back to the cached `refs/remotes/origin/<b>`, and
never fetches. A branch that exists only on a remote you have not fetched
therefore reads as missing, which is correct: Hitch could not have built from it
either.

The staleness verdict is a **SHA comparison** between the record's pinned inputs
and freshly pinned ones. It is never a timestamp comparison: a rebased branch
can have older commit dates and entirely different content.

## 2. Environment health

`EnvironmentHealth` (`src/core/state.rs`) is the verdict, and every display
reads it rather than re-deriving it.

| State | Meaning | Counts as "needs a rebuild"? |
| --- | --- | --- |
| `Realised` | The build is current and included everything declared | no |
| `PartiallyRealised` | The build is current but deliberately held some branches out (a conflict) | no |
| `NeedsRebuild` | An input moved, or the declaration changed (`added` / `removed`), since the build | yes |
| `NeverBuilt` | No record and no `rebuilt_at` stamp | yes |
| `MissingBranch` | The environment branch does not exist locally | yes |
| `LegacyUnknown` | A build exists, but there is no usable record of what it contained | no |

**`LegacyUnknown` is a normal state, not a defect.** A repository last built by
a Hitch from before build records existed has it for every environment until
each one's first rebuild. It also arises for builds Hitch deliberately does not
write a record for (a release's own landing, `hitch resolve`), because Hitch has
no truthful input to put in one. `hitch status` says "actual unknown", exits 0,
and offers `hitch rebuild <env>` as the thing that would make it known. The rule
that makes this safe: a reader that finds no record says *unknown*; it never
substitutes a guess.

```
$ hitch status
Feature   DEV               QA
─────────────────────────────────────────
clash     ? actual unknown  ⛔ held
payments  ? actual unknown  — not desired
search    ? actual unknown  — not desired

DEV  desired 3 · actual 0 · 3 actual unknown
    actual unknown
QA   desired 1 · actual 0 · 1 held
    partially realised
```

The matrix cell vocabulary (`MatrixCell` in `src/core/status.rs`) is the
per-branch counterpart: not desired, included, held, in base, needs rebuild,
actual unknown, missing. `hitch why` (`src/core/why.rs`) explains one cell or one
environment from the same snapshot.

## 3. Plan, validate, apply, receipt

Every mutating command is the same four steps:

```
 intent ──► plan_*  ──► gate ──► validate_* ──► apply_* ──► receipt
            (reads,      (show     (is the plan   (lands      (what actually
             predicts,   the plan,  still true?)   the plan,    happened)
             writes      confirm)                  never
             nothing)                              re-plans)
```

* **Plan** (`OperationPlan<I>`, `src/operations/model.rs`): the intent, the
  fingerprint, current and proposed projections, effects, warnings, whether
  confirmation is required, and an operation-specific `detail`. Planning writes
  nothing durable. `--dry-run` is *only* the plan, rendered; it takes no
  environment lock.
* **Gate** (`confirm_plan` and `decide_gate`): prints the plan and asks, unless
  `--yes`. Under `--json` without `--yes` it refuses (exit 1), because a program
  must not block on a terminal.
* **Validate**: the fingerprint is recomputed from the repository; if it differs,
  the apply refuses with a typed `StalePlan` error that names what moved.
* **Apply**: lands the decision the plan already made. A plan is a decision, not
  a recipe — the composed commit is carried in the plan and published as is,
  because recomposing a second later would produce a different commit id.
* **Receipt** (`ExecutionReceipt`): what the apply did, the outcome
  (`Applied`, `AppliedWithHolds`, `ApprovalRequested`, ...), warnings the apply
  learned, and the resulting state.

One planner per *operation*, not per command: promote and demote are the same
edit in opposite directions and share `plan_declaration_change`.

### The fingerprint, and what makes a plan stale

`PlanFingerprint { metadata_sha, refs, remote_refs, resolution_keys }` is a
**whitelist of what this plan depended on**, not a snapshot of every ref. It
names the environment's base, its promoted branches, its own ref, the
remote-tracking refs it read, and the keys of any recorded resolutions it
replayed. It also carries the tip of `hitch-metadata`, the declaration's own
branch.

Consequences worth knowing:

* An unrelated branch appearing, moving or disappearing does **not** stale a plan
  — it was never read. If every ref counted, a busy repository could never apply
  anything it had planned.
* A branch the plan *did* read moving, or the declaration moving, does stale it.
  So does a recorded resolution that has disappeared: a replay that now misses
  would hold the branch instead of composing it, which is a different operation.
* The digest is a git object hash over a hand-rolled canonical encoding, so the
  same inputs always give the same digest.
* Because the declaration is part of the fingerprint, a plan must be built
  *after* every metadata write that precedes it, including the lock Hitch itself
  commits before running a command's closure. That is why promote, demote and
  release plan inside `with_locked_env`.

### Locks, publishing and recovery

Mutating commands take a repository-wide file lock, rebuilds take a per-environment
lock, and the persisted `locked` flag is the human-facing "do not touch" signal;
`lock_purpose` records whether Hitch or a person set it. A branch ref moves only
through `publish_branch`, a single compare-and-swap ref transaction that also
writes the build record and a journal of what is still owed (resync of any
checkout standing on the branch, push to origin). `recover` repairs or reports an
interrupted publish on the next mutating command. See `AGENTS.md` for the
details; they are the part most worth reading before touching publish code.

## 4. Composition is the one oracle

"Would these branches combine?" has exactly one answer per *kind* of
composition, and nothing else may answer it.

* **Environment builds and predictions** — `compose_environment`
  (`src/utils/prelude.rs`) builds an environment from a base plus an ordered
  list of branches using `git merge-tree --write-tree` and `commit-tree`. No
  worktree, no index, nothing checked out. A branch that conflicts is **held**
  (left out, the build continues) or, under the `halt` policy, the whole build
  refuses. `hitch rebuild --dry-run` reaches it through `plan_rebuild` in its
  `Preview` purpose. `predict_composition` runs the same composition offline and unlocked,
  ejecting on conflict and never replaying; `hitch status`, `hitch tree`,
  `hitch conflicts`, the approval snapshot, promote's pre-check, release's
  dependent planning (against the current, pre-release target) and `hitch resolve`'s mode choice all go through it.
  There is no second, tree-based oracle: when there were two, a preview could
  say "would hold" about a build that went on to succeed.
* **Releases** — `compose_release` (`src/operations/release.rs`). A release
  merges a chain of promoted branches into a target that already has content, and
  it is all-or-nothing: a conflict writes nothing. That is the opposite of a
  build's eject-and-continue, which is why it is a second composition rather than
  a third caller of the first. The rule is *one composition per kind, and no kind
  reached by two doors*.

Composition must stay side-effect free and merge-identical to a real merge; a
differential test (`test_merge_tree_compose_matches_real_merge_across_scenarios`)
holds that line.

## 5. The receipt's warning contract

A plan and a receipt describe one operation, and each part of them has one job:

| Document | Says |
| --- | --- |
| Plan | what is *about* to happen, and the advisories that follow from the plan |
| Receipt `effects` | what happened |
| Receipt `resulting_state` | where things stand now |
| Receipt `warnings` | only what the *apply* learned and the plan could not have known |

A receipt warning is therefore an effect still **owed** (a push that failed, a
nested rebuild that did not run), carried as `ExecutionWarning { owes_effect:
true }` and rendered with `⧗`. Copying a plan's advisory into the receipt is a
prediction in the wrong tense, and prints one fact twice. A hold is not a
warning in the receipt: it is already in `resulting_state` as
`PartiallyRealised { held }`.

The same discipline gives the exit codes: `OperationOutcome::AppliedWithHolds`
is what `hitch rebuild` turns into **exit 2** (`hitch rebuild --dry-run` uses it
for "would hold"). Exit 0 is success, exit 1 any failure, including a refused
plan; the only non-0/1 code Hitch chooses is that 2. (Command-line usage errors
from the parser also exit 2.)

All words come from `src/core/render.rs`: `render_plan`, `render_receipt`,
`render_matrix`, `render_why`, `render_activity`. They are pure functions of a
model value; none opens a repository or reads a clock, so a display cannot
disagree with what it displays. Anything between the plan and the receipt prints
nothing by default; mechanism goes to `--verbose`.

## 6. Activity events

`hitch log` answers "what happened to my environments". `src/core/activity.rs`
has a pure model — `derive_events` diffs two consecutive `hitch.json` documents
into typed `HitchEvent`s (`Promoted`, `Demoted`, `Rebuilt`, `Released`, `Locked`,
`Unlocked`, `EnvironmentCreated`, `EnvironmentRemoved`, `BaseChanged`, and the
`Approval*` family) — plus one reader, `build_activity`, which walks the first-parent
history of `hitch-metadata`.

This is **Phase A: nothing is persisted.** There is no event log file; the events
are derived on every call from history that already exists. Details that matter:

* Hitch's own lock/unlock bracket around an operation is collapsed out, using
  `lock_purpose`; only a manual `hitch lock` or `hitch unlock` shows. History
  older than `lock_purpose` falls back to a 60-second heuristic.
* A `Rebuilt` event carries what the build contained only when a build record can
  be *proved* to belong to it (ancestry plus no intervening rebuild stamp);
  otherwise it is `Unrecorded`, never a guess.
* A release's target is not recorded anywhere, so the log cannot name it.
* An operation lock left behind by a crash is invisible to the log, as is the
  manual unlock that clears it. Fixing that needs a marker written at unlock time.

## 7. Compatibility surfaces kept on purpose

Four things look removable and are not. Each has a live caller, and each has a
condition for removal.

| Surface | Why it stays | Remove when |
| --- | --- | --- |
| `refs/hitch/prev/*` alongside `refs/hitch/backup/*` | Byte-identical today, but removing one changes a user-visible ref family | A release note announces which family is dropped and tooling that reads either has had a release to move |
| Reader for legacy `refs/hitch/pending-resync/*` | Repairs an upgrade taken mid-publish | No supported upgrade path starts from a version that wrote it |
| `rebuilt_at` / `released_at` in `hitch.json` | `core/state.rs` uses `rebuilt_at` to tell `LegacyUnknown` from `NeverBuilt`; `hitch log` reads both | Every environment is known to have a build record, and `log` takes its dates elsewhere |
| `LEGACY_OPERATION_LOCK_WINDOW` (60 s heuristic) | History written before `lock_purpose` exists in every older repository | No un-purposed lock commit is within a default `hitch log` limit |

The desktop app's old adapters (`core::timeline`, `core::details`,
`core::workspace_index`, `core::workspace`) are **not** on this list: they were
deleted; `crates/hitch-desktop` builds its own views over `ActivityLog` and
`RepositoryStateSnapshot` (`src-tauri/src/views.rs`).

## 8. Adding a new operation

Follow the shape of an existing one; the machinery is shared and the ordering is
easy to get subtly wrong.

1. **Model.** In `src/operations/`, add a `*PlanDetail` for what is specific to
   your operation, an `OperationKind` variant (and append it to
   `OPERATION_KINDS`, which a test walks), and an `OperationIntent` variant
   (a headline test walks that list too).
2. **Planner.** Write `plan_<op>`. It reads, predicts through the composition
   oracle if composition is involved, and writes nothing durable. Build the
   fingerprint from exactly what you read. If you anchor a commit so garbage
   collection cannot take it before publication, you owe its release on every exit
   path, including errors.
3. **Validator and executor.** `validate_<op>_plan` recomputes the fingerprint
   and returns `StalePlan` on a mismatch; `apply_<op>_plan` lands the plan's
   decision without re-planning. Reuse the shared fingerprint, validation and
   receipt assembly; if your operation composes an environment, call
   `rebuild_environment_gated`, which already gets the plan, gate, apply and
   anchor-discard ordering right.
4. **Command.** A thin file in `src/commands/`: pre-checks, `with_auto_stash`,
   `with_locked_env`, plan, `confirm_plan`, apply, `emit_receipt` (or
   `emit_plan` alone for `--dry-run`). Register it in `src/commands/mod.rs`,
   `src/cli.rs` and both matches in `src/main.rs`. A command with no plan has no
   place in the mutating set.
5. **Words.** Add rendering to `src/core/render.rs`, and nowhere else. Default
   output may not name mechanism (SHAs, refs, "journal", "fingerprint"): extend
   the scenario in `tests/integration/terminology_tests.rs`.
6. **JSON.** If the command honours `--json`, name it in the `--json` doc comment
   in `src/cli.rs` and in `docs/architecture/json-schema.md`; a test compares
   both lists to the commands that actually emit a document.
7. **Tests.** A plan test, a stale-plan test, a refusal test that asserts the
   repository did not move, and, if it publishes, an abort point and a crash
   recovery test.
