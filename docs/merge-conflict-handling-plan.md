# Plan: merge-conflict handling for environment rebuilds

Status: **proposed** — design synthesized from a five-design panel (git-native,
merge-queue, interactive-UX, state-machine, and team-product lenses), scored by
three independent judges (unanimous winner) and stress-tested by a red-team
pass. This document is the merged result with all mandatory hardening folded in.

## Problem

An environment branch (`dev`) is declared as `base` plus an ordered list of
promoted feature branches. `hitch rebuild dev` composes them as sequential
squash merges. When one or more of the ten promoted branches conflict — with
`base` or with a branch promoted ahead of them — today's behavior is:

1. **Halt everything.** The `git merge-tree` preflight refuses the whole
   rebuild on the first conflict. One conflicting branch blocks nine healthy
   ones; the environment goes stale until that one author acts.
2. **Rebuild runs in the user's checkout.** Temp branch via `git checkout -b`,
   real `git merge --squash` in the working tree, checkout churn
   (original → temp → env → original), and a TOCTOU window: the preflight can
   pass yet the real merge conflict (refs are re-fetched between the two), at
   which point cleanup happens *in the user's tree*.
3. **Resolutions evaporate.** Nothing records how a conflict was resolved, so
   the same conflict is re-resolved on every rebuild. SKILL.md names this as
   the known pain: "repeated merge fixes across environments unless resolved
   changes are carried back into the feature branch."
4. **Attribution stops at the first failure.** The preflight reports the first
   conflicting branch only, and only against the accumulated composition — it
   does not distinguish "conflicts with base" from "conflicts with the branch
   promoted ahead of you", and it never reports the second, third… culprits.

## Prior art (what everyone else does)

- **Merge queues** (GitHub merge queue, bors, Mergify, Zuul, GitLab merge
  trains): universal policy is **eject-and-continue** — drop the conflicting
  item from the set, notify its author with the reason, rebuild the rest,
  never halt. None attempts auto-resolution. Zuul formalizes retry as NNFI
  (nearest non-failing item); bors bisects batches to find the culprit.
- **git rerere**: content-addressed conflict-hunk resolutions, replayed on
  exact match. Purely local (no git transport), and known footgun: silent
  replay of a stale resolution (git-imerge disables rerere for this reason).
- **git-octopus** (`git conflict`): the closest direct prior art to an
  env-branch rebuilder. Stores resolutions as content-addressed refs
  (`refs/conflicts/<sha1>`) — native git push/fetch transport, auditable,
  auto-applied during integration-branch rebuilds. Unmaintained; design
  reference only.
- **Graphite / gh-stack restack UX**: conflict → resolve in a worktree →
  `continue`; gh-stack adds atomic rollback and auto-enables rerere.
- **jj**: first-class conflicts committed and resolved later. Not viable for a
  deployable branch (materializes as conflict-marker soup), but
  conflict-*metadata*-in-refs is.
- **Key nuance worth copying** (GitHub MQ): an item can be conflict-free
  against base yet conflict with items *ahead of it* in the ordered set —
  surface the pair, not just "conflicts".

## Rejected designs

Four alternatives were fully designed and judged; each lost for a specific
reason worth recording:

- **Halt-everything (status quo).** Universal counterexample in prior art; one
  author blocks nine. Kept only as an opt-in per-env policy (`on_conflict:
  halt`) for demo-critical or approval-gated environments.
- **Pause-mid-rebuild (Graphite-style interactive state machine).** Best
  single-player UX of the panel, but a paused rebuild is a *team-wide logical
  lock* on the environment: one developer pausing at branch 6/10 and walking
  away blocks every teammate's and CI's rebuild until someone force-steals.
  With `--yes`/non-TTY defaulting to halt, unattended CI never self-heals.
  The resolve-in-a-worktree ergonomics are grafted; the pause-the-rebuild
  state machine is not.
- **Checkout-free composition via `merge-tree`/`commit-tree`/`mktree`.**
  Intellectually the cleanest (nothing observable changes until one atomic
  `update-ref`), but it replaces the merge engine: the deployable branch would
  be produced by a different code path than the real merges humans resolve,
  so any parity gap (renames, merge drivers, renormalization) lands directly
  on the one branch that must never break — and the human-resolution worktree
  is needed anyway, so you build and maintain two engines. Its best idea —
  **pin all branch OIDs once after fetch and compose only from those** — is
  grafted into the winner.
