<!--
PROVENANCE — read this before the document below.

This file is a VERBATIM copy of the original specification. Nothing below the
divider has been edited, reworded, reordered, or abridged; the 42 sections are
the author's, in the author's order.

  Source:      ~/Downloads/HITCH_EXPLAINABLE_UX_IMPLEMENTATION_SPEC.md
  Imported:    2026-09-25
  Baseline:    doomedramen/hitch main @ 85f3300c7dad109f6d7cde34b6406aaedeb9daee
  Size:        2628 lines / 57,177 bytes, sections 1–42

The document's own §28 milestone plan and §29 recommended order are the
authoritative statement of intent. The actual implementation plan lives in
docs/superpowers/plans/2026-09-25-explainable-ux-program.md and deliberately
departs from it in three places, all recorded there:

  1. SCOPE — §20–§26, §30.5, and milestones M9/M10/M11 (hitch-desktop) are
     deferred. `crates/hitch-desktop` does not currently compile (5 errors at
     import time) and is tracked on a separate repair stream. The shared typed
     API those milestones specify is still built, on the CLI side, because the
     CLI is the first consumer.

  2. MILESTONE 0 — the baseline repair (broken `main`: newer-clippy `?`-operator
     lint, a Windows-only temp-path test failure, and the desktop compile
     errors) is out of scope for this program and handled separately. The
     scenario-inventory slice of M0 IS in scope, as phase P0.

  3. ORDER — this document's §29 puts M1 (state model) before M2 (build
     provenance). The implementation inverts them, and extracts the shared
     composition primitive ahead of both. Rationale in the master plan.

Everything else — the Desired/Actual/Proposed vocabulary, the
OperationPlan/ExecutionReceipt shapes, the plan fingerprint, the Desired/Actual
distinction, the `hitch why` command, the status matrix, plan-vs-apply invariant
testing, and §40's definition of done — is in scope and unchanged.
-->

---

# Hitch Explainable UX Redesign — Implementation Specification

**Status:** Proposed  
**Scope:** CLI + shared Rust core + `hitch-desktop`  
**Primary goal:** Make Hitch transparent, predictable, and state-oriented without weakening or replacing its existing Git safety model.  
**Baseline reviewed:** `doomedramen/hitch` `main` at commit `85f3300c7dad109f6d7cde34b6406aaedeb9daee`.

---

## 1. Executive summary

Hitch already has a strong internal model:

- environments are declared as `base + ordered promoted branches`;
- environment branches are generated outputs;
- inputs are synchronized and pinned before composition;
- rebuilds compose away from the user's checkout;
- conflicts can hold a branch instead of blocking the whole environment;
- publication uses atomic ref updates and crash-recovery journaling;
- releases compose feature branches into a durable target and publish atomically;
- metadata lives separately on `hitch-metadata`;
- the desktop app already exposes environments, branches, approvals, timelines, and mutation commands.

The usability problem is that much of this intelligence is hidden behind imperative commands and prose logs.

The redesign should make Hitch feel like a **declarative state manager**:

```text
INTENT → PLAN → APPLY → RESULT
```

For any meaningful mutation, a user should be able to answer:

1. **Why is Hitch doing this?**
2. **What exactly is Hitch going to change?**
3. **What state will exist afterwards?**

The implementation should therefore expose the state Hitch already reasons about instead of adding more verbose logging.

The core architectural change is a shared structured domain layer:

```text
Repository snapshot
        ↓
      Planner
        ↓
  OperationPlan
   ↙    ↓     ↘
 CLI   Desktop  JSON
        ↓
     Executor
        ↓
 ExecutionReceipt
```

The CLI and desktop must consume the **same structured plan and result**. They must not independently reconstruct what an operation means from command output strings.

---

# 2. Design principles

## 2.1 Make the declarative model visible

Hitch's central equation should appear throughout the product:

```text
dev = main + feature/auth + feature/payments + feature/dashboard
```

The UI should consistently distinguish:

- **source** — durable base and feature branches;
- **desired state** — what `hitch-metadata` says an environment should contain;
- **actual state** — what the currently published environment build contains;
- **proposed state** — what a pending operation would produce.

Do not model environments as a pipeline in which `dev` is merged into `qa`.

A feature can independently be included in any environment:

```text
feature/auth
  dev   ✓
  qa    ✓

feature/dashboard
  dev   ✓
  qa    —
```

---

## 2.2 Explain behaviour, not implementation

Normal output should explain Hitch's semantics:

```text
Requested:
  add feature/payments to dev

Current:
  dev = main + auth + search

Proposed:
  dev = main + auth + search + payments

Compatibility:
  ✓ all branches compose

Will change:
  hitch-metadata
  dev
  origin/dev

Will not change:
  qa
  main
```

It should **not** primarily explain implementation mechanics:

```text
fetch
merge-tree
worktree
update-ref
CAS
OID
```

Those details belong in `--verbose` diagnostics.

---

## 2.3 One source of truth for planning

Never implement separate planning logic for:

- CLI preview;
- desktop confirmation;
- `--dry-run`;
- JSON output;
- execution.

The same planner must produce the same `OperationPlan` for all consumers.

The executor must consume that plan, or a validated equivalent of it.

---

## 2.4 Plans must be safe against staleness

A desktop user may spend 30 seconds reading a plan before clicking **Apply**.

The repository may change during that time.

Every plan therefore needs a fingerprint containing the exact state on which it was calculated, including as applicable:

- `hitch-metadata` SHA;
- base branch SHA;
- promoted feature SHAs;
- current environment branch SHA;
- observed remote branch SHA/lease;
- relevant configuration values;
- recorded resolution IDs/content keys used by the plan.

Before applying a previously displayed plan:

```text
if fingerprint still valid:
    execute
else:
    refuse stale plan
    calculate a fresh plan
    show what changed
```

Never silently execute a materially different operation from the one the user reviewed.

CLI commands that plan and apply during one invocation may keep the repository lock for the short confirmation window, but should still retain the fingerprint invariant.

---

## 2.5 Preserve Hitch's existing safety architecture

This project is **not** a rewrite of Hitch's merge/publish engine.

Keep and reuse:

- pinned branch inputs;
- isolated composition;
- `merge_tree_compose`;
- repo-wide locking;
- per-environment locking;
- compare-and-swap ref publication;
- `publish_journal`;
- checkout resynchronisation safety;
- remote leases;
- held-branch semantics;
- release all-or-nothing semantics;
- approval behaviour;
- recorded-resolution safety and lineage checks.

The UX layer should reveal these guarantees, not replace them.

---

# 3. Non-goals

Do **not** use this redesign as an excuse to:

