# P7 — Status matrix, `hitch why`, and the shared environment equation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `hitch status` answers "which feature is where, and is that state actually realised" as a feature×environment matrix, and a new read-only `hitch why` makes every cell state explainable — both rendered by the one module allowed to choose words, and both over the shared state model with no bespoke conflict logic.

**Architecture:** Three pieces, in this order. (1) `src/core/status.rs` gains the matrix as a *pure projection of the snapshot*, with a total `MatrixCell::classify` — one cell verdict, decided in one function, exhaustive over the enum cross-product so a new `ActualMembership` variant fails a test rather than silently falling through a wildcard. (2) `src/core/why.rs` builds a structured `WhyExplanation` from the same snapshot; the command supplies one existence fact and nothing else, because an existence check is a fact and a verdict is not. (3) `src/core/render.rs` gains the shared environment-equation renderer (§13) and the matrix and `why` renderers, and the plan's `Current`/`Proposed` lines are rewired onto the equation so plans and status speak with one voice. The old per-environment view is not deleted, only moved behind `--environments`.

**Tech Stack:** Rust, `serde`, `serde_json`, `clap`, `anyhow`, `colored`. **No new dependencies.**

**Spec section:** §11 (Desired/Actual/Proposed), §11.2 (health), §12 (status matrix), §12.1 (status detail), §13 (environment equations everywhere), §14 (`hitch why`), §14.1–§14.4, §17 (human terminology), §18 (semantic vs diagnostic channels), §10.3 (`--json`).

---

## Global Constraints

1. **`src/core/render.rs` remains the only place allowed to choose words.** P7
   *adds to* it — `render_equation`, `render_matrix`, `render_why` — and adds
   no sibling vocabulary. A cell label, an environment equation, and a `why`
   line are all words, and §17's terminology table is unenforceable by a
   convention.
2. **A display reads the verdict; it never derives one.** The only new verdict
   is `MatrixCell::classify`, and it is `pub` so tests can call it directly.
   Nothing else may `match` on `ActualMembership` or `EnvironmentHealth` to
   decide a *cell*; reading a field to *render* a cell is what
   `render.rs` does.
3. **Do not reintroduce a timestamp comparison anywhere.** The matrix's
   staleness story is entirely `EnvironmentHealth::NeedsRebuild`'s
   `changed_inputs` / `added` / `removed`, which are SHA comparisons. The only
   production caller of `get_commit_timestamp` stays
   `src/core/timeline.rs:96`, which formats a date for display.
4. **`build_state_snapshot` stays offline and is read exactly once per
   command.** No new `ls-remote`, no `fetch`, no `branch_exists_anywhere` in a
   read-only path. `status` and `why` take one snapshot and hand it to every
   renderer; a second read is a second opinion.
5. **An `ActualMembership::Unknown` is never rendered as a fact.** It becomes
   "needs rebuild" when the environment has a trustworthy record (the branch
   was promoted after the build) and "actual unknown" when it does not. That
   distinction is real — it changes what the user's next action is — and it must
   be *read* from `ActualComposition`, not guessed from the absence of a field.
6. **A fact and a prediction must not share a glyph, a word, or a code path.**
   The matrix is **facts only**. `preflight_compatibility_report_local` keeps
   its two existing call sites (`status --environments`, `tree`) and gains
   none; `hitch why` never calls it. A `why` that answered "why is this held"
   with a *prediction* would be answering a different question than the one
   asked.
7. **Declaration order is composition order and is never sorted.** Matrix
   *rows* come from `snapshot.features` (already `BTreeMap`-ordered, so
   alphabetical by construction) and *columns* from `snapshot.environments`
   (already `sort_by(name)`), so determinism is inherited rather than
   re-imposed. A new renderer that alphabetises `DesiredComposition::branches`
   is rendering a different build.
8. **Colour is decoration.** Every cell and every `why` line must be readable
   with colour disabled, so the glyph *and the word* both carry the state, and
   the golden tests assert words. §12 says so and it is also what makes the
   piped-output case honest.
9. **`status` and `why` are read-only.** `command_is_mutating`'s `false` arm,
   so no repo lock, no `recover` pass, no ref writes, no fetch. Under `--json`
   both reserve stdout entirely and route everything else to stderr, exactly as
   the four mutating commands do.
10. **The old per-environment view is moved, not rewritten.** It keeps its
    wording and its test-asserted lines; `--environments` only changes *when*
    it renders. A P7 diff that rewords `display_environment_status` is a
    P7 diff that cannot be reviewed.
11. **`crates/hitch-desktop` is untouched**, and `main` stays at `5d81fb2`.

## Deviations from the spec

1. **The `✓ Released` cell collapses to `InBase`.** Two reasons, both about
   truth rather than taste. A release *prunes* the released branches from every
   environment based on the released target (`plan_prunes` in
   `src/operations/release.rs`), so a genuinely released feature stops being a
   declared feature and would not appear in a matrix built from declarations at
   all — the spec's own `✓ Released` cell is close to unreachable by
   construction. And the only offline fact available is "this branch's tip is
   reachable from this environment's base", which is
   `ActualMembership::AlreadyInBase`; rendering that as "released" would assert
   a release hitch did not necessarily perform. The story of where a branch
   *went* is an activity-history question and belongs to P9.
2. **`--environments`, not `--detailed` or `--all`.** §12.1 calls the
   Desired/Actual/Difference expansion "status detail mode", and that shape is
   `hitch why <env>` — so `--detailed` would name the wrong view twice over.
   `--environments` names what the flag actually renders, and takes an optional
   environment name to scope it.