- **Hand-rolled "rerere-style" hunk hashing in Rust.** Reimplements exactly
  the subtle normalization machinery git already provides; a normalization bug
  silently changes replay keys or applies a wrong postimage, with no oracle to
  test against. Matching and replay must be literally `git rerere`, with hitch
  reading rerere's own ids off disk.
- **`Vec<String>` → `Vec<PromotedBranch>` metadata migration.** Bricks every
  older hitch binary on the team the moment one member upgrades, for data that
  is not load-bearing to the conflict system. Everything below keys on branch
  names and dedicated refs; `Environment.branches` stays `Vec<String>`.

## Chosen design

Five pillars: isolated execution, eject-and-continue policy, pair attribution,
a two-mode resolve workflow, and (hardened, last) shared resolutions.

### 1. Execution: ephemeral worktree, pinned OIDs, CAS publish

- Rebuild composes in a **disposable `git worktree`** created in a sibling
  directory (or `$TMPDIR`), never inside `.git` (backup tools tar `.git`) and
  never in the user's checkout. The user's working tree is not touched by
  rebuild again, in any code path — including conflict cleanup.
- **One fetch at the start, then pin every involved OID** (base + each
  promoted branch) into an in-memory snapshot. Preflight, attribution, and the
  real squash merges all consume those pinned OIDs. This eliminates the
  preflight-passes-but-merge-conflicts TOCTOU **by construction** rather than
  by handling; the mid-merge conflict path becomes a true assertion failure,
  not an expected fallback.
- **Publish is a single compare-and-swap `update-ref`** of
  `refs/heads/<env>`, preceded by writing a timestamped backup ref
  `refs/hitch/backup/<env>/<ts>` (keep last N; `hitch rebuild <env>
  --restore-backup [<ts>]` restores). This removes today's non-atomic
  rename-to-backup → `checkout -b` window entirely.
- **Remote push uses `--force-with-lease`** keyed to the origin sha observed
  at snapshot time, so two machines rebuilding concurrently (nightly CI + a
  developer) fail loudly instead of last-writer-wins clobbering. A cheap
  remote advisory lock (CAS-push of `refs/hitch/locks/<env>` carrying
  host/pid/started_at, stale after N minutes) closes the remaining race.
- Every env squash commit carries trailers —
  `Hitch-Branch: <name>`, `Hitch-Source-Sha: <sha>`, and (later)
  `Hitch-Resolutions: <ids>` — so any historical build is reconstructible from
  `git log` alone, keyed on branch names (squash-safe), beyond backup-ref
  retention.
- Shallow-clone detection up front: `merge-tree` and merge-base work is wrong
  or impossible on `--depth=1` checkouts (the most common CI configuration).
  Rebuild preflight and `hitch doctor` detect it and fail with the exact
  remediation (`git fetch --unshallow`, `actions/checkout` `fetch-depth: 0`).

### 2. Policy: eject-and-continue

- The preflight computes the **full conflict plan in one pass**: fold the
  branches in promotion order via `merge-tree`; on conflict, record the
  culprit *and continue the fold without it*. One rebuild therefore names
  **every** non-composing branch and its conflict partner — the complete
  work-item list, not just the first failure.
- Conflicting branches become **holds**: they stay in the promoted
  `Vec<String>` (no silent demote, no interaction with the approvals
  machinery), are excluded from the built tree, and are retried on every
  rebuild — so they heal automatically the moment the underlying conflict is
  fixed, with no bot-side state.
- Per-env policy field `on_conflict: "eject" | "halt"` (serde default
  `eject`), CLI override `--on-conflict=…`. `halt` preserves today's behavior
  for environments where a human must sanction every composition.
- When a TTY is present, the plan is confirmed once before building:
  `3 of 10 branches conflict and will be held … rebuild with 7? [y/N]`.
  Under `--yes` the eject plan proceeds (this is the merge-queue lesson: CI
  must self-heal freshness without a human).
- **Distinct exit codes**, so CI can distinguish outcomes without parsing:
  `0` clean · `2` rebuilt-with-holds · `3` halted (policy or `--on-conflict`)
  · `1` error. Pipelines can page-on-2 without failing the build.