- replace Git;
- replace the current merge/composition algorithm;
- introduce a server or central database;
- make the desktop app the source of truth;
- store derived hold/build state in `hitch.json`;
- make environment branches hand-editable;
- turn `dev → qa → main` into a mandatory linear pipeline;
- automatically resolve conflicts with heuristics;
- remove advanced Git terminology from verbose/debug surfaces;
- break old `hitch.json` files;
- change existing command names without a separate deprecation process;
- weaken `release` into a partial/ejecting operation.

---

# 4. Current code areas to preserve and build upon

Before implementation, an agent should read:

- `AGENTS.md`
- `README.md`
- `src/cli.rs`
- `src/main.rs`
- `src/types.rs`
- `src/utils/prelude.rs`
- `src/utils/git_operations.rs`
- `src/utils/publish_journal.rs`
- `src/utils/resolutions.rs`
- `src/utils/output.rs`
- `src/utils/confirm.rs`
- `src/core/status.rs`
- `src/core/workspace.rs`
- `src/core/workspace_index.rs`
- `src/core/details.rs`
- `src/core/timeline.rs`
- `src/commands/promote.rs`
- `src/commands/demote.rs`
- `src/commands/rebuild.rs`
- `src/commands/release.rs`
- `src/commands/status.rs`
- `src/commands/tree.rs`
- `src/commands/diff.rs`
- `src/commands/conflicts.rs`
- `src/commands/resolve.rs`
- `crates/hitch-desktop/src-tauri/src/main.rs`
- `crates/hitch-desktop/src/ui/types.ts`
- `crates/hitch-desktop/src/ui/tauri.ts`
- `crates/hitch-desktop/src/ui/App.tsx`

The current desktop backend invokes command `run()` functions and streams their human-readable log lines. This coupling should be removed progressively.

---

# 5. Target domain model

Names below are guidance, not a requirement. Preserve the conceptual boundaries even if exact Rust names differ.

## 5.1 Repository state snapshot

Add a read-only structured model representing the state Hitch is reasoning about.

Suggested shape:

```rust
pub struct RepositoryStateSnapshot {
    pub metadata_sha: String,
    pub current_branch: Option<String>,
    pub environments: Vec<EnvironmentState>,
    pub features: Vec<FeatureState>,
    pub captured_at: DateTime<Utc>,
}
```

### Environment state

```rust
pub struct EnvironmentState {
    pub name: String,
    pub base: BranchInput,
    pub desired: DesiredComposition,
    pub actual: ActualComposition,
    pub health: EnvironmentHealth,
    pub locked: bool,
    pub approval_policy: ApprovalPolicySummary,
}
```

### Branch input

```rust
pub struct BranchInput {
    pub name: String,
    pub sha: Option<String>,
    pub local: bool,
    pub remote: bool,
}
```

### Desired composition

```rust
pub struct DesiredComposition {
    pub base: BranchInput,
    pub branches: Vec<DesiredBranch>,
}
```

Ordering is significant and must never be lost.

### Actual composition

```rust
pub struct ActualComposition {
    pub environment_sha: Option<String>,
    pub base_sha: Option<String>,
    pub branches: Vec<ActualBranch>,
    pub build_state: ActualBuildState,
}
```

Possible `ActualBranch` states:

```rust
pub enum ActualMembership {
    Included,
    Held,
    AlreadyInBase,
    Missing,
    Unknown,
}
```

Do not pretend to know actual composition when provenance is unavailable. Use `Unknown` explicitly.

---

# 6. Persist reliable build provenance

The new Desired / Actual distinction requires a trustworthy record of what the currently published environment branch actually contains.

Do not infer this solely from timestamps or parse human commit messages.

Introduce a derived build record outside `hitch.json`.

Suggested namespace:

```text
refs/hitch/state/<environment>
```

Suggested record:

```rust
pub struct EnvironmentBuildRecord {
    pub schema_version: u32,
    pub environment: String,

    pub metadata_sha: String,

    pub base_name: String,
    pub base_sha: String,

    pub desired_branches: Vec<PinnedBranch>,
    pub included_branches: Vec<PinnedBranch>,

    pub held: Vec<HoldRecord>,
    pub replayed_resolutions: Vec<ResolutionUse>,

    pub result_sha: String,
    pub built_at: DateTime<Utc>,
    pub hitch_version: String,
}
```

### Requirements

- This is **derived state**, not user-authored configuration.
- It must not live in `hitch.json`.
- Its branch order must be preserved.
- It must record pinned source SHAs.
- It must record held branch/conflict information.
- Its `result_sha` must match the environment branch tip it describes.
- It should be content-addressed as a Git blob or small commit/tree record.
- Publishing the environment branch and updating its build-state ref must be in the **same atomic ref transaction** where feasible.

Generalise `publish_branch` carefully rather than creating a second publication path.

For example:

```rust
pub struct PublishExtraRef {
    pub refname: String,
    pub new_oid: String,
    pub expected_old: Option<String>,
}
```

or a more strongly typed equivalent.

If an existing environment predates these records:

```text
actual = unknown / legacy build
```

and the UI should invite a rebuild to establish provenance.

Do not fabricate actual membership by assuming `desired - current conflicts == actual`.

---

# 7. Operation planning model

## 7.1 `OperationPlan`

Add a structured plan type shared across CLI and desktop.

Suggested structure:

```rust
pub struct OperationPlan {
    pub id: String,
    pub kind: OperationKind,
    pub intent: OperationIntent,

    pub fingerprint: PlanFingerprint,

    pub current: StateSummary,
    pub proposed: StateSummary,

    pub compositions: Vec<CompositionPlan>,

    pub effects: Vec<PlannedEffect>,
    pub unaffected: Vec<UnaffectedResource>,
    pub warnings: Vec<PlanWarning>,

    pub confirmation: ConfirmationRequirement,
}
```

### Operation kinds

At minimum:

```rust
pub enum OperationKind {
    Promote,
    Demote,
    Rebuild,
    Release,
    Lock,
    Unlock,
    SetEnvironment,
    AddEnvironment,
    RemoveEnvironment,
    Cleanup,
    ApprovalApply,
}
```

Do not force every operation into one giant function. Use a shared plan vocabulary with per-operation planners.

---

## 7.2 Composition plan

```rust
pub struct CompositionPlan {
    pub environment: String,
    pub base: PinnedBranch,
    pub branches: Vec<PlannedBranch>,
    pub result: CompositionResultPrediction,
}
```

```rust
pub struct PlannedBranch {
    pub name: String,
    pub sha: String,
    pub state: PlannedBranchState,
}
```

Possible states:

```rust
pub enum PlannedBranchState {
    Included,
    Held {
        conflicts_with: String,
        files: Vec<String>,
    },
    ReplayedResolution {
        resolution_id: String,
    },
    AlreadyInBase,
    Missing,
}
```

The plan shown to the user must preserve promotion order.

---

## 7.3 Planned effects

Represent side effects structurally.

Examples:

```rust
pub enum PlannedEffect {
    MetadataChange(MetadataChange),
    LocalRefUpdate(RefChange),
    RemoteRefUpdate(RemoteRefChange),
    DependentEnvironmentRebuild { environment: String },
    PromotionPrune { environment: String, branch: String },
    ReleaseTag { name: String },
    ApprovalRequest { environment: String, branch: String },
}
```

This drives sections such as:

```text
Will change
Will rebuild
Will push
Will prune
Will not change
```

---

## 7.4 Plan fingerprint

Suggested:

```rust
pub struct PlanFingerprint {
    pub metadata_sha: String,
    pub refs: BTreeMap<String, String>,
    pub remote_refs: BTreeMap<String, Option<String>>,
    pub resolution_keys: Vec<String>,
}
```

Add a stable digest:

```rust
pub fn digest(&self) -> String
```

The digest is useful for:

- desktop plan/apply;
- machine-readable output;
- logs;
- tests.

Do not treat the plan ID alone as proof of freshness.

---

# 8. Execution result / receipt model

Execution should return a structured result instead of requiring consumers to parse logs.

```rust
pub struct ExecutionReceipt {
    pub plan_id: String,
    pub operation: OperationKind,

    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,

    pub outcome: OperationOutcome,
    pub effects: Vec<AppliedEffect>,
    pub warnings: Vec<ExecutionWarning>,

    pub resulting_state: Option<StateSummary>,
}
```

Examples:

```rust
pub enum OperationOutcome {
    Applied,
    AppliedWithHolds,
    ApprovalRequested,
    NoChange,
}
```

Do not collapse “rebuilt with held branches” into a generic success.

Preserve the existing rebuild exit-code distinction.

---

# 9. Planner / executor separation

Suggested module direction:

```text
src/
  operations/
    mod.rs
    model.rs
    snapshot.rs
    promote.rs
    demote.rs
    rebuild.rs
    release.rs
    environment.rs
    render/
      cli.rs
      json.rs
```

Exact layout may differ, but maintain these boundaries:

```text
read repository
      ↓
produce immutable plan
      ↓
validate plan freshness
      ↓
apply plan
      ↓
produce receipt
```

Command modules should become thin argument adapters.

Do not move Git primitives out of `src/utils/git_operations.rs`.

Do not duplicate publication logic.

---

# 10. CLI target experience

## 10.1 Mutating commands show a semantic plan

Example:

```text
$ hitch promote feature/login dev

Promote feature/login → dev

Current
  dev = main + auth + search

Proposed
  dev = main + auth + search + login

Composition
  ✓ auth
  ✓ search
  ✓ login

Will change
  hitch-metadata   add feature/login to dev
  dev              rebuild from 3 branches
  origin/dev       publish rebuilt dev

Will not change
  qa
  main
  feature/login

Apply this plan? [y/N]
```

Afterwards:

```text
Applied

dev
  71ea82a → a39c102

✓ 3/3 desired branches included
✓ metadata updated
✓ origin/dev published

Your CI/CD may now deploy dev.
```

---

## 10.2 `--yes` behaviour

`--yes` should:

- skip the interactive confirmation;
- **not** suppress the plan;
- retain structured outcome output;
- remain valid for CI and agents.

A future `--quiet` may suppress human plan rendering, but do not conflate that with `--yes`.

---

## 10.3 `--json`

Add a stable machine-readable output mode.

Prefer a global:

```bash
hitch --json status
hitch --json promote feature/foo dev
```

or a consistently supported equivalent.

For mutating commands, JSON should emit:

```json
{
  "plan": { "...": "..." },
  "receipt": { "...": "..." }
}
```

If confirmation is required and not supplied, non-interactive JSON mode should fail clearly unless `--yes` is present.

Version the JSON schema:

```json
{
  "schema_version": 1
}
```

Do not mix ANSI/prose logs into JSON stdout.

Diagnostics can use stderr.

---

## 10.4 Dry-run semantics

Existing:

```bash
hitch rebuild dev --dry-run
```

should become a renderer over the same rebuild plan.

For other mutations, add a consistent preview mechanism.

Preferred direction:

```bash
hitch promote feature/foo dev --dry-run
hitch demote feature/foo dev --dry-run
hitch release qa main --dry-run
```

The plan must be identical in meaning to what a real execution would use.

Do not create a generic dry-run path that skips important fetch/pinning/preflight work and therefore predicts something different.

---

# 11. Desired / Actual / Proposed as first-class concepts

## 11.1 Definitions

### Desired

What `hitch-metadata` currently declares.

```text
dev = main + A + B + C
```

### Actual

What the latest published environment build record says is currently in `dev`.

```text
dev = main + A + B
held = C
```

### Proposed

What the plan predicts after a requested operation.

```text
dev = main + A + B + C + D
```

---

## 11.2 Environment health

Expose state such as:

```rust
pub enum EnvironmentHealth {
    Realised,
    PartiallyRealised { held: Vec<String> },
    NeedsRebuild { changed_inputs: Vec<String> },
    NeverBuilt,
    LegacyUnknown,
    MissingBranch,
}
```

Avoid relying only on commit timestamps to detect staleness.

Prefer comparing pinned input SHAs from the last `EnvironmentBuildRecord` with current input SHAs.

This is both more accurate and easier to explain:

```text
Needs rebuild:
  feature/auth changed
  2a42d1c → 7c931af
```

rather than:

```text
new commit timestamp > rebuilt_at
```

Keep timestamps for presentation, not correctness.

---

# 12. Redesign `hitch status`

The default status view should answer:

> Which feature is where, and is that state actually realised?

Primary presentation:

```text
Hitch — repository status

Feature                    DEV               QA                MAIN
────────────────────────────────────────────────────────────────────────
feature/auth               ● Included        ● Included        ✓ Released
feature/payments           ● Included        —                 —
feature/new-dashboard      ⛔ Held            —                 —
bugfix/login-timeout       ● Included        ● Included        —

DEV   desired 4 · actual 3 · 1 held
QA    desired 2 · actual 2 · realised
```

### Semantics

Possible cells:

- `● Included`
- `⛔ Held`
- `↻ Needs rebuild`
- `? Actual unknown`
- `— Not desired`
- `✓ Released` / `✓ In base`
- `! Missing`

The exact glyphs may differ, but text must remain understandable without colour.

---

## 12.1 Status detail mode

