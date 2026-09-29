# P9 — Structured activity and terminology: `hitch log` tells the deployment story

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** hitch can tell its own deployment story. It reads `hitch-metadata` history as typed `HitchEvent`s, renders them through `src/core/render.rs`, and shows them in a new read-only `hitch log`. Normal output across the CLI explains behaviour; `--verbose` explains mechanism.

**Architecture:** Three layers, and the same shape `core/status.rs` / `core/why.rs` already follow:
1. **A pure event model.** `src/core/activity.rs` (new) defines `HitchEvent`, `ActivityEntry` and `ActivityLog`. It also defines `derive_events(old: &HitchConfig, new: &HitchConfig) -> Vec<HitchEvent>`, a pure diff with deterministic order that opens no repository.
2. **One impure reader.** `build_activity(context, &ActivityQuery) -> Result<ActivityLog>` walks `hitch-metadata`'s first-parent history, diffs consecutive configs through `derive_events`, and does three things on top:
   - collapses the lock/unlock pair that `with_locked_env` wraps around every mutation;
   - attaches the current build record's rebuild outcome to the one rebuild event it can prove belongs to that record;
   - reports unreadable historical configs as data instead of failing.
3. **Pure renderers in `render.rs`.** `render_activity` renders the log and `render_event` renders one event as one sentence.

`hitch log` is a thin command over those. The existing `core/timeline.rs` survives as a **compatibility adapter** for the frozen desktop crate: `TimelineItem.summary` becomes `render_event(&event)` instead of a hand-formatted string. Removing the adapter is P10's job.

**Tech Stack:** Rust, `serde`, `serde_json`, `clap`, `anyhow`, `chrono` (already a dependency). **No new dependencies.**

**Spec:** `docs/explainable-ux-spec.md` — §17 (human terminology), §18 (semantic vs diagnostic output), §19 (activity/history model), §19.1 (structured events), §19.2 (event persistence: **Phase A only**), Milestone 8, §36.5 (no stringly typed state), §36.7 (deterministic ordering), §37 (UX copy). The master plan is `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`. Read its **Global Constraints** first; every one of them applies here.

---

## Global Constraints

1. **`src/core/render.rs` is the only place that chooses words.** Every
   sentence `hitch log` prints, every one-line event summary the desktop
   adapter carries, and every day heading are written there. `core/activity.rs`
   holds data. A `format!` that builds user-facing prose anywhere else in this
   phase is a defect. Test by inspection: `grep -n 'format!' src/core/activity.rs`
   must show only non-prose uses, such as ref names and `git` arguments.

2. **Do not store prose as the domain model (§19.1).** `HitchEvent` variants
   carry typed fields: environment names, branch names, `ApprovalStatus`, and
   `HoldPair`s reused from `operations::model`. They never carry a summary
   string. The one string-typed escape hatch in the current code,
   `TimelineItem.summary`, is *derived* from an event and is never read back
   into one.

3. **No persistence (§19.2 Phase B is out of scope).** P9 writes no new ref,
   adds no new blob, and does not grow `hitch.json` except for one field whose
   purpose is to record intent at the moment of writing (Constraint 4). Every
   event is *derived* from `hitch-metadata` history plus the current build
   record at `refs/hitch/state/<env>`.

4. **An operation's own lock is not an event, and history must be able to tell
   it apart from a human's.** `with_locked_env` (`src/utils/prelude.rs:482-521`)
   commits `locked: true` before every mutation and `locked: false` after, using
   the same three fields (`locked`, `locked_by`, `locked_at` —
   `src/types.rs:31-38`) that `hitch lock` writes. So every promote today
   produces "Locked … / Promoted … / Unlocked …" in the timeline, and every
   *refusal* produces a bare lock/unlock pair with nothing between. The fix
   records intent at write time: `Environment.lock_purpose:
   Option<LockPurpose>`, with `#[serde(default)]` and
   `#[serde(skip_serializing_if = "Option::is_none")]`. `with_locked_env`
   writes `Operation` and `hitch lock` writes `Manual`. History written before
   this field existed is read with the bracket heuristic in Task 3, and that
   heuristic is documented as legacy-only.

5. **Only attach a rebuild outcome to a rebuild event when the attachment can
   be proven.** A build record is overwritten in place (`AGENTS.md`:
   "`refs/hitch/state/*` is a live pointer"), so at most **one** rebuild event
   per environment can be matched to it. The attachment rule is in Task 4, and
   it rests on the fact `build_record.rs:80-100` documents: the record's
   `metadata_sha` is always a strict ancestor of the `rebuilt_at` stamp commit
   of the rebuild that wrote it. Every other rebuild event says
   `outcome: Unrecorded`. It never says "clean" by default, which is
   the same `LegacyUnknown` discipline `core/state.rs` follows.

6. **Filters are typed, never substring matches.** Today's `matches_filter`
   (`src/core/timeline.rs:127-133`) does `summary.contains(env)`. As a result
   `--env dev` matches `Promoted 'feature/devtools' → qa`, and a branch called
   `a` matches almost every line. `HitchEvent::environment()` and
   `HitchEvent::branches()` answer the filter questions from the fields.

7. **Output order is deterministic (§36.7).** Within one metadata commit,
   events come out in environment *name* order. Within an environment the order
   is: created, base changed, promotions in the new declaration's order,
   demotions in the old declaration's order, lock, rebuild, release. Removed
   environments come next, in name order, then approval events in request-`id`
   order. `HitchConfig::environments` is a `HashMap`, which is exactly why today's
   derivation reorders between runs.

8. **`crates/hitch-desktop` stays untouched, and so do the public names it
   uses.** The desktop crate reads `hitch::core::timeline::{TimelineItem,
   TimelineKind}` and `hitch::core::details::{build_branch_details_model,
   build_environment_details_model, BranchDetailsModel,
   EnvironmentDetailsModel}` (`crates/hitch-desktop/src-tauri/src/types.rs:122-180`,
   `main.rs:70-95`). Those names, their fields, and their signatures do not
   change. Adding a field to `TimelineItem` is allowed; removing or renaming one
   is not. `git diff --name-only main..HEAD -- crates/` stays empty.

9. **`hitch log` is read-only and lock-free.** It goes in
   `command_is_mutating`'s `false` arm (`src/main.rs:173-193`) for the same
   reason `why` does: an explanatory tool that blocks on an in-flight mutation
   is unavailable at exactly the moment it is most wanted. It never fetches.
   It reads local `hitch-metadata` only, the same offline guarantee
   `build_state_snapshot` makes.