- Known cost, accepted deliberately: eject can publish a *semantically*
  incomplete composition (branch B textually merges but calls code from
  ejected branch A). Mitigations: the exit-code taxonomy, the loud status/PR
  surfaces below shipping in the *same phase* as eject, and `on_conflict:
  halt` for environments where that risk is unacceptable. Optional future
  `group:`/`depends_on:` annotations can hold a whole feature group together.
- `hitch release` is **unchanged**: halt with atomic rollback. Deploy targets
  never receive a partial composition.

### 3. Attribution and visibility

- Errors and holds always name the pair and the files:
  `'payments-retry' conflicts with 'billing-v2' (promoted earlier) in
  src/billing/invoice.rs` — distinguishing *conflicts-with-base* (fix: rebase
  the feature branch) from *conflicts-with-peer* (fix: `hitch resolve`),
  because the correct remedy differs.
- Holds are stored in a **dedicated ref** `refs/hitch/holds/<env>` (small JSON
  blob), *not* in `hitch.json`. Holds are derived state rewritten by every
  rebuild; putting them in `hitch.json` would turn the low-churn, human-authored
  `hitch-metadata` branch into a dozens-of-commits-per-day battleground and
  make metadata divergence routine. The holds ref is last-writer-wins by
  definition (safe, because it is derived), written only *after* the env CAS
  succeeds so it can never describe a build that didn't land.
- `hitch status` / `hitch tree` gain a ⛔ *held* glyph next to the existing
  ❌/⚠️/🔄/✅ set, with the pair and files inline.