Support an environment-oriented expansion:

```text
DEV

Desired
  dev = main + auth + payments + dashboard

Actual
  dev = main + auth + payments

Difference
  ⛔ dashboard held

Why
  dashboard conflicts with payments

Files
  src/payments/api.rs

Next
  hitch resolve dev --branch feature/dashboard
```

The old environment-by-environment detail can survive under a flag if useful, but the matrix should become the primary conceptual view.

---

# 13. Environment equations everywhere

Add a shared renderer for equations:

```text
dev = main + auth + payments + dashboard
```

Use it in:

- `status`;
- `tree`;
- plans;
- environment detail;
- desktop environment page;
- release preview;
- conflict explanations.

Do not hand-format it independently in every UI.

For held actual state:

```text
Desired
  dev = main + auth + payments + dashboard

Actual
  dev = main + auth + payments
              dashboard ⛔ held
```

---

# 14. Add `hitch why`

Add a read-only explanatory command.

## 14.1 Forms

```bash
hitch why feature/dashboard
hitch why feature/dashboard dev
hitch why dev
```

Optional future aliases are fine; keep the first implementation narrow and predictable.

---

## 14.2 `hitch why <branch> <environment>`

Example:

```text
feature/dashboard → dev

Desired
  Included

Actual
  ⛔ Held

Why?
  feature/dashboard conflicts with feature/payments

Files
  src/api/dashboard.rs
  src/api/types.rs

What Hitch did
  Excluded feature/dashboard from the current dev build
  Built dev from the remaining compatible branches

Next
  hitch resolve dev --branch feature/dashboard
```

---

## 14.3 `hitch why <branch>`

Example:

```text
feature/auth

dev     Included
qa      Included
main    Released

feature/auth is already contained in main.

It can be pruned from environments whose base is main.
```

---

## 14.4 `hitch why <environment>`

Example:

```text
dev

Desired
  main + auth + payments + dashboard

Actual
  main + auth + payments

dev is only partially realised.

dashboard is held because it conflicts with payments.

Next
  hitch resolve dev --branch dashboard
```

Implement this over the shared state model. Do not re-run bespoke conflict logic inside the command.

---

# 15. Conflict UX

Keep the underlying two-mode resolution model.

Improve the explanation around it.

## Base conflict

```text
dashboard conflicts with main

This conflict belongs to dashboard because main changed after dashboard branched.

Recommended durable fix:
  rebase dashboard onto main

Hitch can guide this:
  hitch resolve dev --branch dashboard
```

## Peer conflict

```text
dashboard conflicts with payments

Neither feature branch can cleanly own this compatibility fix.

Hitch can open an isolated composition session:
  hitch resolve dev --branch dashboard
```

Explain:

- why the branch was held;
- what remains deployed;
- whether a recorded resolution exists;
- whether replay was attempted;
- how the user can permanently retire the conflict debt.

Do not label peer holds merely as “merge failed”.

---

# 16. Release plan UX

Release deserves the most explicit plan.

Example:

```text
Release QA → main

Target
  main 31ac82e

QA desires
  ✓ feature/auth
  ✓ feature/search
  ✓ feature/payments

Merge order
  1. feature/auth
  2. feature/search
  3. feature/payments

Predicted target
  main 31ac82e → 905ab11

After release
  ↻ dev will rebuild because its base is main
  ↻ qa will rebuild because its base is main
  − integrated promotions will be pruned

Remote writes
  origin/main
  origin/dev
  origin/qa

Tag
  hitch-release-qa-to-main-...

Compatibility
  ✓ release is conflict-free

Apply this release? [y/N]
```

If the release cannot be composed:

```text
Release cannot be applied

feature/payments conflicts with the release target.

Nothing has been changed.
```

Preserve release's all-or-nothing behaviour.

---

# 17. Human terminology

Normal UX should prefer:

| Internal / advanced term | Normal user-facing wording |
|---|---|
| OID / SHA | commit / revision, show short SHA where useful |
| ref | branch / internal reference |
| CAS | “changed since this plan was calculated” |
| eject | held |
| materialisation | generated build |
| metadata mutation | desired-state change |
| composition | build / composition depending context |
| source SHA | feature revision |
| force-with-lease | safe remote replacement; exact mechanics in verbose mode |

Do not hide Git completely. The rule is:

> Default language explains meaning; verbose language explains mechanism.

---

# 18. Logging redesign

Keep two channels conceptually separate.

## Semantic output

Used by normal CLI and desktop:

```text
✓ payments included
⛔ dashboard held because it conflicts with payments
✓ dev published
⚠ remote push is still owed
```

## Diagnostic output

Used with `--verbose`:

```text
pinned refs/heads/payments at ...
merge_tree_compose ...
update-ref transaction ...
publish journal ...
force-with-lease ...
```

Do not make the desktop parse either channel to determine application state.

---

# 19. Activity / history model

The product should show a deployment story rather than only raw Git history.

Target:

```text
Today

14:32  Martin added payments to QA
       QA rebuilt successfully

14:07  CI rebuilt DEV
       dashboard was held
       conflicts with payments

13:51  Sarah updated dashboard
       DEV now needs rebuilding

12:26  Martin released authentication
       QA → main
       DEV and QA rebuilt
```

---

## 19.1 Structured events

Refactor `src/core/timeline.rs` toward structured events:

```rust
pub enum HitchEvent {
    EnvironmentCreated { ... },
    Promoted { ... },
    Demoted { ... },
    Rebuilt { ... },
    RebuiltWithHolds { ... },
    Released { ... },
    Locked { ... },
    Unlocked { ... },
    ApprovalRequested { ... },
    ApprovalGranted { ... },
    ApprovalRejected { ... },
}
```

Then render those events into prose.

Do not store prose as the domain model.

---

## 19.2 Event persistence

Use existing Git-native storage where possible.

Recommended staged approach:

### Phase A

Continue deriving human intent events from `hitch-metadata` history, but return typed events instead of strings.

Use build-state records for current rebuild/hold detail.

### Phase B

Add operation receipts to Hitch-owned Git metadata if exact historical rebuild outcomes are required.

If persistent operation receipts are introduced:

- do not put volatile derived state into `hitch.json`;
- do not create one top-level Git ref per event forever;
- design transport/concurrency before implementation;
- keep history inspectable without the desktop app;
- version the schema.

A compact append-only Git-native receipt chain is preferable to an external DB, but this phase should only ship after its remote concurrency behaviour is proven.

---

# 20. Desktop application target UX

The desktop app should become a **deployment workspace**, not mainly a Git-object inspector.

Primary view:

```text
┌──────────────────────────────────────────────────────────────────────┐
│ Hitch — my-project                                       ✓ Healthy │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│ FEATURE                     DEV              QA           MAIN       │
│                                                                      │
│ authentication              ● Included       ● Included    ✓         │
│ payments                    ● Included       —             —         │
│ new dashboard               ⛔ Held           —             —         │
│ login timeout               ● Included       ● Included    —         │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│ ⛔ new dashboard                                                    │
│                                                                      │
│ Desired in DEV, but not present in the current build.               │
│ Conflicts with payments in src/api/dashboard.ts                     │
│                                                                      │
│ [Explain]                                      [Resolve conflict]    │
└──────────────────────────────────────────────────────────────────────┘
```

---

# 21. Desktop feature × environment matrix

The matrix is the main workspace.

Rows:

- feature / bug branches relevant to Hitch;
- promoted branches first;
- optionally unpromoted branches in a separate section/filter.

Columns:

- Hitch environments;
- durable base/release state such as `main`.

Cell states should match the shared domain model.

Clicking a cell should explain its state.

---

# 22. Desktop environment membership controls

Selecting a feature should show:

```text
feature/payments

Environment membership

DEV       ● Included
QA        ○ Not included
```

Possible actions:

```text
[ Add to QA ]
[ Remove from DEV ]
```

These are still `promote` and `demote` semantically; the UI should express user intent rather than command names.

---

# 23. Desktop plan drawer/dialog

Before a mutation, show the structured plan.

Example:

```text
Add feature/payments to QA

Current
  qa = main + auth + search

Proposed
  qa = main + auth + search + payments

Compatibility
  ✓ clean

Will change
  desired QA membership
  generated qa branch
  origin/qa

Will not change
  dev
  main

                        Cancel     Add to QA
```

The **Apply** request must include the plan ID/fingerprint.

If stale:

```text
This plan is out of date

QA changed while you were reviewing the plan.

Changed:
  feature/search 93f4a1c → c8210fe

A fresh plan has been calculated.
```

Never auto-apply the fresh plan.

---

# 24. Desktop release flow

Use a dedicated release review screen rather than a generic yes/no dialog.

Show:

- source environment;
- target;
- feature merge order;
- target before/after;
- conflicts;
- dependent rebuilds;
- pruning;
- remote writes;
- release tag;
- approval/lock overrides if applicable.

A destructive or policy-bypassing option must be visible in the plan, not hidden in a backend `force: true`.

The current desktop implementation unconditionally sets `force: true` for release after UI confirmation. Replace that with an explicit structured override decision.

---

# 25. Desktop backend redesign

Current:

```text
React
  ↓ invoke
Tauri
  ↓ constructs command args
commands::<x>::run()
  ↓
human log stream
```

Target:

```text
React
  ↓
Tauri typed API
  ↓
shared Hitch planner/executor
  ↓
OperationPlan / ExecutionReceipt
```

Suggested Tauri APIs:

```text
repository_state(repo)
plan_operation(repo, intent)
apply_plan(repo, plan_id, fingerprint)
why(repo, subject)
activity(repo, filters)
```

Keep optional diagnostic log streaming for an expandable “details” panel.

Do not use streamed log lines as state.

---

# 26. Shared DTO/schema strategy

Avoid manually duplicating Rust and TypeScript models without checks.

At minimum:

- keep conversion code in one Tauri `types` module;
- add serialization tests for all public DTOs;
- check enum string values;
- version plan and receipt schemas.

If practical later, generate TypeScript definitions from Rust schemas, but do not make that a prerequisite for the first milestone.

---

# 27. Command migration strategy

Do not rewrite all commands simultaneously.

Each migrated command should follow:

```text
Args
 ↓
Intent
 ↓
Planner
 ↓
OperationPlan
 ↓ confirmation / dry-run
Executor
 ↓
ExecutionReceipt
 ↓
Renderer
```

The old command's integration tests should continue to pass or be intentionally updated with equivalent semantics.

---

# 28. Milestone plan

---

## Milestone 0 — Baseline, invariants, and golden scenarios

### Goal

Freeze current behaviour before refactoring.

### Tasks

- Read all files listed in section 4.
- Update `AGENTS.md` if any current documentation is stale before beginning.
- Add integration fixtures/scenarios covering:
  - clean promote + rebuild;
  - demote;
  - rebuild with held branch;
  - rebuild with `halt`;
  - recorded-resolution replay;
  - clean release;
  - failed release conflict;
  - dependent environment rebuild after release;
  - prune after release;
  - locked environment;
  - approval-required promotion;
  - `--no-push`;
  - interrupted publish recovery where practical.
- Record current exit codes.
- Record current remote/local ref effects.

### Deliverable

A regression test baseline proving the redesign does not accidentally alter Git semantics.

### Exit criteria

```text
just format
just format-check
just lint
just test
```

all green.

---

## Milestone 1 — Structured repository state

### Goal

Create a trustworthy read-only state model without changing mutation behaviour.

### Tasks

- Add `RepositoryStateSnapshot`.
- Add typed environment/feature state.
- Centralise desired-state calculation.
- Centralise branch-to-environment membership.
- Add typed health states.
- Preserve promotion ordering.
- Replace timestamp-only correctness checks where possible with SHA comparisons.
- Keep existing CLI output unchanged initially.

### Tests

- same branch promoted to multiple environments;
- remote-only feature;
- missing branch;
- feature already integrated into base;
- base changed after rebuild;
- feature changed after rebuild;
- locked/approval metadata represented accurately.

### Exit criteria

`status`, desktop workspace index, and future planners can all consume the same state model.

---

## Milestone 2 — Environment build provenance

### Goal

Make “Actual” state reliable.

### Tasks

- Define versioned `EnvironmentBuildRecord`.
- Add Git-native storage under a derived Hitch ref.
- Update rebuild publication to atomically associate environment tip and build record.
- Include:
  - base name/SHA;
  - desired ordered branches;
  - included branches;
  - held conflicts;
  - replayed resolutions;
  - result SHA;
  - metadata SHA;
  - timestamp/tool version.
- Teach state snapshot builder to read it.
- Handle legacy environments as `Actual unknown`.
- Ensure cleanup/GC logic keeps required state reachable.

### Tests

- record matches published branch;
- held branches recorded;
- replayed branch recorded as included;
- branch change makes state “needs rebuild”;
- corrupted/mismatched record becomes explicit degraded/unknown state;
- crash cannot publish env tip with a build record describing a different result.

### Exit criteria

Hitch can answer Desired vs Actual without guessing.

---

## Milestone 3 — Shared planning primitives

### Goal

Introduce `OperationPlan` without changing the visible UX yet.

### Tasks