10. **`--json` for `hitch log` is a one-half read-only envelope:
    `{"schema_version": 1, "log": ActivityLog}`.** Every enum in it is
    `snake_case`. Timestamps are RFC 3339 UTC; local-time bucketing is a
    *rendering* concern and must not leak into the document. The `--json` doc
    comment in `src/cli.rs:38-55` gains `log`, making **fourteen** commands and
    "three read-only ones". `tests/integration/json_support_tests.rs` then checks
    the list both ways, and its `the_two_read_only_json_commands_are_status_and_why`
    test is renamed and updated.

11. **Default language explains meaning; verbose language explains mechanism
    (§17).** Normal output from any command may not contain `SHA`, `OID`,
    `ref`/`refs/`, `CAS`, `eject`, `materialis/z`, `merge-tree`, `update-ref`,
    `commit-tree`, `force-with-lease`, `journal`, `fingerprint`, `anchor`, or
    `hitch-metadata`. It may show a *short commit id* where one helps, labelled
    as a commit and never as a SHA. Three exceptions, each recorded in Task 7:
    - errors that report an internal invariant violation, which a user
      forwards verbatim in a bug report;
    - a remedy the user must paste, which may name a real git command;
    - `hitch resolve`'s printed worktree path.

    This is enforced by a test (Task 7), not by review.

12. **Exit codes do not change.** `hitch log` exits 0, including on an empty or
    partly unreadable history, and 1 on an `Err` (no `hitch-metadata` at all,
    or a filter naming an environment that does not exist *and* never existed
    in the scanned history).

13. **`main` stays at `5d81fb2`.** All work is on `explainable-ux`.

---

## Deviations from the spec

1. **The event list has more variants than §19.1 names, and every addition
   already appears in today's timeline.** The spec lists eleven variants. The
   current `diff_configs_to_events` also emits environment removal,
   environment base changes, and approval votes/applied/cancelled. The master
   plan's "preserve the current derivation" rules out dropping them. The full
   set is in Task 1, and every current `format!` in `timeline.rs:135-360` maps
   to exactly one variant.

2. **Feature-branch pushes are not events.** §19's target shows "Sarah updated
   dashboard / DEV now needs rebuilding". A push to a feature branch leaves no
   trace in `hitch-metadata`, and reconstructing it would mean walking every
   promoted branch's reflog or commits, a second history source with its own
   failure modes. The combined timeline the desktop reads already interleaves
   branch commits (`TimelineKind::GitCommit`), and that adapter keeps doing so.
   `hitch log` shows hitch events only. The "needs rebuilding" consequence is
   what `hitch status` is for, and `hitch log` points there when the newest
   entry for an environment is a declaration change with no rebuild after it
   (Task 5).

3. **The spec's `ApprovalGranted` is two variants:**
   - `ApprovalVoted { approvals, required }` is a person approving.
   - `ApprovalGranted` is the request reaching its threshold (status →
     `Approved`).

   These are different moments in the story, and today's derivation already
   emits both ("Approval +1 for …" and "Approval approved: …").

4. **`hitch log`, not `hitch activity` or a `status` section.** The user chose
   the name. It sits beside `git log` deliberately: `git log` shows commits and
   `hitch log` shows deployment events, and the command's help text says so in
   its first line.

5. **`lock_purpose` is a new `hitch.json` field.** The spec (§19.2) forbids
   putting *volatile derived state* into `hitch.json`. This field is neither
   volatile nor derived: it is the reason a lock exists, written in the same
   commit as the lock and cleared with it. Without it, history cannot tell an
   operation apart from a human (Constraint 4). Older hitch versions ignore
   the unknown field: serde's default is to ignore unknown fields, and
   `Environment` has no `deny_unknown_fields`, which Task 3 verifies.

---

## Task 1 — The event model: `src/core/activity.rs`, pure