- **`hitch conflicts <env>`** — the standup board: one row per held branch
  with owner (from the branch's last commit author), conflict partner, files,
  age, and the exact command to run.
- One **upserted PR comment** per held branch (found via the existing `gh`
  plumbing): edited in place on subsequent rebuilds, flipped to
  "✅ re-included" on heal. No per-rebuild comment spam.

### 4. Resolution workflow: `hitch resolve` with a Mode A/B split

The single sharpest panel insight: *where a fix lives depends on who can own
it.*

- **Mode A — the branch conflicts with `base`** (the common case). The durable
  fix is the one hitch already preaches: rebase the feature branch.
  `hitch resolve <env>` detects this mode and runs a **guided rebase**:
  check out the feature branch, `git rebase <base>` with wrapped
  continue/abort, then a confirmed `--force-with-lease` push. The conflict is
  permanently retired — no recorded-resolution debt is created, matching the
  README philosophy that hitch does not erase real Git costs.
- **Mode B — peer-vs-peer conflict** (branch A vs branch B; neither branch can
  canonically own the fix, because carrying B's changes into A would couple
  independent features). `hitch resolve <env>` creates an **ephemeral
  worktree**, replays the pinned composition up to the conflict point, runs
  the failing squash merge, and stops with real conflict markers:
  `Conflicts left in <path>. Resolve, then: hitch resolve <env> --continue`.
  Ergonomics: `--path` prints the worktree path for IDE users; `--tool` runs
  `git mergetool`; `--abort` deletes the worktree and keeps the hold.
  `--continue` refuses if unmerged entries or leftover `<<<<<<<` markers
  remain, stages **only the recorded conflicted paths** (never `add --all`,
  which sweeps in editor droppings and `.orig` files), and commits.
- The user's own checkout is never the execution locus in either mode
  (Mode A operates on the feature branch the user explicitly asked to fix).

### 5. Shared resolutions (Phase 5 — ship whole or not at all)

Mode B resolutions are worth persisting: the same peer conflict otherwise
reappears on every rebuild, for every rebuilder, including CI. Mechanism:

- Recording is **literally `git rerere`** (enabled per-invocation:
  `-c rerere.enabled=true -c rerere.autoUpdate=false`; hitch never
  reimplements conflict normalization — it reads rerere's own
  content-addressed ids off disk after the resolve commit).
- Each new entry is mirrored to **`refs/hitch/resolutions/<rerere-id>`**: a
  parentless commit whose tree holds `preimage`, `postimage`, and `meta.json`
  (env, branch, conflicts-with, path, author, recorded-at, **the exact
  stage-1/2/3 blob OIDs**, and **the source branch's head sha**). Transport is
  plain `git push`/`fetch` of that refspec — the git-octopus model, solving
  rerere's two worst gaps (no transport, no audit trail) with native git.
- On rebuild, hitch fetches resolution refs, hydrates them into `rr-cache`,
  and replays: a conflicting squash merge whose `git rerere remaining` comes
  back empty is staged and committed with loud provenance —
  `♻️ Reused resolution a1b2c3 for payments-retry ↔ billing-v2 (recorded by
  Martin, 2026-07-23)` — and the id lands in the squash commit's
  `Hitch-Resolutions:` trailer.
- Every replay prints the carry-back hint (the documented pain point):
  rebasing the feature branch with `rerere.enabled=true` replays the same
  resolution there, retiring it permanently.

#### Hardening (mandatory, same phase — the red-team findings)

The naive version of this is **not shippable**; two critical attacks:

1. **Review-gate bypass.** `refs/hitch/*` is invisible to GitHub rulesets and
   branch protection. Without hardening, anyone with write access pushes a
   resolution ref whose postimage contains arbitrary code, and nightly
   `hitch rebuild --yes` auto-lands it on the deployable branch — punching a
   hole through the PR review gate this project runs in production.
   Mitigations, all required:
   - Refuse replay when the postimage diff exceeds the recorded conflict
     region (checked against the stored stage OIDs).
   - **First use of a never-before-seen resolution id requires explicit
     confirmation that does NOT inherit `--yes`** — or, on approval-enabled
     environments, folds into the existing `ApprovalRequest` flow so a human
     approves each new resolution id once. The active resolution set becomes
     first-class in `RebuildSnapshot` (ids, recorded-by, diffstat), so
     approvals attest to what actually ships.
   - Publishing a resolution requires an explicit `--share` — the one side
     effect that must not inherit the global `--yes`.
2. **Stale replay after force-push.** rerere keys on the conflict hunk text
   only; if the author force-pushes a semantic rewrite whose conflicted hunk
   is textually identical, the old (now wrong) resolution replays silently.
   Mitigation: the recorded **source head sha** is a staleness gate — if the
   held branch's head moved, the resolution is downgraded to *stale*: replay
   only with local confirmation, never under `--yes`/CI; `hitch doctor` lists
   stale resolutions; auto-expire after N consecutive stale misses.

Further required hardening:

- **Ephemeral rr-cache hydration.** `rr-cache` is global to the repo and not
  relocatable; a developer with `rerere.enabled=true` in their global config
  would otherwise get teammates' resolutions silently replayed inside their
  *own* rebases. Hitch hydrates at the start of its worktree operation,
  records what it added, and removes those entries when done. Persistent
  hydration (for the carry-back workflow) is an explicit opt-in.
- **Pinned merge configuration** on every hitch-invoked git command, same
  discipline as the existing `LC_ALL=C`: `-c merge.conflictStyle=merge`
  (zdiff3 and custom conflict-marker-size skew rerere ids across machines —
  "works on my laptop, never fires on CI").
- **Revalidate before clearing a hold.** After `--continue` records, refetch,
  re-pin fresh OIDs, and re-run the preflight; clear the hold only if the
  held branch now composes. If the recorded preimage never fired in the fresh
  composition (a peer moved mid-resolve), keep the hold, delete the dead
  resolution ref, and tell the author exactly why.
- **Honesty about un-rerere-able conflicts.** rename/modify, delete/modify,
  binary, and submodule conflicts produce no rr-cache entry. `--continue`
  diffs the cache before/after; if nothing was recorded, it says so —
  "this conflict type cannot be auto-healed; the durable fix is rebasing
  <branch>" — and steers to Mode A instead of printing false success and
  ejecting the same branch forever. rr-cache *variants* (`preimage.N`) are
  modeled in the ref tree or refused with a clear message.
- **Push races on resolution refs**: fetch-CAS before pushing a resolution
  ref; on divergence (two devs resolved the same conflict differently), stop
  *before* clearing the hold, show both postimages, make the developer
  choose. Never force-push resolution refs implicitly.
- **Debt SLA — the forcing function.** Every live resolution is tracked
  conflict debt: `hitch doctor` reports age and reuse count per resolution,
  with a configurable `parked_max_age_days` gate (nonzero exit for CI
  rulesets). `hitch release` preflight lists exactly which recorded
  resolutions the release would need and **refuses `--yes` when that list is
  nonempty** — resolution debt must be retired (via carry-back/rebase) before
  release day, not resolved ad hoc under deadline pressure by whoever cuts
  the release.

### 6. Crash safety and recovery

- A **journal ref** per rebuild records phase
  (`Snapshot | Compose | Finalize | Push | Done`) plus host/pid/started-at, so
  a dead run is diagnosable ("died in Compose on branch 6") instead of
  branch-name archaeology over orphan `hitch-tmp-*` refs.
- **Startup reconciliation sweep** in every command (cheap): orphan
  resolve-worktrees, stale journal refs, leftover legacy
  `hitch-tmp-*`/`hitch-backup-*` branches each get a one-line named
  remediation; `hitch doctor` gets the corresponding checks.
- A failed `hitch-metadata` push becomes a **hard error with re-derive and
  retry**, not today's warning — with holds out of `hitch.json`, metadata
  writes are rare again and this is affordable.
- `--restore-backup` also rewrites (or explicitly marks stale) the holds ref
  and `rebuilt_at` in the same operation, so `status` doesn't lie during the
  one moment someone is investigating an incident.

## Implementation phases

Each phase ships standalone value; later phases are strictly optional.

1. **Worktree engine + OID pinning + CAS publish + timestamped backups.**
   No policy change, no new commands. Kills the checkout-hijack, the TOCTOU
   class, and the non-atomic swap window. Includes shallow-clone detection
   and the journal ref. Existing tests keep passing; the
   `test_hitch_rebuild_with_conflicts` assertions (no temp branch leaked,
   user still on original branch) get strictly easier to satisfy.
2. **Exhaustive pair attribution + `hitch rebuild --dry-run`.** The preflight
   folds past failures and reports every culprit with its partner; `--dry-run`
   prints the full conflict matrix read-only (also feeds the approvals
   snapshot). Better errors, zero behavior change.
3. **Eject-and-continue.** `on_conflict` config + `--on-conflict` flag, holds
   ref, exit-code taxonomy, ⛔ status/tree glyph, `hitch conflicts <env>`
   board, upserted PR comments. The policy change — shipped together with all
   its visibility surfaces, not before them.
4. **`hitch resolve` Mode A/B.** Guided rebase for base conflicts; worktree
   resolution with `--continue`/`--abort`/`--path`/`--tool` for peer
   conflicts. Big UX win even with zero sharing.
5. **Shared resolutions + all hardening + debt SLA.** The only phase with
   real trust risk; the hardening list above is part of the phase definition,
   not follow-up polish. If it is never built, phases 1–4 already deliver
   most of the user-visible value: eject, complete attribution, and a guided
   resolve workflow.

## Data-model and surface summary

| Item | Change |
|---|---|
| `Environment.branches` | unchanged (`Vec<String>` — no schema break, old binaries degrade gracefully) |
| `Environment.on_conflict` | new optional field, `"eject"` (default) \| `"halt"` |
| `refs/hitch/holds/<env>` | new — derived hold state, rewritten each rebuild, last-writer-wins |
| `refs/hitch/backup/<env>/<ts>` | new — timestamped pre-publish backups, keep last N |
| `refs/hitch/locks/<env>` | new — remote advisory lock (CAS push, stale-after-N-minutes) |
| `refs/hitch/resolutions/<id>` | new (phase 5) — preimage/postimage/meta.json, rerere-keyed |
| `RebuildSnapshot` | phase 5: gains active resolution ids for approval attestation |
| Commands | `rebuild` gains `--on-conflict`, `--dry-run`, `--restore-backup`; new `resolve`, `conflicts`; `doctor` gains reconciliation/debt/shallow-clone checks |
| Exit codes | `rebuild`: 0 clean · 2 rebuilt-with-holds · 3 halted · 1 error |

## Open questions

- Owner mapping for notifications: last-commit author vs. a per-branch
  `promoted_by` captured at promote time (the latter without any schema break,
  e.g. in the holds ref) — decide in phase 3.
- `group:`/`depends_on:` promoted-branch annotations to hold feature groups
  together on eject — deferred until the semantic-incompleteness cost is
  observed in practice.
- Whether approval-gated environments should default to `on_conflict: halt` —
  leaning yes; decide in phase 3 setup guidance.