- Add operation intent types.
- Add plan/fingerprint/effect/composition models.
- Implement planners for:
  1. rebuild;
  2. promote;
  3. demote;
  4. release.
- Reuse existing preflight/composition functions.
- Make planning side-effect-free except necessary fetch/synchronisation reads.
- Pin exact SHAs.
- Add plan digest/fingerprint validation.

### Critical rule

Do not duplicate merge/conflict calculations in the planner and executor if avoidable.

Extract a shared deterministic composition primitive that both can consume.

### Tests

For each operation:

```text
same inputs → same plan
changed input → different fingerprint
```

Test branch reorder, conflicts, missing refs, remote movement, metadata movement, and resolution replay.

### Exit criteria

All major operations can be represented completely before mutation.

---

## Milestone 4 — Executor + structured receipts

### Goal

Make actual execution consume validated plans and return structured results.

### Tasks

- Implement freshness validation.
- Introduce typed stale-plan error with changed refs.
- Move mutation orchestration behind executors.
- Return `ExecutionReceipt`.
- Preserve repo/environment locks.
- Preserve publish/recovery semantics.
- Preserve rebuild-with-holds exit status.
- Keep command modules thin.

### Tests

- plan then mutate metadata → apply refused;
- plan then move feature ref → apply refused;
- plan then move remote env ref → lease/staleness handled correctly;
- unchanged plan applies exactly predicted effects;
- receipt reflects actual published SHAs;
- push failure represented as warning/owed effect, not falsely “fully synced”.

### Exit criteria

No migrated CLI/desktop consumer needs to parse log strings to know what happened.

---

## Milestone 5 — Explainable CLI

### Goal

Expose plans and receipts to humans.

### Tasks

- Add shared CLI plan renderer.
- Add shared receipt renderer.
- Update:
  - promote;
  - demote;
  - rebuild;
  - release.
- Add consistent `--dry-run` to these commands.
- Keep `--yes` as confirmation bypass only.
- Keep `--verbose` as mechanism/debug detail.
- Add `--json` with versioned schema.
- Clearly display:
  - current;
  - proposed;
  - composition;
  - held branches;
  - side effects;
  - unaffected important resources;
  - dependent rebuilds;
  - pruning;
  - remote writes.

### UX acceptance

A user unfamiliar with Hitch internals should be able to read the plan and describe what will happen without reading the README.

### Exit criteria

The four core workflows no longer feel black-box in the CLI.

---

## Milestone 6 — Status matrix + Desired / Actual / Proposed vocabulary

### Goal

Make repository state understandable at a glance.

### Tasks

- Redesign default `hitch status`.
- Add feature × environment matrix.
- Show durable-base/released state.
- Show environment summary rows.
- Add per-environment detail.
- Use build provenance for actual state.
- Add explicit legacy/unknown state.
- Add shared environment-equation renderer.
- Refactor or retire duplicated old status calculations.
- Decide whether old detailed status moves to:
  - `hitch status --detailed`, or
  - a compatible secondary section.

### Tests

Golden-output tests for:

- clean repo;
- held branch;
- stale feature;
- changed base;
- missing feature;
- legacy no-build-record repo;
- released feature;
- multiple environments.

### Exit criteria

`hitch status` directly answers “what is where?” and “is desired state realised?”

---

## Milestone 7 — `hitch why`

### Goal

Make explanation a first-class command.

### Tasks

Implement:

```text
hitch why <branch>
hitch why <branch> <environment>
hitch why <environment>
```

Resolve ambiguity explicitly if a branch and environment share a name.

Return structured explanation model internally, then render.

Include:

- desired membership;
- actual membership;
- release/base integration;
- held reason;
- conflict partner/files;
- staleness reason;
- missing branch;
- suggested next action.

### Tests

Every major matrix cell state should have a useful `why` explanation.

### Exit criteria

A user should not need to inspect `hitch.json`, Git logs, or raw refs to understand a Hitch state.

---

## Milestone 8 — Structured activity and terminology cleanup

### Goal

Make Hitch tell the deployment story.

### Tasks

- Convert timeline domain model from preformatted summaries to typed events.
- Preserve current metadata-history derivation.
- Add semantic renderers.
- Include current rebuild/hold outcome where reliable.
- Standardise normal terminology:
  - Included
  - Held
  - Needs rebuild
  - Actual unknown
  - Released / In base
  - Desired
  - Actual
  - Proposed
- Audit normal CLI messages for unnecessarily internal Git terms.
- Keep detailed internals under `--verbose`.

### Exit criteria

CLI and desktop can consume the same activity event model.

---

## Milestone 9 — Desktop typed API migration

### Goal

Stop using command prose as the desktop application's behavioural API.

### Tasks

Add typed Tauri commands:

```text
repository_state
plan_operation
apply_plan
why
activity
```

- Return structured DTOs.
- Keep log streaming only as optional diagnostic detail.
- Implement stale-plan response.
- Remove direct dependency on `commands::<x>::run()` for migrated actions.
- Do not delete old bridges until the new UI is using the typed path.

### Tests

Rust DTO conversion/serialization tests.

Frontend type coverage/build:

```text
pnpm build
```

plus any desktop Rust tests.

### Exit criteria

Desktop state and confirmations come from typed core models.

---

## Milestone 10 — Desktop deployment workspace

### Goal

Replace the Git-object-first main screen with a Hitch-state-first workspace.

### Tasks

- Build feature × environment matrix.
- Add clear cell states.
- Add filtering/search.
- Add environment summary.
- Add feature detail.
- Add environment equations.
- Add Explain action backed by `why`.
- Preserve access to lower-level branch information in a secondary view.

### Interaction requirements

- no hidden hover-only essential state;
- colour is never the only signal;
- keyboard navigation for matrix;
- accessible labels for status glyphs;
- long branch names must remain usable;
- large repos need virtualisation or otherwise bounded rendering.

### Exit criteria

The primary desktop screen visually communicates Hitch's actual data model.

---

## Milestone 11 — Desktop mutation and release flows

### Goal

Make every important desktop mutation visibly planned.

### Tasks

- Environment membership actions:
  - Add to environment;
  - Remove from environment.
- Plan dialog/drawer before mutation.
- Stale-plan refresh flow.
- Dedicated release review screen.
- Explicit lock/approval override state.
- Rebuild action with plan.
- Held conflict card with:
  - reason;
  - files;
  - conflict class;
  - resolve action.
- Post-operation receipt view.
- Keep technical logs collapsible as “Details”.

### Critical change

Remove the current pattern where desktop release simply calls command code with `force: true`.

Any override must be represented in the user's reviewed intent and plan.

### Exit criteria

No high-impact desktop operation is an opaque button followed by a log stream.