**Files:**
- Create: `src/core/activity.rs`
- Modify: `src/core/mod.rs` (add `pub mod activity;`)
- Test: unit tests in `src/core/activity.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::types::{HitchConfig, Environment, ApprovalRequest, ApprovalStatus, Operation}`.
  `ApprovalRequest.operation` is `crate::types::Operation` (`src/types.rs:204-208`),
  which serializes **PascalCase** (`"Promote"`) because it is persisted in
  `hitch.json`. Do **not** add a serde rename to it: that would change the
  on-disk format. The event carries its own `ApprovalDirection`, below, with a
  `From<Operation>`.
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitchEvent {
    EnvironmentCreated { environment: String, base: String },
    EnvironmentRemoved { environment: String },
    BaseChanged { environment: String, from: String, to: String },
    Promoted { environment: String, branch: String },
    Demoted { environment: String, branch: String },
    Locked { environment: String, by: Option<String> },
    Unlocked { environment: String },
    Rebuilt { environment: String, outcome: RebuildOutcome },
    Released { environment: String },
    ApprovalRequested { request_id: String, environment: String, branch: String, direction: ApprovalDirection },
    ApprovalVoted { request_id: String, environment: String, branch: String, approvals: usize, required: usize },
    ApprovalGranted { request_id: String, environment: String, branch: String },
    ApprovalRejected { request_id: String, environment: String, branch: String },
    ApprovalApplied { request_id: String, environment: String, branch: String },
    ApprovalCancelled { request_id: String, environment: String, branch: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDirection { Promote, Demote }
impl From<crate::types::Operation> for ApprovalDirection { /* total match, no wildcard */ }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    /// No build record can be proven to describe this rebuild. Never read as "clean".
    Unrecorded,
    Clean { included: Vec<String> },
    WithHolds { included: Vec<String>, held: Vec<crate::operations::model::HoldPair> },
}

impl HitchEvent {
    pub fn environment(&self) -> &str;          // every variant has exactly one
    pub fn branches(&self) -> Vec<&str>;        // empty for env-level events
}

pub fn derive_events(old: &HitchConfig, new: &HitchConfig) -> Vec<HitchEvent>;
```

The spec's `RebuiltWithHolds` is `Rebuilt { outcome: WithHolds { .. } }`, one
variant with a typed outcome. A separate variant would make "a rebuild whose
outcome we do not know" and "a clean rebuild" two different *kinds*, and they
are the same kind of event with different knowledge. Record this in `## As
executed`, as it refines deviation 1. `HoldPair` is the type
`AppliedEffect::DependentEnvironmentRebuild` already carries. Before using it,
check that it derives `Serialize`, `Clone`, `PartialEq` and `Eq`; if not, add the
derives rather than defining a second pair type.

Approval events carry `environment` and `branch` copied from the request, so
`environment()` and `branches()` stay total with no config lookup.

- [x] **Write the failing tests first.** Build `HitchConfig` values in
  memory; this is a pure function with no git. One test per current
  `timeline.rs` `format!` site, each asserting the typed event:

```rust
#[test]
fn a_new_environment_is_created_with_its_base() {
    let old = config(&[]);
    let new = config(&[("dev", "main", &[])]);
    assert_eq!(
        derive_events(&old, &new),
        vec![HitchEvent::EnvironmentCreated { environment: "dev".into(), base: "main".into() }]
    );
}

#[test]
fn promotions_come_out_in_declaration_order_and_demotions_in_old_order() {
    let old = config(&[("dev", "main", &["a", "b", "c"])]);
    let new = config(&[("dev", "main", &["c", "z", "y"])]);
    assert_eq!(
        derive_events(&old, &new),
        vec![
            HitchEvent::Promoted { environment: "dev".into(), branch: "z".into() },
            HitchEvent::Promoted { environment: "dev".into(), branch: "y".into() },
            HitchEvent::Demoted { environment: "dev".into(), branch: "a".into() },
            HitchEvent::Demoted { environment: "dev".into(), branch: "b".into() },
        ]
    );
}

#[test]
fn environments_are_diffed_in_name_order_not_map_order() {
    // Twenty environments, each gaining one branch. Run derive_events 50 times;
    // every run must equal the first and be sorted by environment name.
}

#[test]
fn a_filter_on_dev_does_not_match_a_branch_called_feature_devtools() {
    let e = HitchEvent::Promoted { environment: "qa".into(), branch: "feature/devtools".into() };
    assert_ne!(e.environment(), "dev");
    assert!(!e.branches().contains(&"dev"));
}
```

  Also cover: `EnvironmentRemoved`, `BaseChanged`, `Locked` with and without
  `locked_by`, `Unlocked`, `Rebuilt` (outcome `Unrecorded` from the pure diff,
  always), `Released`, and each approval transition:
  - new request → `ApprovalRequested`;
  - approvals grew → `ApprovalVoted` carrying the new count, with `required` =
    the *new* config's `environments[request.environment].min_approvals`, which
    is what `utils/approvals.rs` checks against. If the environment has since
    been removed, use the new approvals count, and give that fallback a test;
  - status → `Approved` → `ApprovalGranted`;
  - status → `Rejected`, or `rejection` newly `Some` → exactly **one**
    `ApprovalRejected`. Today's code emits two lines for one rejection (the
    status change *and* the `rejection` field), and that duplicate is a defect
    to fix, not preserve;
  - status → `Applied` → `ApprovalApplied`;
  - status → `Cancelled` → `ApprovalCancelled`.

  Add a test that a single commit which both adds a vote and reaches the
  threshold yields `ApprovalVoted` then `ApprovalGranted`, in that order.

- [x] Run: `cargo test -p hitch --lib core::activity` and confirm the tests
  fail to compile, because the module does not exist yet.

- [x] Implement `derive_events` in the order Constraint 7 fixes:
  `new.environments` sorted by name, then removed environments sorted, then
  approvals sorted by `id`. `derive_events` does **not** decide whether a lock
  is an operation's; it emits `Locked`/`Unlocked` faithfully. Collapsing happens
  in the reader (Task 3), which can see neighbouring commits.

- [x] Run the module's tests until they pass. Then run `just format && just
  format-check && just lint && just test`.

- [x] Commit: `P9 Task 1: typed HitchEvent model, derived purely from two configs`.

---

## Task 2 — The reader: `build_activity`, and history as data

**Files:**
- Modify: `src/core/activity.rs`
- Modify: `src/utils/git_operations.rs` (one new read primitive)
- Test: `tests/integration/log_tests.rs` (new; register it in `tests/integration/mod.rs` the way the other `*_tests.rs` files are)

**Interfaces:**
- Consumes: Task 1's `derive_events`, `HitchEvent`.
- Produces:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityEntry {
    pub commit: String,                       // full hitch-metadata commit id
    pub when: chrono::DateTime<chrono::Utc>,
    pub actor: String,                        // commit author name (%an)
    pub events: Vec<HitchEvent>,              // one commit may carry several
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkippedCommit { pub commit: String, pub reason: String }

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityLog {
    pub entries: Vec<ActivityEntry>,          // newest first
    pub skipped: Vec<SkippedCommit>,
    /// True when the walk stopped at `limit` before reaching the first commit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ActivityQuery {
    pub environment: Option<String>,
    pub branch: Option<String>,
    /// Maximum number of entries returned (not commits scanned).
    pub limit: usize,
}

pub fn build_activity(context: &GlobalContext, query: &ActivityQuery) -> anyhow::Result<ActivityLog>;
```

and in `GitOperations`:

```rust
pub struct MetadataCommit { pub sha: String, pub when: DateTime<Utc>, pub author: String }
/// `git log --first-parent --format=%H%x00%ct%x00%an <reference>`, newest first.
pub fn list_first_parent_history(&self, reference: &str) -> Result<Vec<MetadataCommit>>;
```

`--first-parent` is load-bearing. The metadata branch is written linearly by
hitch, but a `git pull` of it by a human can produce a merge, and diffing a
merge against its second parent would replay someone else's history as though
it happened here. The new primitive goes through `run_git_command`, never
`Command::new` (master Constraint 3).

- [x] **Write the failing integration tests first.** Use the harness in
  `tests/test_framework/` (`HitchTestFramework::new().with_test_environment(TestSetup::HitchInit, |env| …)`),
  driving real commands through `env.hitch` and then calling the *library*
  (`hitch::core::activity::build_activity`) on the test repo, or `hitch log
  --json` once Task 6 lands. For Tasks 2–5, call the library through whatever
  context-construction pattern `tests/integration/plan_apply_tests.rs` already
  uses to call `rebuild_environment_gated` directly. Tests:
  - `a_promote_is_one_entry_whose_actor_is_the_committer`: promote `feature/a`
    into `dev` (`--no-rebuild` keeps it to the declaration edit). The newest
    entry holds `Promoted { dev, feature/a }`, and `actor` equals the test
    repo's configured `user.name`.
  - `entries_are_newest_first_and_limit_counts_entries_not_commits`: do five
    promotes, query with `limit: 2`, and assert exactly two entries (the newest
    two promotes) and `truncated == true`.
  - `an_unreadable_historical_config_is_skipped_not_fatal`: hand-craft a
    `hitch-metadata` commit whose `hitch.json` is `{not json` (with
    `git hash-object -w` / `mktree` / `commit-tree` / `update-ref` through
    `env.git`), then make a normal promote on top. `build_activity` returns
    `Ok`. `skipped` names that commit with a reason, and the promote is still
    there. Today this fails because `read_config_at`'s `?` aborts the whole
    walk, and `details.rs`'s `.unwrap_or_default()` then hides the entire
    timeline from the desktop.
  - `a_merge_on_hitch_metadata_is_walked_by_first_parent`: create a side
    commit, merge it into `hitch-metadata` with `git merge --no-ff`, and assert
    that no event is attributed to the second parent's changes twice.

- [x] Implement. Walk newest→oldest, pairing each commit with its first parent
  in the list, and read each `hitch.json` once. Reuse the existing
  `read_config_at`, moved from `timeline.rs` into `activity.rs` and made
  `pub(crate)`. Its comment in `src/utils/config_validation.rs:5-14` names
  `timeline.rs`; update the comment to name `activity.rs`, and fix its stale
  mention of a `hitch timeline` command, which never existed. A parse failure
  pushes a `SkippedCommit`; diff the next commit against the last *readable*
  config, so the gap shows up as the combined change rather than a phantom
  "created every environment". The first commit (no parent) is diffed against
  `HitchConfig::default()` (`src/types.rs:611`, which delegates to
  `HitchConfig::new()`; check that it has no environments), so a repo's
  `hitch init` commit yields nothing, and each later `hitch add` yields
  `EnvironmentCreated`. Stop
  once `limit` entries are collected, *after* finishing the commit being read,
  and set `truncated`. Entries with no events after filtering are dropped, and
  so are entries emptied by lock collapsing in Task 3.

- [x] Apply the typed filter (Constraint 6) per event:
  - `environment: Some(e)` keeps events where `ev.environment() == e`;
  - `branch: Some(b)` keeps events where `ev.branches().contains(&b)`;
  - both keep the intersection.

  Add a test that `--env dev` excludes `Promoted { qa, feature/devtools }`.

- [x] `just format && just format-check && just lint && just test`. Commit:
  `P9 Task 2: build_activity reads metadata history as typed entries`.

---

## Task 3 — An operation's lock is not an event

**Files:**
- Modify: `src/types.rs:22-94` (`Environment`, `lock`)
- Modify: `src/utils/prelude.rs:482-560` (`with_locked_env`, `lock_environment`)
- Modify: `src/operations/metadata.rs`, the lock apply path. Find the
  `environment.lock(` call there, or the `MetadataEdit` arm for lock.
- Modify: `src/core/activity.rs` (collapse in `build_activity`)
- Test: `tests/integration/log_tests.rs`; `src/types.rs` unit tests for serde

**Interfaces:**
- Consumes: Task 2's `build_activity`.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockPurpose { Operation, Manual }

// on Environment:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub lock_purpose: Option<LockPurpose>,

impl Environment {
    pub fn lock(&mut self, user_email: String, purpose: LockPurpose); // sets all four fields
    pub fn unlock(&mut self);                                        // clears all four
}
```

Changing `lock`'s signature is deliberate: every caller has to decide, and the
compiler finds every caller. There should be exactly two, `lock_environment`
(called by `with_locked_env`) and `hitch lock`'s apply. If `lock_environment`
has other callers, each one passes `Operation`.

- [x] **Failing tests first:**
  - Serde: a `hitch.json` without `lock_purpose` deserializes with `None`. An
    unlocked environment serializes *without* the key, so older hitch versions
    see byte-identical JSON for unlocked environments. A locked one round-trips
    `"lock_purpose": "manual"`. Also add a test deserializing an `Environment`
    JSON that has an extra unknown key, which proves the "older hitch ignores
    it" claim in deviation 5.
  - `a_promote_is_not_bracketed_by_lock_events`: after `hitch promote
    feature/a dev --yes --no-push`, the log has no `Locked`/`Unlocked` event
    at all.
  - `a_refused_operation_leaves_no_trace_in_the_log`: `hitch lock dev`, then a
    promote into `dev` that refuses. The log shows exactly one `Locked { dev,
    by }` and nothing else. The refusal's lock/unlock pair is the bare bracket
    P8 recorded as "a refusal costs two commits".
  - `a_manual_lock_and_unlock_are_events`: `hitch lock dev` then `hitch
    unlock dev` gives `Unlocked`, then `Locked` (newest first).
  - `legacy_brackets_are_collapsed_by_heuristic`: hand-write three metadata
    commits with no `lock_purpose` (lock, promote, unlock, 1 s apart via
    `GIT_COMMITTER_DATE`). The promote shows and neither lock event does. Also
    hand-write a legacy lock followed by an unlock 2 hours later, with nothing
    else touching that environment in between: both show.

- [x] Implement. `with_locked_env` → `Operation`, `hitch lock` → `Manual`.
  Collapsing in `build_activity` is a post-pass over the per-commit events,
  newest to oldest, per environment:
  - A `Locked`/`Unlocked` event whose *new* config (or *old*, for `Unlocked`)
    has `lock_purpose == Some(Operation)` is dropped.
  - `Some(Manual)` is kept.
  - `None` is legacy: a `Locked` whose matching `Unlocked` is ≤ 60 s later, in
    the same walk, is an operation bracket and both are dropped; otherwise both
    are kept. Name the constant `LEGACY_OPERATION_LOCK_WINDOW`, with a comment
    saying the heuristic exists only because history written before
    `lock_purpose` cannot say which kind of lock it was. A bracket still open
    at the walk's `limit` edge counts as unmatched and is kept, which is the
    conservative choice.

  To make the post-pass possible, `derive_events` must not lose which config
  carried the purpose. Have the reader pass it along. The simplest route is to
  make the reader, not `derive_events`, look at `new.environments[e].lock_purpose`
  when it sees a lock event for `e`. Keep `derive_events`' signature as Task 1
  defines it.

- [x] Check that `hitch status`, `hitch why` and the lock refusal messages
  ("locked by X") are unchanged: `lock_purpose` is read by the log only.
  `just test` covers that.

- [x] Update `AGENTS.md`'s locking-discipline paragraph (Conventions): the
  persisted `locked` flag now records *why* in `lock_purpose`, and history uses
  it. Gates. Commit: `P9 Task 3: an operation's own lock leaves no event`.

---

## Task 4 — A rebuild's outcome, only where it is provable

**Files:**
- Modify: `src/core/activity.rs`
- Test: `tests/integration/log_tests.rs`

**Interfaces:**
- Consumes: `crate::utils::build_record::{read_state, EnvironmentBuildState, EnvironmentBuildRecord}`
  (read `read_state`'s return enum first; only its `Known` arm, or whatever the
  arm carrying a parsed record is called, is used here). Also consumes a
  git ancestry check. `get_merge_base(a, b)` exists
  (`src/utils/git_operations.rs:2908`); `is_ancestor(a, b)` is
  `get_merge_base(a, b) == Some(a)`. Do not add a new subprocess helper for it.
- Produces: `Rebuilt { outcome }` filled with `Clean` or `WithHolds` on at
  most one event per environment.

**The attachment rule (Constraint 5).** For environment `e` with a `Known`
record `r`, let `R1` be the newest `Rebuilt { e }` event in the walk (at commit
`c1`), and let `R2` be the next older one (at `c2`, possibly absent). Attach
`r` to `R1` only if both hold:
1. `r.metadata_sha` is an ancestor of `c1` (or equal to it); and
2. `R2` is absent, or `r.metadata_sha` is **not** an ancestor of `c2`.

Together they say that `r` was written by the rebuild whose `rebuilt_at` stamp
is `c1`. If the ancestry calls error, or the record is not `Known`, leave the
outcome at `Unrecorded`. Map the record like this:
- `included` = `r.included_branches` names, in order;
- `held` = `r.held` → `HoldPair` (reuse whatever conversion
  `operations/declaration.rs` uses to fill `DependentEnvironmentRebuild.held`);
- `held` empty → `Clean`, else `WithHolds`.

- [x] **Failing tests first:**
  - `the_latest_rebuild_carries_its_holds`: set up the conflicting pair the
    existing hold tests use (grep `tests/integration/rebuild_tests.rs` for a
    helper creating two branches that conflict on one file), promote both, and
    rebuild. The newest `Rebuilt { dev }` has `WithHolds` naming the held
    branch and its partner.
  - `an_older_rebuild_is_unrecorded_not_clean`: rebuild twice (touch a branch
    in between). Only the newest rebuild event has a known outcome; the older
    one is `Unrecorded`.
  - `a_record_from_a_rebuild_outside_the_walk_is_not_misattributed`: rebuild,
    then `limit` the walk so it covers only later commits (promote after the
    rebuild, `limit: 1`). No event carries the record.
  - `a_legacy_repo_without_a_record_says_unrecorded`: rebuild, then
    `git update-ref -d refs/hitch/state/dev`, the same simulation
    `test_an_environment_built_without_a_record_is_legacy_unknown` uses. Assert
    that the ref existed before deletion so the test cannot pass vacuously.

- [x] Implement as a post-pass after collapsing. Gates. Commit: `P9 Task 4:
  attach the build record to the one rebuild it provably describes`.

---

## Task 5 — Words: `render_event` and `render_activity`

**Files:**
- Modify: `src/core/render.rs`
- Test: unit tests in `src/core/render.rs`'s test module (golden strings)

**Interfaces:**
- Consumes: Tasks 1–4's types.
- Produces:

```rust
/// One event as one sentence, no actor, no time, no glyph. The desktop adapter's `summary`.
pub fn render_event(event: &HitchEvent) -> String;

/// The whole log. `now` is injected (pure; §36.6) and only used for day headings.
pub fn render_activity(log: &ActivityLog, now: chrono::DateTime<chrono::FixedOffset>, verbose: bool) -> String;
```

**The words** (§19's target, §37's copy rules; `render.rs` owns every one):

| Event | `render_event` |
|---|---|
| `EnvironmentCreated` | `created environment dev from main` |
| `EnvironmentRemoved` | `removed environment dev` |
| `BaseChanged` | `changed dev's base from main to develop` |
| `Promoted` | `added feature/a to dev` |
| `Demoted` | `removed feature/a from dev` |
| `Locked` | `locked dev` (`by` omitted: the actor already says who) |
| `Unlocked` | `unlocked dev` |
| `Rebuilt` / `Unrecorded` | `rebuilt dev` |
| `Rebuilt` / `Clean` | `rebuilt dev` |
| `Rebuilt` / `WithHolds` | `rebuilt dev, holding dashboard` |
| `Released` | `released dev` |
| `ApprovalRequested` | `asked to add feature/a to prod` (or `remove … from` for a demote request) |
| `ApprovalVoted` | `approved adding feature/a to prod (1 of 2)` |
| `ApprovalGranted` | `adding feature/a to prod is approved` |
| `ApprovalRejected` | `rejected adding feature/a to prod` |
| `ApprovalApplied` | `applied the approved change: feature/a to prod` |
| `ApprovalCancelled` | `cancelled the request to add feature/a to prod` |

`render_activity` layout: newest first, grouped under a day heading (`Today`,
`Yesterday`, or `Mon 28 Sep 2026`), computed from `now`'s offset. Each entry is
`HH:MM  <actor> <first event sentence>`, and any further events in the same
entry go on indented continuation lines. A `WithHolds` rebuild adds a
continuation line per hold: `dashboard was held — it conflicts with payments`
(the §37 copy). This reuses the hold sentence P7 put in `render_why` /
`render_plan`, so find that helper and call it rather than writing a third
wording. After the entries:
- If `skipped` is non-empty: `N change(s) to hitch's settings could not be
  read and are not shown.`
- If `truncated`: `Older activity not shown — use --limit to see more.`
- If the log is empty: `No activity yet.`

When `verbose` is set, each entry line ends with `  (metadata commit
<short>)`. That is mechanism, so it appears under verbose only.

Where the newest entry touching an environment is a `Promoted`, `Demoted` or
`BaseChanged` with no later `Rebuilt` for that environment in the log, add
the continuation line `dev has not been rebuilt since — see hitch status`.
This is deviation 2's pointer. It is a statement about *this log*, and it is
worded so it does not claim the environment needs a rebuild: that verdict
belongs to `core::state`.

- [x] **Failing golden tests first**, one per table row, plus:
  - day grouping across midnight with a fixed `now`;
  - a `WithHolds` rebuild's continuation lines;
  - `skipped`, `truncated` and empty footers;
  - verbose adds the commit and non-verbose never contains `commit`;
  - the "not rebuilt since" pointer, both present and absent.
- [x] Implement. Gates. Commit: `P9 Task 5: render the deployment story`.

---

## Task 6 — `hitch log`

**Files:**
- Create: `src/commands/log.rs`
- Modify: `src/commands/mod.rs`, `src/cli.rs` (`Commands::Log`, and the
  `--json` doc comment at `:38-55`), `src/main.rs` (`command_name` match,
  dispatch match, and `command_is_mutating`'s `false` arm at `:173-193`)
- Modify: `tests/integration/json_support_tests.rs` (the read-only-commands
  test at `:205`), `SKILL.md` (command reference), `README.md` (command list
  only; full docs are P10)
- Test: `tests/integration/log_tests.rs`

**Interfaces:**
- Consumes: `build_activity`, `render_activity`, `emit_json`, `JSON_SCHEMA_VERSION`.
- Produces:

```rust
#[derive(clap::Args)]
pub struct LogCommand {
    /// Show only events for this environment
    #[arg(long = "env")]
    pub environment: Option<String>,
    /// Show only events that name this branch
    #[arg(long)]
    pub branch: Option<String>,
    /// How many entries to show
    #[arg(long, short = 'n', default_value_t = 20)]
    pub limit: usize,
    /// Also show which metadata commit each entry came from
    #[arg(long)]
    pub verbose: bool,
}
```

Help text first line: `Show what happened to your environments — who promoted,
rebuilt, released, locked and approved what (not a git log)`. Model the
command on `src/commands/why.rs` (thin; `emit_json` of a two-key document or
`println!` of the render; `context.verbose = args.verbose`). JSON document:

```rust
#[derive(serde::Serialize)]
struct LogDocument { schema_version: u32, log: ActivityLog }
```

Pass `now` as `chrono::Local::now().fixed_offset()`.

An `--env` naming an environment that is neither in the current config nor in
any scanned event is an `Err`. The error lists the environments that do exist
and ends with `hitch status` as the next step (AGENTS.md error convention). An
environment that existed and was removed is valid, and its history is exactly
what the reader wants.

- [x] **Failing tests first:**
  - `hitch_log_tells_the_story`: create an environment, promote two branches,
    rebuild, then `hitch log`. Assert stdout contains `Today`, `added
    feature/a to dev`, and `rebuilt dev`, and does **not** contain `Locked` or
    `metadata commit`.
  - `hitch_log_json_is_one_document_with_snake_case_enums`: parse stdout as one
    JSON value, and assert `schema_version == 1`, `log.entries[0].events[0].kind`
    is snake_case, and stderr carries anything else. Reuse the PascalCase
    collector from `tests/integration/why_tests.rs:91` by moving it into
    `tests/test_framework/` (or a shared test helper module) so both files call
    one copy.
  - `hitch_log_does_not_take_the_repo_lock`: hold the repo lock the way the
    `why` lock test does (grep `why_tests.rs` for it) and assert `hitch log`
    still exits 0.
  - `hitch_log_unknown_env_is_an_error_with_a_next_step`.
  - `hitch_log_verbose_shows_the_metadata_commit`.
- [x] Implement and register (four places, as `AGENTS.md`'s `src/cli.rs` entry
  lists). Update the `--json` doc comment to add `log`: fourteen commands, three
  read-only ones, and "`status`, `why` and `log` have no 'after' half". Rename
  `the_two_read_only_json_commands_are_status_and_why` to
  `the_read_only_json_commands_are_status_why_and_log` and update its set.
- [x] Build the debug binary and run `hitch log`, `hitch log --env dev`,
  `hitch log --json | jq .`, and `hitch log --verbose` against a throwaway
  repo. Read the output as a user would.
- [x] Update `AGENTS.md`: the `src/cli.rs` entry (fourteen, three read-only),
  a `src/core/activity.rs` architecture-map entry (pure model plus one reader,
  and the lock-collapse and record-attachment rules in one line each), and the
  `core/timeline.rs` note (now an adapter; see Task 8). Gates. Commit:
  `P9 Task 6: hitch log`.

---

## Task 7 — Terminology: default output explains meaning

**Files (known offenders from the pre-plan inventory; the test below finds any
others):**
- `src/commands/approvals/status.rs:163,168` (`Base branch SHA: …` and per-branch
  SHAs): label as `commit`, e.g. `main at commit 1a2b3c4`
- `src/commands/approvals/refresh.rs:43` (`No SHA drift detected`): `Nothing
  has changed since this request was filed — its snapshot is current.`
- `src/commands/init.rs:28,70-73` (`hitch-metadata` named in normal output).
  Reword to "hitch's settings branch". The *pasteable* remedy `git push origin
  hitch-metadata` stays verbatim, because it is a command (Constraint 11,
  exception 2).
- `src/core/render.rs:269` (`Clean up N refs`): headline by what the plan
  actually holds, e.g. `Clean up 2 branches and 4 archived builds`. Read
  `CleanupPlanDetail`'s fields for the split. `count()` already pluralises.
- `src/core/render.rs:1624` (`WhyReason::NoRef` "no branch ref resolves for
  it"): `no branch by that name exists, locally or on origin`
- `src/utils/snapshot.rs:48` (`Failed to get SHA for branch`): `Could not read
  branch '…'`, keeping `{e}` for the mechanism
- `src/commands/status.rs:295` (`detached HEAD`): keep. It describes the
  user's own checkout in the term git itself shows them, and that is a
  deliberate exception. Record it in the test's allow-list with that reason.
- `with_locked_env`'s `✓ Environment 'x' unlocked successfully`
  (`src/utils/prelude.rs:~518`) and `lock_environment`'s `Environment 'x'
  locked by …` (`:~544`): move both to `log_verbose`. They are the operation's
  mechanism, not its meaning, and P8's manual check found them wrapped around
  every plan. The *human* `hitch lock` has its own receipt and loses nothing.
- **Approval-gated promote** (inherited from P8's manual check). It prints
  "Environment 'prod' requires approval before promotion" once before the plan
  and again inside it, and its plan heads the gate with ⛔ "Why this cannot
  apply" even though the operation goes on to file a request. Find the
  pre-plan line (grep `requires approval` in `src/commands/promote.rs` and
  `src/operations/declaration.rs`) and delete it: the plan says it. Render
  `PlanWarningKind::ApprovalRequired` under its own heading, `Needs approval`,
  not the blocking `⛔ Why this cannot apply`, and with the non-blocking `⏳`
  glyph. It *is* blocking in the model, since the declaration edit does not
  happen, and the model stays as it is. This is a wording decision about what
  the reader is told the operation will do, so it lives in `render.rs` alone.
- Internal-invariant errors (`git_operations.rs:357,1538,1744`, "produced no
  tree OID"): keep them verbatim, as Constraint 11 exception 1. They report a
  broken assumption in hitch's own git plumbing, and a user who sees one
  forwards it.
- Test: `tests/integration/terminology_tests.rs` (new)

**Interfaces:**
- Consumes: every command.
- Produces: `terminology_tests.rs`, which future phases extend when they add
  output.

- [x] **Write the enforcing test first.** One scenario script drives the
  common commands' normal paths, without `--verbose` or `--json`, against one
  repo:
  - `init`, `add qa`, promote two branches (one conflicting, so a hold occurs),
    `rebuild dev`, `status`, `why`, `log`, `lock`/`unlock`;
  - a refused promote into a locked environment;
  - an approval-gated promote and `approvals status`;
  - `cleanup`, and `release` of `dev` into `main`.

  Collect stdout and stderr of every step and assert that none contains any of
  the forbidden tokens from Constraint 11, matched as case-insensitive whole
  words (`\bsha\b`, `\boid\b`, `\bref\b`, `\brefs/`, `\bcas\b`, `eject`,
  `materiali[sz]`, `merge-tree`, `update-ref`, `commit-tree`,
  `force-with-lease`, `\bjournal\b`, `fingerprint`, `\banchor`,
  `hitch-metadata`). Each match gets through only if it is on an explicit
  allow-list, and every allow-list entry carries a reason string. `detached
  HEAD` is allowed (Constraint 11 is about *hitch's* internals, not the user's
  checkout), and so is a pasteable `git …` remedy line. A second test runs a
  subset with `--verbose` and asserts that at least one mechanism term *does*
  appear, which proves the verbose channel still carries it and that the
  suppression was a move, not a deletion.
- [x] Run it. It fails on the offenders above; that is the red.
- [x] Fix each offender as listed. Existing tests asserting the old strings
  change with them. Keep that list in the commit message body.
- [x] Gates. Commit: `P9 Task 7: default output explains meaning, verbose
  explains mechanism`.

---

## Task 8 — `timeline.rs` becomes an adapter

**Files:**
- Modify: `src/core/timeline.rs` (shrinks to an adapter)
- Modify: `src/core/details.rs:40-50, :81-91` only if the filter type changes
  (it should not; see below)
- Test: unit tests in `src/core/timeline.rs`

**Interfaces:**
- Consumes: `build_activity`, `render_event`.
- Produces: unchanged public surface. `TimelineItem { when, kind, summary,
  detail }`, `TimelineKind`, `HitchEventFilter`, `HitchEventScope`,
  `build_combined_timeline(context, reference, commit_limit, hitch_limit,
  filter)`, and `build_hitch_events(context, max, filter)` keep their names,
  fields and signatures (Constraint 8). Add
  `pub event: Option<crate::core::activity::HitchEvent>` to `TimelineItem`
  (`None` for `GitCommit` items). That is the typed hook a future desktop
  migration reads instead of `summary`.

- [x] **Failing test first:** build an in-memory `ActivityLog` with one
  `Promoted { qa, feature/devtools }` entry and convert it through the adapter
  function (factor out `fn items_from_log(log: &ActivityLog, filter:
  &HitchEventFilter) -> Vec<TimelineItem>`, which is pure). An `Environment`
  scope with `environment: Some("dev")` yields nothing. Today's substring
  filter would yield one item.
- [x] Reimplement `build_hitch_events` as:
  1. `build_activity` with `limit: max_commits` and the filter's
     environment/branch mapped into `ActivityQuery`; `Any` maps to no filter;
  2. then one `TimelineItem` per *event*, not per entry, because the desktop
     shows one line per event today, with `summary: render_event(&e)`,
     `when: entry.when`, `kind: HitchEvent`, `event: Some(e)`,
     `detail: None`.

  Delete `diff_configs_to_events`, `diff_approvals`, `status_word`,
  `approval_short`, `push_if_match`, `matches_filter`, and the old
  `read_config_at`, which moved in Task 2. `hitch_limit` changes meaning from
  "metadata commits scanned" to "entries". Record that in the adapter's doc
  comment; the desktop passes 80 and gets at most 80 events' worth of entries.
- [x] Run `git diff --name-only main..HEAD -- crates/` and confirm it is empty.
  Check the desktop's use sites by reading them
  (`crates/hitch-desktop/src-tauri/src/types.rs:122-180`). Every field it
  reads still exists, and adding `event` cannot break its struct-literal-free
  `From` impl.
- [x] Gates. Commit: `P9 Task 8: timeline.rs is an adapter over the event model`.

---

## Task 9 — Manual verification, gates, and documentation

**Files:** `AGENTS.md`, `docs/superpowers/plans/2026-09-25-explainable-ux-program.md`, this file.

- [x] Build the **debug** binary (`cargo build -p hitch`) and, in a throwaway
  repo under `/tmp`, run with `--yes --no-push` a realistic day:
  - create `dev` and `qa`;
  - promote three branches, two of them conflicting;
  - rebuild `dev`, which holds one;
  - lock and unlock `qa` by hand;
  - make a refused promote into the locked `qa`;
  - run an approval round trip on an approval-gated environment;
  - release `dev`.

  Then read `hitch log`, `hitch log --env qa`, `hitch log --branch <b>`,
  `hitch log --verbose` and `hitch log --json` as a user would:
  - Does it read like §19's target?
  - Does any operation lock leak through?
  - Is the held branch named with its partner?
  - Does the refused promote leave no trace?
  - Does the release's post-rebuild show?

  Delegate this to a fresh agent with the same brief shape P8's Task 11 used.
  That walkthrough found nine defects a green suite had not.
- [x] Gates, in order, all clean: `just format`, `just format-check && just
  lint`, `just test`.
- [x] `AGENTS.md`:
  - `core/activity.rs` in the architecture map (if Task 6 did not already add
    it);
  - `core/timeline.rs` as an adapter with a P10 deletion note;
  - `LockPurpose` in the locking-discipline convention;
  - the terminology test and its allow-list rule ("a new allow-list entry needs
    a reason string, and the reason is reviewed like code");
  - fix the gotcha that says `get_commit_timestamp` has "exactly one production
    caller left, `core/timeline.rs:96`" — after Task 2 it moves to
    `core/activity.rs` or disappears, because `list_first_parent_history`
    carries the time.
- [x] Master plan: move P9 to **COMPLETE** with the test count, deviations,
  and commit list entries. Update the header status line (`P0–P9 are complete.
  P10 is next.`) and "Where this work lives". P10's inheritance is authored
  at P10, for the usual reason. List for P10 at minimum:
  - delete the timeline adapter once the desktop reads `event`;
  - delete `StepNarration` and the `on_step` plumbing (P8's inheritance);
  - reconsider the legacy lock heuristic once no un-purposed history is within
    anyone's default `--limit`.
- [x] Add `## As executed` to this file: what landed, every deviation from the
  tasks above, and what P10 inherits.

## As executed

**Commits** (`8c7ebb0` is the plan; suite at completion in the master plan):

| Task | Commits |
|---|---|
| 1 event model | `5a81729` |
| 2 reader | `6596fbb` |
| 3 lock collapse | `101f352`, `6d828b8` |
| 4 rebuild outcome | `46e4c07`, `0185bea` |
| 5 words | `12c8fd9`, `3c5c787` |
| 6 `hitch log` | `f80a48c`, `dfecbbb` |
| 7 terminology | `12bb841`, `9c7d753`, `9e0f1da` |
| 8 timeline adapter | `48fc111` |
| 9 walkthrough fallout, docs | `c49d713`, `28f7794`, then the docs commit |

**What landed.** `src/core/activity.rs`: typed `HitchEvent`, pure `derive_events`,
one reader `build_activity` over `hitch-metadata` first-parent history, lock
collapse driven by `Environment.lock_purpose`, build-record attachment.
`render_event`/`render_activity` in `render.rs`. Read-only `hitch log`
(`--env`, `--branch`, `--limit`, `--verbose`, `--json`; fourteenth `--json`
command, third read-only). A terminology test that drives one scenario through the
common commands and fails on mechanism words and path-shaped ref names, with a
reasoned allow-list. `core/timeline.rs` is an adapter over the event model.

**Rulings** (why / cost):
- T2 tests strip Locked/Unlocked events before asserting; T3 restores the plan's exact assertions. Why: they are only true once T3 collapses operation locks. Cost: T2's tests were weaker for one task.
- T4 attachment: a truncated walk with one visible rebuild resolves conservatively, and attachment precedes the branch filter. Why: wrong attachment is worse than `Unrecorded`; `Unrecorded` has no branches. Cost: none.
- **Range rule replaces the plan's R2 condition.** Attach record `r` to the newest `Rebuilt{e}` at commit `c1` iff `r.metadata_sha` is ancestor-or-equal of `c1` and no first-parent commit in `metadata_sha..c1^` changes `e.rebuilt_at`; any unreadable commit in the range gives `Unrecorded`. Why: provable without the previous rebuild in view, so it works at the default `-n 20`; R2 made the feature silent in the common case. Cost: a crash between record and stamp could misattribute (reviewer argues it fails the ancestry condition).
- T5 adds one private hold-sentence helper in `render.rs` and does not refactor existing render sites. Why: out of scope, risk to P6/P7 golden tests. Cost: a fourth hold wording until P10.
- **All six approval variants carry `direction: ApprovalDirection`** (extends Task 1's interface). Why: the wording table needs it and "applied" is wrong for demotes. Cost: small churn.
- The not-rebuilt-since pointer ignores Locked/Unlocked-only entries. Why: a manual lock does not rebuild. Cost: none.
- T6 has no "why lock test" (the brief cited a nonexistent one); the test takes `RepoLock::acquire` in-test.
- Walkthrough: D1-D4 fixed in P9; short SHAs stay in normal output (spec §17); D3's Applied triple, D5, D6 deferred as pre-P9 paths.
- **D2: `Released` stays `released dev` with no target; it is hoisted to the front of its commit's events.** Why: the target is overridable and not persisted, and Constraint 3 allows only `lock_purpose` as a new field. Cost: the log does not say where `dev` was released to.

**Deviations from the tasks as written:**
- The spec's `RebuiltWithHolds` is `Rebuilt { outcome: WithHolds }`, not a separate variant (refines deviation 1).
- Attachment uses the range rule above, not R2.
- Approval direction on all variants; `Released` first and no `into`.
- `Environment.lock_purpose` is the one new persisted field; `Environment::lock` takes the purpose.
- D1: `ActivityLog.branch_filtered` is `serde(skip)`; a branch-filtered log prints no pointer.
- D3: a vote and its grant in one entry render as one line.
- D4: the anchors block reads "The new build is kept safe until it is published."; the terminology test also catches `<family>/<name>/` ref paths (`REF_FAMILIES`).
- `Cargo.toml` gained a stray `2.0.0` bump in `101f352`; reverted in `6d828b8`. Origin unknown: an external process changed it mid-session.

**Deferred minors:** new-env `continue` drops same-commit branch/lock events; `Rebuilt` branches for Clean/WithHolds untested and `--branch` matches only rebuilds with a known outcome; a rejection field set on an already-rejected request yields a second `ApprovalRejected`; look-ahead may record `skipped` beyond truncation and `truncated` is true when the limit lands on the last event-bearing commit; range-check unreadable commits beyond the walk count in `skipped`; `--branch` + limit counts filtered-out entries; a truncated-walk test leans on commit layout; `log --verbose` overrides global `--verbose` (as `why`); `cli.rs` `--json` doc lines are long; `branches_noun` hand-rolled plural; cleanup classified by `refs/hitch/` prefix; a `lock_unlock_tests` "settings" count is brittle; `Expect::Any` prompt steps do not assert prompt text; corrupt-config default warning is unit-tested only; adapter tests miss None-scope returns, Rebuilt-by-Branch and GitCommit `event == None`; single-commit `hitch-metadata` now yields `EnvironmentCreated`; unnecessary `#[allow(dead_code)]` at `timeline.rs`; `REF_FAMILIES` omits `pending-resync`; "new build is kept safe" wording also used for release; D3 scan is O(n^2); D1 `take(0)` idiom.

### What P10 inherits
- Delete the timeline adapter once the desktop reads `event`.
- Delete `StepNarration` and the `on_step` plumbing (from P8).
- Reconsider the 60 s legacy lock heuristic once no un-purposed history is within a default `--limit`.
- `get_commit_timestamp` has no production caller; delete it.
- The release target is not recorded; derive it from the release tag or record it.
- D5: the release plan's "Will not change" list is wrong (duplicates, and lists environments it rebuilds).
- D6: a conflict-refused promote narrates a rollback and names the wrong partner (`main` instead of `payments`); a release failure's remedy repeats the failing command.
- An approval apply still shows as three log lines (D3, Applied triple).
- The adapter drops `skipped`/`truncated`, so the desktop shows a partial timeline silently.
- An operation lock left by a crash or `rebuild --force` has `lock_purpose` Operation, so neither it nor the manual `hitch unlock` that clears it appears in `hitch log`; fixing it needs a marker written at unlock time.