3. **A read-only `--json` envelope is `{"schema_version", "<view>"}`, not
   `{plan, receipt}`.** P6's two-key envelope exists because a mutation has two
   halves — before and after. A read-only view has one, and forcing it into a
   two-key shape would mean either a `null` receipt (which says "nothing
   happened", true and useless) or a second envelope shape anyway. So
   `hitch --json status` emits `{"schema_version": 1, "status": …}` and
   `hitch why --json` emits `{"schema_version": 1, "why": …}`. Both keep the
   `schema_version` discipline and both are pinned by key-set tests.
4. **`hitch why <branch>` answers about a branch hitch knows nothing about.**
   A local branch promoted nowhere is a real and common state, and the honest
   answer ("not promoted to any environment") is more useful than an error. The
   only thing the command contributes beyond the snapshot is whether the name
   resolves to *any* ref — a fact, read with `rev_parse_opt`, needed so that
   `hitch why typo-name` is an error rather than a confident non-answer.
5. **Every enum in the read-only `--json` envelope is `snake_case`.** P6's
   mutation envelope kept Rust's `PascalCase` variant names, which are stable but
   unidiomatic on the wire and inconsistent with the `snake_case` field names
   beside them. P7's two envelopes are `snake_case` throughout, and a collector
   in `tests/integration/why_tests.rs` walks the whole document and asserts that
   **no** PascalCase token survives anywhere in it — so a *new* enum added
   without the rename fails a test nobody had to remember.
6. **No `terminal_size` dependency; the width budget is `COLUMNS` alone.** §12
   requires the matrix to degrade on a narrow terminal and the obvious reading is
   to ask the tty. hitch already reads `COLUMNS` for its table widths, and a new
   dependency is a build-config surface for one number the shell already exports.
   Unset, unparseable, or `0` means **unbounded** — the safe direction, because a
   width guessed too small truncates and a width guessed too large only wraps.
7. **The narrow-terminal fallback is prose, not truncation.** When a row cannot
   fit, the matrix switches to a per-branch prose list rather than clipping the
   right-hand columns. Clipping would drop the environment name — the one column
   that says which cell is which — and a cell without its column header is a cell
   nobody can read, which is the one thing §12's "understandable without colour,
   in a pipe" forbids.
8. **Ambiguity is an error, and the error names both readings.** §14.1 says to
   "resolve ambiguity explicitly" without saying how. A branch and an environment
   sharing a name is not resolvable from the snapshot alone, so `hitch why`
   refuses and prints the two commands that would each answer one reading —
   `hitch why <target> <environment>` for the branch, and
   `hitch status --environments <target>` for the environment. Guessing would be
   indistinguishable, to the reader, from the other reading being right.

---

### Task 1 — The matrix as a pure projection of the snapshot

**Files:** `src/core/status.rs`

**Interfaces:**
- Consumes: `RepositoryStateSnapshot`, `ActualMembership`, `EnvironmentHealth`,
  `ActualComposition` (all from `src/core/state.rs`). No `GlobalContext`, no
  `Result`, no git.
- Produces: `MatrixCell` (+ `MatrixCell::classify`, `MatrixCell::glyph`,
  `MatrixCell::label`, `MatrixCell::is_actionable`), `MatrixRow`,
  `MatrixModel`, `EnvironmentSummaryRow`, `build_matrix_model(&RepositoryStateSnapshot) -> MatrixModel`.

- [x] `MatrixCell` with the seven states §12 names, mapping onto what
  `ActualMembership` and `EnvironmentHealth` can actually distinguish:
  `NotDesired`, `Included`, `Held`, `InBase`, `NeedsRebuild`, `ActualUnknown`,
  `Missing`.

- [x] `classify(desired: bool, actual: ActualMembership, has_record: bool) -> MatrixCell`,
  documented as an **order of authority**, in this order:
  1. `!desired` → `NotDesired`. The environment does not declare it; nothing
     else is a question about this cell.
  2. `Missing` → `Missing`. A declared branch with no ref anywhere. Nothing
     else can be true of it — this mirrors `membership_within`'s step 2
     (`src/core/state.rs:454`).
  3. `Held` → `Held`. The record says the last build excluded it, with a named
     partner.
  4. `AlreadyInBase` → `InBase`.
  5. `Included` → `Included`.
  6. `Unknown` + record → `NeedsRebuild`. The branch is declared, a build
     exists, and that build does not mention it — which is exactly a promotion
     that has not been built yet.
  7. `Unknown` + no record → `ActualUnknown`. Deliberately *not* the same
     cell as 6: the next action differs (`hitch rebuild` vs "nothing to fix,
     the last publisher did not record"), and conflating them is how
     `LegacyUnknown` got treated as "probably fine" in the first place.
- [x] `has_record` is a *parameter*, not something `classify` re-derives from
  the `ActualComposition` variant. The caller reads
  `ActualComposition::FromRecord(_)` and passes a bool, so the classifier stays
  a two-input function and its table is a table.

- [x] `build_matrix_model` projects the snapshot: columns from
  `snapshot.environments` (name order, inherited), rows from
  `snapshot.features` (alphabetical, inherited), one cell per
  `(feature, environment)` pair. An environment that does not declare a feature
  gets `NotDesired`, materialised rather than left absent — a matrix is a grid,
  and a hole in a grid reads as "not shown" rather than "not desired".
  This is the one place where P3's "absent means not desired" convention is
  deliberately inverted, and the reason is a table, not a list.
- [x] `EnvironmentSummaryRow` per environment: `desired` count, `actual` count
  (included + in-base), `held` count, `needs_rebuild` count, and the
  environment's `health` carried whole. Counts come from the *cells*, not from
  a second pass over the declaration — one place decides, so the summary cannot
  disagree with the grid above it.
- [x] Unit tests, all in-module:
  - `classify` over the **full** `ActualMembership` × `desired` × `has_record`
    cross-product, asserting the exact cell for each — a new variant added to
    `ActualMembership` without updating this test is a compile-or-test failure,
    never a silent `NotDesired`.
  - `classify` is total: no wildcard arm; a `match` over `ActualMembership` in
    the implementation makes adding a variant a compile error.
  - `build_matrix_model` on a hand-built snapshot: the seven §12 states appear
    as the seven expected cells; a feature declared nowhere yields `NotDesired`
    in every column; a feature declared in one environment and absent from
    another yields the grid, not a ragged list.
  - The summary row's counts equal a count of the grid's cells (a test that
    recomputes them from the rows, so it fails if the two code paths drift).

### Task 2 — The shared environment equation, and the matrix renderer

**Files:** `src/core/render.rs`

**Interfaces:**
- Consumes: `crate::core::status::{MatrixCell, MatrixModel, EnvironmentSummaryRow}`.
- Produces: `EnvironmentEquation`, `EquationTerm`, `ExcludedTerm`, `render_equation(&EnvironmentEquation) -> String`, `render_matrix(&MatrixModel) -> String`, `render_environment_summaries(&[EnvironmentSummaryRow]) -> String`, and a `matrix_cell_label`/`matrix_cell_glyph` pair that live *with* `MatrixCell` (impl block in `render.rs` is not possible across modules for a foreign type — so `glyph`/`label` are `pub fn` on `MatrixCell` in `core/status.rs` and this task only renders).

- [x] `EnvironmentEquation { environment, base, terms: Vec<EquationTerm>, excluded: Vec<ExcludedTerm> }`,
  where a term carries a branch and one of `Plain` / `InBase`, and an excluded
  term carries a branch and a reason (`Held` with a conflict partner, or
  `Unknown`). This is the minimum a renderer can need, which is what makes one
  function serve plans, status, `why`, and (Task 4) `tree`.
- [x] `render_equation` produces exactly §13's form —
  `dev = main + auth + payments` — with excluded terms indented underneath as
  `dashboard ⛔ held`, and a `(base only — no promoted branches)` line when
  there are no terms. The latter is copied from `render_plan`'s existing
  handling (`src/core/render.rs:68`) because a build from a base alone is a
  real build, and rendering it as nothing-to-do is how it was nearly worded.
- [x] **Rewire `describe_projection` (`src/core/render.rs:311`) onto
  `render_equation`.** This is the point of §13: the plan's `Current` and
  `Proposed` lines and the status matrix's equations must be the same
  characters. `describe_projection` becomes a two-line adapter that builds an
  `EnvironmentEquation` with every term `Plain` and no exclusions — a plan
  already lists held branches in its `Composition` section with a remedy, so
  the equation must not *also* list them or the same fact appears twice with
  two vocabularies.
- [x] `render_matrix` renders the table: a header row of environment names, a
  rule, one row per feature. Column widths are computed from the content, the
  feature-name column is left-aligned and the cell columns centred under their
  header. No trailing whitespace, no colour-dependent padding.
- [x] `render_environment_summaries` renders the per-environment lines beneath
  the grid, using the spec's own compact form:
  `DEV   desired 4 · actual 3 · 1 held` — with a variant per health, all
  worded by `EnvironmentHealth::label()` (`src/core/state.rs:307`) rather than
  a second vocabulary of health words.
- [x] Unit tests in `render.rs`:
  - `render_equation` for zero terms, one term, many terms, an excluded held
    term with a partner, and an excluded unknown term.
  - `the_plan_and_the_status_equation_are_the_same_characters` — build an
    `EnvironmentProjection` and the equivalent `EnvironmentEquation`, assert
    both renderers produce the same string. This is the §13 regression test and
    it is deliberately a *string* comparison, not a "both contain the branch"
    assertion.
  - `render_matrix` for the seven states, with the glyph **and** the word
    present, and with colour disabled (`NO_COLOR=1` is already global in tests
    via `colored::control::set_override(false)`; assert on the plain string).
  - The table is rectangular: every rendered row has the same number of
    whitespace-separated fields, asserted for a snapshot with a long branch
    name and a short one in the same table (the long-name case is where a
    naive `{:width$}` renderer silently shifts columns).

### Task 3 — `hitch status`: matrix by default, `--environments`, `--json`

**Files:** `src/commands/status.rs`, `src/cli.rs` (doc comment only)

**Interfaces:**
- Consumes: `build_matrix_model`, `render_matrix`, `render_environment_summaries`,
  `emit_json` + `JSON_SCHEMA_VERSION` from `src/core/render.rs`.
- Produces: `StatusCommand { verbose, diff, environments: Option<String>, json_handled_globally }`.
  `--json` is a *global* flag read from `context`, not a per-command arg, so
  `StatusCommand` does not gain a `json` field — the same shape the four
  mutating commands already have.

- [x] Default `run` builds one snapshot, then:
  1. the headline (repository status + current branch),
  2. `render_matrix`,
  3. `render_environment_summaries`,
  4. the suggested-actions block, **moved up from `display_status_summary`**
     so it sits directly under the grid where the pending work is visible. It
     is computed from `EnvironmentHealth` variants via the same `match` as
     today (`src/commands/status.rs:157-178`) — no new verdict.
  5. the quick-commands footer, unchanged.
- [x] The per-environment counters line `📊 N environments: …`
  (`src/commands/status.rs:123`) is **deleted**, not moved: the summary rows
  now carry the same facts per environment with more precision, and two
  summary lines whose numbers come from two different passes is the drift P3
  was built to kill. `display_overall_summary`'s remaining job is the headline
  and the current-branch line.
- [x] `--environments [NAME]` renders the existing `display_environment_status`
  (`src/commands/status.rs:200`) with **no changes to its wording**. With a
  `NAME`, only that environment; without, all of them, in name order. A `NAME`
  that is not a declared environment is an error naming the known environments.
- [x] `--environments` and `--json` together: the JSON document wins and the
  prose is not printed, with the per-environment view available in the
  document under its own key. No gate, no prompt — a read-only command has
  nothing to confirm, so `decide_gate` is not involved.
- [x] `status --json` emits `{"schema_version", "status"}` where the value is
  the serialised `StatusDocument` — a new small struct carrying the matrix
  model, the summary rows, and the snapshot's `captured_at`. The document is
  built from the *same* model the prose renders, so the two cannot disagree.
- [x] `src/cli.rs`: `--json`'s doc comment now names **five** commands
  (`rebuild`, `promote`, `demote`, `release`, `status`) and drops the "status
  gains it alongside the status matrix" forward-reference, replaced by a
  sentence about the read-only envelope shape. This is the first time the
  four-command list in `AGENTS.md` changes, so it changes here and in the same
  commit.
- [x] `StatusCommand`'s `--verbose` behaviour is unchanged; `--diff` still runs
  after the view, and still prints to stdout (it is a `git diff` of
  `hitch.json`, not part of the state view).

### Task 4 — `tree` shares the equation

**Files:** `src/commands/tree.rs`

**Interfaces:**
- Consumes: `EnvironmentEquation`, `render_equation` from `src/core/render.rs`.
- Produces: no new public interface; one changed line per environment node.

- [x] The environment node's `(base: main, 3 promoted)` parenthetical is
  replaced by the shared equation when the environment has promoted branches,
  and by `(base only)` when it does not. §13 lists `tree` as a place the
  equation must appear, and this is the only composition `tree` ever shows.
- [x] The pre-existing `⛔ … (conflicts with X — held on rebuild)` suffix stays
  exactly as it is. It is a **prediction** (`preflight_compatibility_report_local`)
  and the equation is a **fact** (the declaration), so the two must not be
  merged into one cell — Global Constraint 6 is the reason.
- [x] Re-point `tests/integration/tree_tests.rs` (12 tests) for the changed
  node line. The `[env]`, the `[LOCKED]` marker, the `[base]` prefix and every
  conflict suffix are asserted unchanged, so a re-point is a one-line diff per
  test and any test needing more than that is a real behaviour change.

### Task 5 — `hitch why`'s explanation model

**Files:** `src/core/why.rs` (new), `src/core/mod.rs`

**Interfaces:**
- Consumes: `RepositoryStateSnapshot` (P3) and nothing else. **No git, no
  `GlobalContext`, no `Result`** except for the ambiguity error, which is a
  caller-facing judgement about *the subject*, not about the repository.
- Produces: `WhySubject` (`Feature(String)` / `FeatureIn(String, String)` /
  `Environment(String)`), `WhyExplanation` (an enum of three forms),
  `WhyMembership`, `WhyReason`, `NextAction`, `build_why(&RepositoryStateSnapshot, WhySubject) -> Result<WhyExplanation>`.

- [x] `WhySubject` is resolved by the *caller* (Task 7), which has the
  `GlobalContext`; `build_why` takes an already-resolved subject. This keeps
  the one git call in the command where it can be logged and where a
  `GlobalContext` is available, and it means the model builder is testable from
  a hand-built snapshot with no repository at all.
- [x] `WhyMembership` is the vocabulary §14 needs and `ActualMembership` is
  not: it merges the environment's `has_record` into the answer, because "this
  branch is in the build" and "this branch's standing in the build is unknown"
  are the two things a `why` must distinguish and `ActualMembership::Unknown`
  deliberately conflates them. Variants: `NotDesired`, `Included`, `Held`,
  `InBase`, `NeedsRebuild`, `ActualUnknown`, `Missing`. `build_why` reuses
  `MatrixCell::classify` for the membership, so the matrix and `why` cannot
  disagree about what a cell means — one classifier, two views.
- [x] The `FeatureInEnvironment` form carries: subject names, the equation for
  that environment (as an `EnvironmentEquation`, so §13 holds), `desired` and
  `actual` memberships, an optional `WhyReason`, an optional `NextAction`, and
  `what_hitch_did: Vec<String>` read from the record — §14.2's "What Hitch did"
  is the record's own `included`/`held`/`replayed_resolutions`, restated as
  sentences by the *renderer*, never recomputed.
- [x] `WhyReason` variants, each a fact read from the snapshot:
  `HeldAgainst { conflicts_with, files }` (from `RecordActual::held`),
  `ChangedSinceBuild { from, to }` (from `EnvironmentHealth::NeedsRebuild`'s
  `changed_inputs`, using `ChangedInput::short()` at
  `src/core/state.rs:246` for the `2a42d1c → 7c931af` form),
  `PromotedSinceBuild` (`added`), `DemotedSinceBuild` (`removed`),
  `NoRef` (`Missing`), `NoBuildRecord` (`LegacyUnknown`),
  `EnvironmentBranchMissing` (`MissingBranch`).
- [x] `NextAction` is a **closed enum with a renderer-chosen command**, not a
  free string built by the model: `Rebuild(env)`, `Resolve(env, branch)`,
  `Demote(env, branch)`, `Promote(branch, env)`, `None`. The model decides
  *which* action; `render.rs` decides the *words*, because the words are
  display. A model that emitted `"hitch rebuild dev"` would make the next
  action untestable without string matching.
- [x] `NextAction::Resolve` is emitted **only** for a `Held` branch, because
  `hitch resolve` is the only command that operates on a held branch
  (`src/commands/resolve.rs`). A `NeedsRebuild` branch gets `Rebuild`; a
  `Missing` branch gets `None` with a reason, because `hitch resolve` would
  fail on a branch that does not resolve and `hitch rebuild` would hold it
  again.
- [x] The `Environment` form carries both equations (Desired from
  `EnvironmentState::desired`, Actual from `RecordActual`) and the
  environment's `health` whole.
- [x] The `Feature` form carries the per-environment cells (reusing
  `classify`) plus a `summary: Option<String>` for §14.3's closing prose —
  `feature/auth is already contained in main.` The *fact* is
  `InBase`; the *sentence* is the renderer's, per Constraint 1.
- [x] Ambiguity: `build_why` returns an error naming **both** readings and both
  commands when the subject is simultaneously an environment and a declared
  feature. §Milestone 7 says "resolve ambiguity explicitly"; a guess would be
  the opposite of explicit. `WhySubject::resolve` (the caller-side helper in
  Task 7) is what detects it, and its test is the one that pins the message.
- [x] Unit tests, all from hand-built snapshots, no repository:
  - each of the three forms, in its plain shape;
  - a held branch produces `HeldAgainst` with the partner and the file list, and
    `NextAction::Resolve`;
  - `Unknown` + record → `NeedsRebuild` + `ChangedSinceBuild`; `Unknown` + no
    record → `ActualUnknown` + `NoBuildRecord`. Two tests, because these are
    the two arms that must never collapse.
  - a feature promoted nowhere → `NotDesired` in every environment, no
    `NextAction`, and a `summary` that says so rather than being `None`.
  - a `LegacyUnknown` environment produces a `why` that *renders*, not an
    error and not a `NeedsRebuild`.

### Task 6 — Rendering `why`

**Files:** `src/core/render.rs`

**Interfaces:**
- Consumes: `crate::core::why::{WhyExplanation, WhyMembership, WhyReason, NextAction}`.
- Produces: `render_why(&WhyExplanation) -> String`, plus the
  `why_membership_label` / `next_action_command` helpers the renderer's
  vocabulary lives in.

- [x] The three forms follow §14.2/§14.3/§14.4's section names: `Desired`,
  `Actual`, `Why?`, `Files`, `What Hitch did`, `Next`. Headings come from the
  existing `heading()` helper (`src/core/render.rs:579`) so they are indented
  and blank-line-separated like every other renderer's.
- [x] The equations are rendered by `render_equation`, not reformatted. So
  `hitch why dev` and `hitch status --environments dev` cannot describe `dev`
  differently.
- [x] `Why?` is present **only** when there is a reason. A `Realised` branch
  gets no "Why?" section rather than a section saying nothing — §14's whole
  point is that the section appears when there is something to explain.
- [x] `next_action_command` is the one place that writes `hitch rebuild dev`,
  `hitch resolve dev --branch feature/dashboard`, `hitch demote …`, and
  `hitch promote …`. A test asserts each `NextAction` variant renders the exact
  command line a user would paste, so this table cannot drift from the CLI's
  actual argument order without a test failing. (`hitch resolve <env> --branch
  <branch>` — `src/commands/resolve.rs:12-20`.)
- [x] Unit tests: three golden-ish assertions over hand-built
  `WhyExplanation` values, one per form, plus one for "no reason → no Why?
  section" and one for each `WhyReason` variant's sentence.

### Task 7 — The `hitch why` command

**Files:** `src/commands/why.rs` (new), `src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `WhySubject::resolve(&GlobalContext, &[String]) -> Result<WhySubject>`,
  `build_state_snapshot`, `build_why`, `render_why`, `emit_json`.
- Produces: `WhyCommand { target: String, environment: Option<String>, json: global }`,
  registered in `Commands`, dispatched in `main.rs`, and listed in
  `command_is_mutating`'s `false` arm.

- [x] Two positional forms, matching §14.1 exactly and nothing more:
  `hitch why <target>` and `hitch why <target> <environment>`. No aliases, no
  `--for`, no subcommands — §14.1 asks for narrow and predictable.
- [x] `WhySubject::resolve` (in `commands/why.rs`, because it needs
  `GlobalContext`): given the target and an optional second argument —
  - second argument present → `FeatureIn(target, env)` after checking the
    environment is declared (error naming the known ones if not);
  - absent → the target is both checked as an environment and as a ref:
    - environment **and** declared feature → error naming both readings and
      both commands;
    - environment only → `Environment(target)`;
    - otherwise, if `rev_parse_opt("refs/heads/<target>")` resolves → `Feature(target)`;
    - otherwise → error: not an environment, not a branch, with the known
      environment names.
  The one git call is this `rev_parse_opt`, and it is the only reason the
  command touches `GitOperations` at all.
- [x] `run` takes exactly one snapshot, resolves the subject, calls
  `build_why`, then either `render_why` to stdout or `emit_json` under
  `{"schema_version", "why"}`. Under `--json`, stdout is the document and
  nothing else — including the `pre_check_repo_only` chatter, which goes
  through `context.log_verbose` and is therefore already stderr-only.
- [x] `main.rs`: `Commands::Why(_) => "why"` in `command_name`, the dispatch
  arm, and `command_is_mutating`'s `false` arm. It is read-only, so it takes
  no repo lock and runs no `recover` pass — a read-only command that could
  block on a lock would be the wrong kind of explanatory tool.
- [x] `cli.rs`: `Why(commands::why::WhyCommand)` with a doc comment naming the
  three forms. Its `subcommand_negates_reqs`/help ordering puts it next to
  `Status` in `--help`.

### Task 8 — Tests

**Files:** `tests/integration/status_tests.rs`, `tests/integration/tree_tests.rs`,
`tests/integration/state_model_tests.rs`, `tests/unit/why_tests.rs` (new)

- [x] **Golden unit tests over hand-built snapshots** — this is where the §12
  breadth lives, because a matrix is a pure function of a snapshot and a real
  repository only makes it slower to construct. Cover all seven cell states, and
  the display conditions the master's exit criteria name: colour disabled, a
  narrow terminal (a 20-column budget forcing a documented fallback), a long
  branch name beside a short one, zero environments, many environments (12), and
  many features (20).
- [x] **Integration tests for the states that only real history produces**, in
  `status_tests.rs` — a real conflict producing a real hold, a base branch
  moved after a build, a legacy no-record repo (`git update-ref -d` the state
  ref, the same technique as
  `test_an_environment_built_without_a_record_is_legacy_unknown`), and a
  deleted feature ref. Each asserts the *cell*, not the whole screen, plus one
  full-screen assertion for the clean-repo case so the shape is pinned once.
- [x] `state_model_tests.rs` gains
  `the_matrix_cell_and_the_why_membership_never_disagree` — for a set of
  scenarios, `MatrixCell::classify` and `build_why`'s membership produce the
  same answer for every (branch, environment) pair. This is the test that makes
  Global Constraint 2 true rather than aspirational, and it is the one that
  would fail if a future `why` grew its own classifier.
- [x] `why_tests.rs` (unit, no repository): one test per §14 form, one per
  `WhyReason`, one per `NextAction`'s rendered command, and the ambiguity error.
- [x] Re-point the existing `status_tests.rs` (14 tests) and
  `tree_tests.rs` (12 tests) assertions. `state_model_tests.rs`'s two
  snapshot/model agreement tests are about the model, not the screen, and must
  keep passing **unmodified** — if a re-point is needed there, the model moved
  and the test is telling the truth.

### Task 9 — Manual verification and gates

**Files:** none (verification only)

- [x] `cargo build -p hitch` (debug — `just build` is release, and nothing
  here depends on the abort hook, but the manual check must exercise the same
  binary shape CI does not).
- [x] Drive a throwaway repo in `/tmp` through: `hitch init`; a promotion; a
  `hitch rebuild`; `hitch status` (clean grid); a conflicting promotion so a
  real hold appears in the grid; `hitch why <branch>`;
  `hitch why <branch> <env>`; `hitch why <env>`; `hitch why <env> <other>`
  (the ambiguity error); `hitch why nonexistent`; `hitch status --environments`;
  `hitch status --json | jq .`; `hitch why --json | jq .`; and
  `hitch status --enviroments` (typo) to confirm the error is legible. Read the
  output as §12 and §14 do: can you answer "where is this feature and is it
  realised" without opening `hitch.json`?
- [x] `NO_COLOR=1 hitch status` and `hitch status | cat` — both must be fully
  legible (Constraint 8), and a byte-check that the piped form carries no escape
  sequences while the TTY-ish form does.
- [x] `just format`, then `just format-check && just lint`, then `just test`.
  All three clean, in that order. Clippy runs `-D warnings`.
- [x] Record the findings in this plan's "As executed" section, in the same
  commit as the code, and update the master plan and `AGENTS.md` in the same
  change.

---

## As executed

## As executed

Landed as one commit on `explainable-ux`, on top of the P7 plan commit `6f9e261`.
`main` is still `5d81fb2` and `crates/` is still untouched.

### What was built

Three new pieces plus rewiring, exactly as planned.

- **`src/core/status.rs`** — the matrix as a pure projection.
  `MatrixCell` (`src/core/status.rs:140`) is the **seven** states §12 names, and
  the plan's "eight" was simply wrong: `NotDesired`, `Included`, `Held`,
  `InBase`, `NeedsRebuild`, `ActualUnknown`, `Missing`. `MatrixCell::classify`
  (`:202`) is total over `(desired, actual, has_record)` and the order of its
  arms is the order of authority: not-declared first (a branch nobody declared
  has no membership story), then `Missing`, then `Held`, then `AlreadyInBase`,
  then `Included`, then the two `Unknown` arms split on whether a record exists.
  `build_matrix_model` (`:316`) materialises **every** cell and the summary
  counters are computed *from* those cells, so a count and a row can never
  disagree. `has_record_for` (`:405`) is derived — `has_record ⟺ !matches!(health,
  LegacyUnknown)` — rather than a second fact read from a second place.
  `build_status_model` (`:65`) stays a pure function of the snapshot, and it now
  builds the matrix as well as the headline.
- **`src/core/why.rs`** — the explanation model. `WhyExplanation` (`:60`) is
  `#[serde(tag = "form", rename_all = "snake_case")]` over three forms —
  `feature`, `feature_in_environment`, `environment` — so the shape is chosen by
  the form rather than by which fields happen to be populated. `WhyMembership`
  (`:84`) is a distinct vocabulary from `ActualMembership` with a one-to-one
  `From<MatrixCell>`, because §14 asks questions `ActualMembership` cannot
  answer (`InBase` is an outcome, "already in the base" is a fact about a
  journey). `build_why` (`:296`) is pure over the snapshot; `WhySubject` is
  resolved by the caller, because resolution needs a repository and the model
  must not have one.
- **`src/core/render.rs`** — `render_equation` (`:388`),
  `render_matrix` (`:672`, with `render_matrix_at` (`:805`) taking the width
  budget), `render_environment_summaries` (`:834`), `render_why` (`:1155`).
  `describe_projection` is rewired onto `render_equation`, so the equation has
  exactly one spelling in the codebase — §13's whole point.
- **`src/commands/why.rs`** — the command, taking one snapshot, resolving the
  subject, and rendering. `src/cli.rs` and `src/main.rs` carry the registration;
  `--json`'s doc comment now names **five** commands.
- **`src/commands/status.rs`** — matrix by default, the pre-existing
  per-environment view moved wholesale behind `--environments [NAME]`, and
  `status --json` emitting `{"schema_version", "status"}`.
- **`src/commands/tree.rs`** — the environment node's parenthetical now goes
  through `render_equation`, so `hitch tree` and `hitch status` cannot spell a
  composition differently.

### Design decisions worth keeping

- **A cell and its row answer different questions, and are rendered
  differently.** A cell is *what is in the last build*; the row under the
  environment name is *whether that build is still current*. Collapsing them
  into one verdict is what produces a status line that says "3 desired, 3
  actual, up to date" for an environment that is holding a branch.
- **`ActualMembership::Unknown` is never rendered as a fact.** It is one of
  Constraint 11's cases and it recurred in the implementation: `classify` splits
  it on `has_record`, and `render_why` refuses to emit a `Why?` clause for a
  membership that has no reason, because "unknown" plus a confident explanation
  is a contradiction in one line.
- **`reason_for`'s order of authority is deliberate and is the thing to
  preserve** (`src/core/why.rs:635`): the branch itself, then the
  environment-level condition, then *this branch's* staleness, then the **base's**
  staleness (`BaseMoved`), and only then the catch-all. A `NotDesired` cell
  returns **early**: it is answerable from the declaration alone
  (`DemotedSinceBuild` when the record's `removed` names the branch, `None`
  otherwise), and letting it reach the environment-level arms would let a stale
  *environment* explain a branch that was never in it. That early return is a
  bug class, not a style choice — it is the reason `reason_for` is a function
  with arms in an order rather than a match on the cell.
- **`NextAction` is a closed enum** (`src/core/why.rs:175`), and only
  `NextAction::Resolve` is ever emitted for a `Held` branch — a missing branch's
  next action is not a resolution.

### Deviations from the plan as written

- **A second unit-test file.** `tests/unit/why_tests.rs` and
  `tests/unit/matrix_render_tests.rs` rather than in-module tests in
  `src/core/*.rs`. The in-module tests for `render_plan`/`render_receipt` stayed
  in-module; the *model* tests did not, because `why.rs`'s doc and fixtures ran
  to a size where an in-module test module is the majority of the file and the
  doc comment stops being the thing you read first. Recorded in
  `tests/unit/mod.rs`.
- **`MatrixSummaryRow` gained `locked: bool`.** A locked environment's summary
  is a fact the status view would otherwise omit, and the alternative — a
  separate warning line — reintroduces a second voice for the same environment.
- **`display_overall_summary` → `display_headline(context)`.** The old name says
  what it *was* (a summary of the overall picture) rather than what it is now
  (one line of context before the headline). It is the only renamed P6 symbol.
- **`why_membership_label` capitalises; `MatrixCell::label` does not.** A matrix
  is a table and its cells are not sentences; an explanation is prose and its
  first word is. The tests compare case-insensitively so a future cell cannot
  acquire a capital accidentally.
- **Section dividers in `render.rs`.** The `// ── … ──` markers added for the new
  sections had to come out: this clippy version's `empty_line_after_doc_comments`
  rejects a blank line after a doc comment *and* a `//` marker between two doc
  comments, so a divider can only follow a non-doc item. Rather than reorder
  real code to host a cosmetic comment, the two new markers are gone and the
  spec section is named in each function's own doc line instead. Cosmetic cost
  only; worth knowing before the next author adds a divider.

### The P7 tail — two display defects found in a manual walkthrough

A walkthrough of `hitch status` / `promote` / `demote` / `release` found two
defects in surfaces P6 built, both in the same class, both fixed in this commit
rather than left for P8. They are recorded here because the fixes touch code
P7's plan never mentions, and a future reader diffing the plan against the tree
should know that is deliberate.

1. **A nested rebuild narrated itself between "Will change" and the receipt.**
   The plan's section said what would change; then `[1/6] Synchronizing
   branches`, `[2/6] Merging 'feature/payments'`, `✅ Rebuilding environment
   'dev'`; then the receipt. The root cause was documented in-tree and known
   false — `src/utils/prelude.rs` claimed the nested rebuild prints the
   `StepLogger` transcript *instead of* a plan and receipt, when in fact it
   plans, applies, **and throws both away**.
   Fixed by threading a `StepNarration` parameter through
   `rebuild_environment_opts` / `rebuild_environment`, whose **default posture
   is `Suppressed`**: a command with a plan and a receipt of its own has nothing
   to add between them. `StepNarration::Log` is the exception, taken by exactly
   one caller — `src/commands/approvals/approve.rs:379`, the one mutating
   command that has no plan of its own. Suppressing by default rather than
   per-call-site is the point: a new caller gets the quiet behaviour by
   omission, and a caller that genuinely has no receipt has to say so.
   The same pass removed two more duplications in the same voice: release's
   apply-time `on_step` is now `&mut |_| {}` (matching promote/demote), and the
   `⛔ Held '<x>' — conflicts with '<y>'` line inside `compose_environment` is
   gone. That last one had a second benefit: every production caller of
   `plan_rebuild` renders a plan whose Composition section already names the
   branch, the partner, the file count and the remedy, so the line was a fourth
   copy — and the one that made `rebuild_tests.rs` document a dry-run/real-build
   divergence that no longer exists. That comment is corrected in the same commit.
2. **The `Result` block did not group, and duplicated.** `⧗ dev needs rebuild`
   was followed by the release's own `main 5e701ab → 7986a00` and its prune
   lines, as three unrelated facts at one indent. Two distinct causes:
   - *Grouping* — `render_resulting_state` (`src/core/render.rs:1053`) now
     nests everything about an environment under its own line, which is what
     makes `⧗ dev needs rebuild` a statement about `dev` rather than the header
     of a list.
   - *Duplication* — `feature/clean 63ba073 → gone` and `feature/clean removed
     from the declaration` were the same branch printed twice. A changed input
     whose branch is in the record's `removed` is now skipped. This is provable
     rather than incidental: `health_from_record`
     (`src/core/state.rs:604`) walks the *recorded* pins, and a departed branch
     resolves to no current SHA, so `removed ⊆ changed_inputs` always. `added`
     is disjoint by construction and needs no guard. Each detail line also gained
     the verb `moved` — `main moved 5a33953 → 5ba013b` — so all three kinds read
     in one grammar: *moved*, *added to the declaration*, *removed from the
     declaration*.

   One thing deliberately **not** changed: a fact that legitimately appears in
   both the effects and the Result (a release's `main 5e701ab → 7986a00` as both
   an effect and a dependent environment's changed input) is not a duplication
   bug. The effect is *what I did*; the Result is *where things stand*; the
   heading is what keeps them apart.

3. **A hold inside a nested rebuild never reached the receipt at all.** This was
   found while verifying (1) and is the same user-visible symptom as (1)'s
   duplication, so it is fixed here too. `apply_declaration_plan` and
   `apply_release_plan` each dropped the nested build's conflicts on the floor
   (`Ok(_)`), so `✓ rebuild dev` claimed a clean rebuild of an environment that
   was in fact holding a branch — the exact flattening `AppliedWithHolds` exists
   to prevent, arriving through a different door. `AppliedEffect::DependentEnvironmentRebuild`
   gained `held: Vec<HoldPair>` (`src/operations/model.rs`), and the renderer's
   glyph ladder for that effect is `⧗` if the outcome owes an effect, else `⚠️`
   if anything is held, else `✓`. The middle rung is the argument: a hold *did*
   rebuild, so `✓` is true and useless; `⧗` is reserved for work owed, which a
   hold is not. The line reads
   `⚠️ rebuild dev — 1 branch held: branch-b (conflicts with branch-a)`, and the
   `Result` block's `held:` line agrees with it, as a fact and a summary of the
   same fact respectively.

### Tests

668 pass, 0 fail: 160 `--lib`, 507 `--test mod`, 1 `--test no_args_help`.
P7 itself accounts for 120 of them across `tests/unit/why_tests.rs`,
`tests/unit/matrix_render_tests.rs`, `tests/integration/why_tests.rs`,
`tests/integration/status_tests.rs`, `tests/integration/state_model_tests.rs`,
`tests/integration/tree_tests.rs`, plus the three tail tests in
`tests/integration/promote_demote_tests.rs` and the `src/core/render.rs` unit
tests for the Result block and the hold ladder.

Two test facts that cost time to establish and are worth not re-learning:

- **`promote` cannot reach a nested hold.** `pre_promote_conflict_reason` refuses
  a branch whose target environment already holds an incompatible pair, so all
  three tail tests go through `demote`, which has no such preflight. The test
  names say `test_demote_…` and that is why.
- **A `removed` line only reaches `NeedsRebuild` under `--no-rebuild`.** A
  demote that rebuilds leaves the environment `realised`, so
  `test_a_demoted_branch_is_named_once_in_the_result` passes `--no-rebuild` and
  scopes its count assertion to the `Result` block (via `split_once`) — the plan
  legitimately names the same branch three more times above it.

### Gates

`just format` → `just format-check && just lint` → `just test`, in that order,
all clean. Manual verification in throwaway repos under the approved temp dir
covered: the status matrix in a held and a partially-held environment;
`demote` with and without `--no-rebuild`; `release` of two branches with two
dependent rebuilds; a release that conflicts (exit 1, unchanged, single report);
and `hitch rebuild`'s own held case.

### A third duplicate, left alone deliberately

`hitch rebuild` prints the identical hold sentence twice — once in the plan's
"Needs your decision" and again as a receipt advisory, because `rebuild::run`
copies `plan.warnings` into `receipt.warnings` mechanically. Same class as the
two that were fixed, different surface: the *plan* is a prediction and the
advisory is a fact, so the right fix is to decide which of the two carries the
hold and let the other say something else, which is a question about
`hitch rebuild`'s receipt rather than a display bug. Reported rather than
silently fixed, because the two that were fixed were the two that were asked
for.

> **Resolved after P7, in a follow-up commit.** The framing above was right and
> the answer turned out to be neither half. The premise — "the plan is a
> prediction and the advisory is a fact" — is what makes the *copy* the bug
> rather than either document. Both were the same sentence because a receipt's
> warnings were built by a mechanical `map` over the plan's, and a `PlanWarning`
> is never a receipt warning. Removing the copy in all four places it existed
> (`rebuild`, `declaration`, `release`, and `declaration`'s `ApprovalRequested`
> arm) leaves the hold predicted once in the plan and stated once as a fact in
> the `Result` block, read from `core::state`. `a_receipt_never_restates_a_plan_warning`
> holds the generalisation. See the master plan's "the receipt's warning
> contract" for the full rule and the two costs that were accepted.

### Carried forward, still open

- **`hitch approve` has no receipt.** The honest route is a
  `plan_declaration_change` for the approved request's change, applied *after*
  the approval commits — which is also the only reason `StepNarration::Log`
  still exists, and removing that last caller is the marker that it is done.
- **`src/commands/resolve.rs` still chooses its resolution mode from
  `preflight_compatibility_report`** (`resolve.rs:131`, `:180`). It is the last
  remaining decision point on a preflight rather than on the shared composition
  primitive, and a disagreement there picks the wrong resolution mode rather
  than merely printing a stale preview.
- **The rollback/lock-ordering bug.** `capture_config_state` runs *inside*
  `with_locked_env`; `rollback_metadata_changes` runs outside it, by which time
  the lock has been released. Restoring the snapshot therefore restores
  `locked: true` and the environment stays locked until someone runs `hitch
  unlock`. Reproduce with `hitch promote` into an approval-gated environment as
  the sole approver, then promote the same branch again. Needs a decision about
  the snapshot's shape (re-clear the lock after restoring, versus capture before
  the lock and keep the current ordering) — not a rendering question, so it was
  never P7's.
