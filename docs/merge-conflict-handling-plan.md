# Plan: merge-conflict handling for environment rebuilds

Status: **in progress** — design synthesized from a five-design panel (git-native,
merge-queue, interactive-UX, state-machine, and team-product lenses), scored by
three independent judges (unanimous winner) and stress-tested by a red-team
pass. This document is the merged result with all mandatory hardening folded in.

## Implementation status

- ✅ **Phase 1** — worktree-isolated rebuild, OID pinning, CAS publish, timestamped backup refs.
- ✅ **Phase 2** — exhaustive pair attribution (`preflight_compatibility_report`), `hitch rebuild --dry-run`.
- ✅ **Phase 3 (core policy)** — `Environment.on_conflict` (eject default / halt), live eject-and-continue in `rebuild_environment`, `--on-conflict` override, `hitch set --on-conflict`, `hitch conflicts <env>` standup command.
- ✅ **Phase 3 (exit codes)** — `hitch rebuild` exits 0 clean / 2 succeeded-with-holds / 1 halted-or-error (simplified from the original 4-way 0/2/3/1 sketch: halt and generic error both exit 1, since in practice a CI script treats "the rebuild didn't happen" the same either way — the taxonomy that actually matters for automation is "did anything get held", which 0 vs 2 covers).
- ✅ **Phase 3 (status glyph)** — `hitch status` shows a ⛔ glyph and "(conflicts with X — held on rebuild)" for a branch that would be held, via a new local-only, no-fetch `preflight_compatibility_report_local` so status stays fast and offline.
- ✅ **Phase 3 (PR comment integration)** — `hitch rebuild <env> --pr-comments` (opt-in, off by default) upserts a single marker-tagged comment per promoted branch's open PR: held branches get the pair/files/fix-command, and a branch that heals gets its held comment flipped to "re-included" — but only on a PR hitch already commented on, never a fresh comment to one it hasn't. Entirely best-effort: any `gh`/network failure is swallowed and logged at verbose only, so this can never be why a rebuild fails. Verified locally that the no-`gh`/no-GitHub-remote no-op path is silent and doesn't affect the rebuild; the actual GitHub API round-trip (PR lookup, comment create/update) could not be exercised end-to-end without a live repo — the marker-matching and message-formatting logic has unit tests instead.
- ✅ **Phase 3 (tree glyph)** — `hitch tree` now threads `GlobalContext` through its recursive display and shows the same ⛔/"conflicts with X — held on rebuild" as `hitch status`, via the same `preflight_compatibility_report_local`. Phase 3 is now fully done — no deferred pieces remain.
- ✅ **Phase 4** — `hitch resolve <env> [--branch b] [--continue|--abort|--path] [--tool]`. Mode auto-detected from `conflicts_with`: **Mode A** (conflicts with base) runs `git rebase <base>` in a disposable detached worktree, handing off to plain Git for the resolution itself and landing the rebased tip with a CAS `update-ref` plus the standard checkout resync; on success, offers a confirmed `--force-with-lease` push. *(Originally this checked the branch out in the user's own repository and checked them back afterwards — see the Mode A bullet below for why that was replaced.)* **Mode B** (conflicts with a peer) builds the composition up to that branch in a disposable worktree at a *deterministic* path (`.hitch-resolve-<repo>-<env>-<branch>`, sibling to the repo) so `--continue`/`--abort`/`--path` can find it again with zero persisted state — the worktree's existence on disk *is* the state. `--continue` refuses if unmerged entries or leftover `<<<<<<<`/`>>>>>>>` markers remain, then finishes the commit, composes any branches promoted after it, and **publishes the result as the environment's new content** via the same `publish_environment_build` (extracted from `rebuild_environment` in this phase) rebuild itself uses — a one-time inclusion, explicitly *not* persisted (no rerere, no resolution cache — that's phase 5), so the next plain `hitch rebuild` holds the branch again unless the fix is carried back into a real branch. Verified end-to-end locally for both modes (rebase-pause-and-continue; worktree-conflict-continue-publish-cleanup; `--abort`; branch inference when unambiguous; ambiguous-branches error) plus 4 integration tests.
  - **Bug found and fixed during phase-4 testing**: all three preflight functions (`preflight_compatibility_merge_tree`, `preflight_compatibility_report`, `preflight_compatibility_report_local`) were passing `base_branch`'s own current tip as `git merge-tree`'s `--merge-base` argument instead of the true common ancestor of `base_branch` and each branch. Whenever base had moved on independently after a branch diverged — the single most common real-world conflict shape, and exactly Mode A's scenario — `git merge-tree` saw "our" side as unchanged since the (wrong) merge-base and silently fast-forwarded to the branch's content, missing the conflict entirely. This was a pre-existing bug (inherited from before this session, in the original promote-gate preflight), invisible until Mode A's smoke test hit it, because every prior test's conflict scenario was peer-vs-peer (two branches diverging from an *unmoved* base), where the wrong merge-base happens to equal the right one. Fixed by computing `get_merge_base(base_branch, branch)` per branch in all three functions; regression test added (`test_conflicts_detects_branch_conflicting_with_moved_base`). The real squash-merge execution path (`rebuild_environment`'s actual `git merge --squash`) was never affected — real `git merge` always computes its own correct merge-base — so this only undercounted what the *preflight* commands (`--dry-run`, `hitch conflicts`, the status glyph) reported; a real rebuild's eject/halt behavior was correct throughout.
- ✅ **Phase 5** — shared, persisted conflict resolutions, **content-addressed by exact merge-stage OIDs, not rerere**. Deliberate deviation from the original "rerere-backed" wording, and the single most important design decision of this phase: the red-team pass surfaced that rerere's two worst failure modes are exactly the two the doc most feared, and both vanish under OID-tuple keying — (a) rerere replays a *stale* resolution when a force-push changes meaning but leaves the conflicted hunk textually identical, whereas keying on the exact `(path, base_oid, ours_oid, theirs_oid)` blobs makes any changed side a cache *miss*, never a wrong replay (staleness is structural, not a check that can be forgotten); (b) rerere's shared `rr-cache` is global to the repo and silently alters the user's *own* plain-git merges, whereas resolution refs are never touched outside hitch's own worktree operations. Since Mode B already owns the whole conflict, hitch reads the three merge stages straight from `git ls-files --unmerged` and needs neither rerere nor any hand-rolled hunk normalization. Full rationale in `src/utils/resolutions.rs`'s module header.
  - **Storage** — one ref per resolution at `refs/hitch/resolutions/<key>`, `<key>` = a git hash over the sorted stage tuples; the ref is a parentless commit whose tree holds `meta.json` + one blob per resolved file, so it transports via ordinary `git push`/`fetch` and can't be GC'd.
  - **Recording** — `hitch resolve --continue --record` (or `--share`, which also pushes). Both off by default; `--share` is separate from `--record` so team-wide sharing is always deliberate and never inherits `--yes`. `hitch resolutions list/show/forget/fetch` inspects, prunes, and pulls.
  - **Replay** — `hitch rebuild --replay-resolutions` (opt-in per run; the flag can't hide in `HITCH_YES`, so it is itself the authorization to apply someone's recorded content to a deployable branch). Without `--yes`, each distinct resolution is confirmed once; under `--yes` (CI) every application is logged loudly with key + recorder. Applying overwrites only the recorded conflicted paths, re-stages, refuses to commit if any conflict remains, and on any failure aborts back to a clean worktree and holds the branch rather than leaving it half-applied.
  - **Debt SLA** — `hitch doctor` reports every recorded resolution and its age; `hitch doctor --max-resolution-age-days <n>` exits nonzero when any exceed the threshold, so CI can gate on stale conflict debt. This is the forcing function that keeps recorded resolutions from becoming permanent load-bearing infrastructure instead of being carried back into feature branches.
  - **How the red-team criticals are met**: review-gate bypass (unreviewed code auto-landing via resolution refs) → replay is opt-in per run and can't inherit `--yes`, plus loud logging and the debt SLA; stale replay → eliminated structurally by exact-OID keying (verified by `test_replay_is_a_miss_after_branch_moves`); rr-cache corrupting plain git → doesn't exist, resolutions never touch rr-cache. Verified end-to-end locally (record → held-without-flag → composed-with-flag → miss-after-branch-moves → doctor debt gate) and by integration + unit tests.
  - **Deferred / not built** (documented rather than half-implemented): per-resolution *reuse count* (would need mutating the shared ref on every replay — race-prone and complicates transport; age-based SLA is the load-bearing property and is built); `hitch release` refusing `--yes` when a release would need a recorded resolution (release never replays by design, so this is a softer forcing function than the doctor SLA — left out to avoid coupling release to resolution internals for speculative value); a first-use approval store that survives across runs (the per-run `--replay-resolutions` flag + per-run confirmation is the authorization model instead).

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
  `hitch resolve <env>` detects this mode and runs a **guided rebase** in a
  disposable *detached* worktree at the same deterministic session path Mode B
  uses: `git rebase <base>` there, `--continue`/`--abort` wrapped the same way
  as Mode B, then the rebased tip is landed on the branch with a CAS
  `update-ref` (plus the standard checkout resync) and offered as a confirmed
  `--force-with-lease` push. The conflict is permanently retired — no
  recorded-resolution debt is created, matching the README philosophy that
  hitch does not erase real Git costs.
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
- The user's own checkout is never the execution locus in either mode, and is
  never modified at all. Mode A originally checked the feature branch out in
  the real repository and checked the user back out afterwards; that put their
  working tree in the middle of the operation (a failure between the two
  checkouts stranded them on another branch, and a conflicted rebase parked
  them mid-rebase on a branch they never asked to be on). Working detached
  removes that entirely, and also handles the case the old design could not:
  rebasing the very branch the user is standing on.

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