---

## Milestone 12 — Remaining command migration

### Goal

Apply the same architecture consistently.

Migrate as appropriate:

- add/remove environment;
- lock/unlock;
- `set`;
- cleanup;
- approval apply/execute;
- setup-affecting flows where planning makes sense;
- push if its effects benefit from structured receipts.

Do not over-engineer simple read-only commands.

### Exit criteria

All significant mutations have structured plan/receipt semantics.

---

## Milestone 13 — Documentation, compatibility, and removal of legacy paths

### Goal

Finish the transition cleanly.

### Tasks

- Update README screenshots/diagrams/examples.
- Add `docs/architecture/explainable-operations.md`.
- Document JSON schema.
- Document Desired / Actual / Proposed.
- Document plan staleness.
- Update `SKILL.md`.
- Update `AGENTS.md`.
- Remove obsolete duplicate status logic.
- Remove desktop command-prose coupling.
- Remove compatibility adapters only after callers have migrated.
- Add changelog/migration notes.

### Exit criteria

There is one obvious architectural path for future Hitch operations.

---

# 29. Recommended implementation order

Do not start with the React redesign.

The dependency order should be:

```text
M0  Regression baseline
 ↓
M1  Structured repository state
 ↓
M2  Actual/build provenance
 ↓
M3  OperationPlan
 ↓
M4  Executor + receipt
 ↓
M5  Explainable CLI
 ↓
M6  Status matrix
 ↓
M7  why
 ↓
M8  Structured activity
 ↓
M9  Desktop typed API
 ↓
M10 Desktop workspace
 ↓
M11 Desktop plan/apply/release UX
 ↓
M12 Remaining mutations
 ↓
M13 Cleanup/docs
```

The desktop must not invent a second state model while waiting for backend work.

---

# 30. Testing strategy

## 30.1 Domain unit tests

Pure tests for:

- state classification;
- environment equations;
- matrix cell calculation;
- plan effects;
- plan fingerprint;
- `why` explanations;
- typed event derivation;
- JSON serialization.

---

## 30.2 Git differential/integration tests

Continue using real throwaway repositories.

Every planner/executor path should cover actual Git behaviour.

Particularly important:

- rename conflicts;
- delete/modify;
- branch already contained in base;
- branch reorder;
- annotated tags if accepted as branch-ish inputs;
- concurrent ref movement;
- remote movement;
- shallow clone behaviour;
- dirty checkout protection;
- recorded resolution lineage;
- publish journal recovery.

Do not replace existing Git differential tests with mocks.

---

## 30.3 Plan-vs-apply invariant tests

This redesign needs a new class of tests:

```text
plan predicts X
execute plan
receipt reports X
repository now equals X
```

For every important effect:

- metadata;
- environment ref;
- remote update;
- holds;
- release target;
- tag;
- dependent rebuild;
- pruning.

---

## 30.4 Golden UX tests

Snapshot/golden tests are appropriate for semantic renderers.

Keep the domain model independent from terminal width/colour where possible.

Test:

- colour disabled;
- narrow terminal;
- long branch names;
- no environments;
- many environments;
- many features.

---

## 30.5 Desktop

At minimum:

```text
pnpm build
```

must remain green.

Add component/unit testing infrastructure if the matrix and plan flows become complex enough to warrant it.

Do not rely solely on visual manual testing.

---

# 31. Performance constraints

Planning must not make every status command unnecessarily fetch the network.

Separate:

```text
Local snapshot
Fresh synchronized plan
```

`hitch status` should remain fast and mostly local.

Mutating plans may synchronize required refs because correctness matters.

The UI should label stale/local information honestly.

For very large branch sets:

- avoid O(environments × branches × repeated Git subprocesses) where a map/index can be built once;
- pin refs in batches where practical;
- cache immutable calculations inside one snapshot;
- never cache across operations without a validation key.

---

# 32. Concurrency rules

The redesign must preserve current concurrency discipline.

### CLI plan + apply

May hold repo lock across the immediate confirmation if that remains the simplest safe model.

### Desktop plan

Must **not** hold the repository lock while a human reads the plan.

Instead:

```text
plan → release lock
user reviews
apply → reacquire lock → validate fingerprint
```

If stale:

```text
do not mutate
return changed inputs
re-plan
require another explicit Apply
```

### Remote races

Continue relying on appropriate remote leases/CAS behaviour.

A locally fresh plan can still lose a remote race; the receipt/error should explain this as:

```text
The remote changed after the plan was calculated.
Nothing was overwritten.
Refresh and try again.
```

---

# 33. Error design

Errors should be structured where possible.

Examples:

```rust
pub enum PlanApplyError {
    StalePlan { changed: Vec<ChangedInput> },
    PolicyBlocked { reason: PolicyBlock },
    Conflict { conflict: ConflictExplanation },
    PublishRace { branch: String },
    RemotePushFailed { branch: String, remedy: String },
}
```

Render human guidance at the edge.

Do not use string matching in the desktop to infer error type.

---

# 34. Backward compatibility

## `hitch.json`

Avoid a schema change for this redesign unless absolutely necessary.

New derived build provenance belongs under Hitch-owned refs.

If a config field must be added:

- use Serde defaults;
- update compatibility checks;
- update `set` if user-configurable;
- test old config deserialization.

## Environment branches

Existing repositories should upgrade in place.

Until each environment is rebuilt under the new version:

```text
Actual state: unknown (legacy build)
```

A rebuild establishes exact provenance.

## Scripts

Preserve existing command names and exit-code semantics where feasible.

Document intentional human-output changes.

Machine users should be directed to `--json`.

---

# 35. Security and trust guidance

Do not weaken any existing trust boundary while making things friendlier.

Particular cautions:

- `hitch-metadata` is writable by users with repository write access;
- approval identity is not a cryptographic security boundary;
- deploy-key/ruleset semantics must remain accurately described;
- shared resolution signature/lineage checks must not be bypassed by planning;
- plans from external/untrusted JSON must never be blindly executed.

`apply_plan` should apply a server/core-generated plan identity/fingerprint, not arbitrary caller-authored ref edits.

---

# 36. Agent implementation guidance

## 36.1 Follow repository instructions first

At the beginning of every milestone:

1. read `AGENTS.md`;
2. inspect the current implementation, not just this spec;
3. verify named paths/functions still exist;
4. adapt the implementation plan to drift;
5. update `AGENTS.md` in the same change when architecture or gotchas change.

---

## 36.2 Small commits, one invariant at a time

Good sequence:

```text
add model + tests
add adapter from old code
switch one caller
remove duplicate path
```

Bad sequence:

```text
rewrite CLI + prelude + desktop + types in one commit
```

---

## 36.3 Do not “clean up” Git safety code casually

`prelude.rs`, `git_operations.rs`, `publish_journal.rs`, and resolution replay contain hard-earned edge-case handling.

When extracting planners/executors:

- preserve pinned-input semantics;
- preserve exact merge engine;
- preserve CAS;
- preserve journal ordering;
- preserve checkout safety;
- preserve remote lease behaviour;
- preserve error recovery.

Refactor around those primitives rather than reimplementing them.

---

## 36.4 No dual sources of truth

If both old and new code can answer:

```text
what branches are included?
what is held?
what will release touch?
```

for an extended period, they will diverge.

Migration adapters are acceptable temporarily, but every milestone should identify which old calculation becomes obsolete.

---

## 36.5 Avoid stringly typed state

Do not encode state as:

```text
"held"
"up to date"
"released"
```

inside core logic.

Use enums/structs.

Rendering owns wording.

---

## 36.6 Keep output separate from behaviour

Core operations should not need a console to function.

Desired:

```rust
let plan = planner.plan(intent)?;
let receipt = executor.apply(plan)?;
```

Then:

```rust
cli_renderer.render_plan(&plan);
desktop_serializes(plan);
```

Logging remains supplemental.

---

## 36.7 Preserve deterministic ordering

Hitch composition order matters.

Never sort promoted branch lists merely for prettier UI.

Sort only presentation collections whose ordering is semantically irrelevant.

The matrix may sort feature rows alphabetically for display, but each environment's equation/plan must retain configured promotion order.

---

## 36.8 Test failure paths first-class

For each happy-path test, ask:

```text
What if metadata moves?
What if the feature moves?
What if origin moves?
What if the branch disappears?
What if a conflict appears?
What if push fails after local publish?
What if the process dies here?
```

The new UX is only trustworthy if its explanations remain true under failure.

---

# 37. UX copy guidance

Prefer:

```text
dashboard is held because it conflicts with payments
```

over:

```text
merge-tree failed
```

Prefer:

```text
This plan is out of date because feature/search changed.
```

over:

```text
CAS mismatch on refs/heads/feature/search
```

Prefer:

```text
The local release succeeded, but origin/main was not updated.
```

over:

```text
push error
```

Show the technical equivalent under verbose/details.

---

# 38. Accessibility / visual guidance for desktop

The desktop redesign should not rely on coloured dots alone.

Every state requires:

- icon/glyph;
- text or accessible label;
- colour as enhancement only.

Examples:

```text
● Included
⛔ Held
↻ Needs rebuild
? Unknown
✓ Released
```

Use tooltips for compact matrix cells, but essential meaning must be available without hover.

Support:

- keyboard focus;
- screen-reader labels;
- high contrast;
- reduced-motion preference;
- wide and narrow window layouts.

---

# 39. Proposed future architecture diagram

```text
                         ┌──────────────────────┐
                         │    Git repository    │
                         └──────────┬───────────┘
                                    │
                          read / pin / validate
                                    │
                         ┌──────────▼───────────┐
                         │ Repository Snapshot  │
                         │ desired + actual     │
                         └──────────┬───────────┘
                                    │
                              user intent
                                    │
                         ┌──────────▼───────────┐
                         │       Planner        │
                         └──────────┬───────────┘
                                    │
                         ┌──────────▼───────────┐
                         │    OperationPlan     │
                         │ current / proposed   │
                         │ effects / warnings   │
                         │ fingerprint          │
                         └───────┬────┬────┬────┘
                                 │    │    │
                             CLI │    │    │ JSON
                                 │ Desktop
                                 │
                             confirmation
                                 │
                         ┌───────▼──────────────┐
                         │ Validate fingerprint │
                         └───────┬──────────────┘
                                 │
                         ┌───────▼──────────────┐
                         │      Executor        │
                         │ existing Git safety  │
                         └───────┬──────────────┘
                                 │
                         ┌───────▼──────────────┐
                         │  ExecutionReceipt   │
                         └───────┬──────────────┘
                                 │
                         render / store / show
```

---

# 40. Definition of done for the redesign

The redesign is complete when all of the following are true:

- [ ] Hitch has one shared structured repository-state model.
- [ ] Desired, Actual, and Proposed have precise meanings in code.
- [ ] Environment builds persist trustworthy provenance.
- [ ] Promote, demote, rebuild, and release have shared planners.
- [ ] Those operations execute validated plans.
- [ ] They return structured receipts.
- [ ] CLI plans explain current → proposed state before mutation.
- [ ] `--dry-run` uses the same planning path.
- [ ] `--json` exposes versioned structured plans/results.
- [ ] `hitch status` provides a feature × environment view.
- [ ] Environment equations are reused throughout the UX.
- [ ] `hitch why` explains branch/environment state.
- [ ] Holds explain conflict partner, files, and next action.
- [ ] Release previews dependent rebuilds, pruning, tags, and remote writes.
- [ ] Normal output explains behaviour; `--verbose` explains mechanism.
- [ ] Activity uses structured events.
- [ ] Desktop reads typed state instead of parsing command output.
- [ ] Desktop uses a deployment-workspace matrix as its primary view.
- [ ] Desktop mutations display a real core-generated plan before Apply.
- [ ] Desktop rejects stale plans and requires review of the replacement.
- [ ] Desktop release no longer hides `force: true` behind a generic confirmation.
- [ ] All important mutations eventually use the same plan/apply/receipt architecture.
- [ ] Existing Git safety, recovery, conflict, and release semantics remain intact.
- [ ] `AGENTS.md`, `SKILL.md`, README, and architecture docs match the final design.
- [ ] `just format`, `just format-check`, `just lint`, and `just test` pass.
- [ ] Desktop production build passes with `pnpm build`.

---

# 41. Product test

Before calling the project finished, give Hitch to someone who understands normal Git but has not studied Hitch internals.

Ask them to perform:

1. add a feature to dev;
2. understand why it is absent from QA;
3. diagnose a held feature;
4. determine whether dev matches desired state;
5. preview a release;
6. explain which branches will change during that release.

They should be able to answer those questions **from Hitch itself**, without:

- opening `hitch.json`;
- inspecting raw refs;
- reading Git commit graphs;
- reading the README to decode command output.

That is the real acceptance test.

---

# 42. Final architectural rule

For every significant Hitch operation:

```text
SHOW THE STATE
      ↓
SHOW THE CHANGE
      ↓
SHOW THE CONSEQUENCES
      ↓
APPLY EXACTLY THAT
      ↓
SHOW WHAT ACTUALLY HAPPENED
```

If the implementation cannot do that reliably, the operation is not yet ready for the new UX.
