# Plan: GitHub PR workflow for hitch-managed repositories

Status: **implemented** — `hitch pr` / `hitch doctor` ship in this repo; the
GitHub-side ruleset has been applied to a real repository (see "Reference
implementation" below) as the template for adopting this on other repos.

## Problem

In a hitch-managed repository there is no obviously valid branch to target a
GitHub pull request against:

- `main` is production — any push to it triggers a production deployment.
- Environment branches (`dev`, `qa`, …) are build artifacts: hitch deletes and
  force-pushes them on every rebuild, so a PR targeting one is invalidated
  (or auto-closed) the next time the environment is rebuilt.

Teams still want GitHub PRs for code review, inline comments, and an audit
trail.

## Rejected design: a separate `pr_target` integration branch

An earlier proposal added a `pr_target` field per environment pointing at a
plain branch (e.g. `develop`) that exists only to receive PR merges, while
hitch ignores it for rebuilds. Rejected because:

- **It drifts irreversibly.** `develop` accumulates every merged PR forever —
  including features later demoted or abandoned — while deployments ignore it.
  Eventually it must be reset to `main`, which invalidates all open PRs and
  recreates the original problem on a delay.
- **Two sources of truth.** "Merged into develop" and "promoted/released via
  hitch" diverge immediately. The Merged badge on a PR would be meaningless.
- **Wasted conflict work.** Conflicts resolved while merging into `develop`
  never flow to any deployed branch.
- **Extra hitch surface area** (config field, `set --pr-target`, inference
  logic) for a branch Git gives no reason to have.

## Chosen design: PRs target `main`; `hitch release` *is* the merge

Two facts make this work:

1. **PR activity never writes to the target branch.** Opening, reviewing, and
   approving a PR are operations on the feature branch and GitHub's database.
   Approval does not merge; only the merge button (or auto-merge) does, and we
   disable it. So a PR targeting `main` has zero deployment power.
2. **`hitch release` already performs real `--no-ff` merges** of each promoted
   branch into the target, then pushes once (`src/commands/release.rs`). When
   GitHub sees the PR's head commits become reachable from `main`, it
   automatically flips the PR to **Merged**. No webhook, no API integration —
   the PR lifecycle becomes truthful for free: a PR shows Merged exactly when
   the code actually reached production.

`main` is the one branch in the system that is stable, never force-pushed, and
moves only forward — it is the natural merge-base of every feature branch, so
PR diffs are always clean and never invalidated.

### Workflow

```bash
hitch branch feature/foo main --to dev --to qa   # branch from main, as today
git push -u origin feature/foo
gh pr create --base main                          # PR open for review from day one

hitch promote feature/foo dev                     # rebuilds/deploys dev only
hitch promote feature/foo qa                      # rebuilds/deploys qa only

hitch release qa main                             # ONE push to main → one prod deploy
# → GitHub auto-marks the PR "Merged"
# → dependent environments rebuilt, fully-merged branches pruned (existing behavior)
```

Causality chain:

```
PR opened/reviewed/approved    → no push, no deploy
hitch promote                  → env branches rebuilt → dev/qa deploys only
hitch release <env> main       → one push to main → one production deploy (intended)
                               → GitHub marks PRs merged (status update, no further push)
```

Abandoned feature: close the PR, `hitch demote`. No residue anywhere.

### Required GitHub configuration

A **repository ruleset on `main`** (not classic branch protection — see below)
with a rule that blocks all pushes except from a named bypass actor:

- **Rule: `update`** ("Restrict updates" in the UI) — blocks every push to
  `main`, including PR-merge-button clicks (merging is a push), for anyone not
  on the bypass list. Add `deletion` and `non_fast_forward` too for
  belt-and-braces, though classic protection's force-push/deletion settings
  already cover that if present.
- **Bypass actor: a dedicated team** containing only the people/identities
  allowed to run `hitch release`. Rulesets can't name one specific individual
  directly as a bypass actor — only a role (org owner / repo admin), a team,
  a GitHub App, or a deploy key — and role-based bypass readmits everyone with
  that role. A small team scoped to exactly the intended people is the way to
  get a precise allowlist.
- Do **not** add the `pull_request` rule ("require a pull request before
  merging") — that would let non-bypass actors merge via an approved PR
  instead of being blocked outright, and would also reject `hitch release`'s
  own push unless it's separately exempted.
- Required approvals / required reviews (via classic protection, which layers
  independently) can stay on as desired; they gate nothing hitch does, but
  keep review discipline visible.

**Why a ruleset and not classic branch protection's "Restrict who can push":**
classic protection's admin bypass (`enforce_admins`) is a single all-or-nothing
switch — if it's off, *every* repo admin/org owner bypasses the push
restriction, not just the intended release identities; if it's on, admins also
get blocked by `required_pull_request_reviews`, which breaks `hitch release`'s
own direct push. Rulesets support an explicit bypass list independent of
admin/owner role, which is the only way to name exactly the intended people.

**Known residual gap:** GitHub cannot distinguish "pushed via `git push`" from
"pushed by clicking the PR merge button" for the same identity — both are just
a push. So bypass-listed humans can still technically click merge on an
approved PR. Disabling the repo's PR merge methods entirely is *not* a fix for
this: GitHub requires at least one merge method to stay enabled, and even if
it didn't, disabling them wouldn't stop a bypass actor anyway (bypass exempts
them from the same rule that would otherwise block the merge-button push).
The only real fixes are (a) use a non-human bypass identity (bot/deploy key)
so no human can ever merge via GitHub, or (b) accept it as a process-discipline
risk among a small, aware set of people. Prefer (a) when practical; (b) is a
reasonable interim choice for a small trusted team.

Hitch's existing environment gates (`requires_approval`, `min_approvals`,
`lock`) remain the release-time authority, as today.

## Implementation plan (hitch changes)

Deliberately small — the design leans on existing behavior.

1. **`hitch pr` command** (`src/commands/pr.rs`), plus a `--pr` flag on
   `hitch branch`:
   - Infer the PR base from the environments the branch targets: the shared
     `base` of those environments (which is also what the branch was cut
     from). No new config field needed.
   - Push the branch to `origin` if needed.
   - Run `gh pr create --base <base>` if `gh` is available; otherwise print
     the exact command.
2. **Docs**: a "GitHub PRs with hitch" section in the README covering the
   ruleset recipe, the approval-vs-merge distinction, and the workflow above.
3. **Squash caveat**: `hitch release --squash` rewrites the commits, so GitHub
   cannot auto-detect the merge and PRs linger open. Either:
   - document "don't use `--squash` for releases when using GitHub PRs" (v1), or
   - teach release to `gh pr close --comment` matching PRs in squash mode
     (later, optional).

Out of scope for now (revisit only if needed): syncing GitHub PR approvals
into hitch's approval gate, blocking `hitch promote`/`release` on PR review
state, or any GitHub API integration beyond shelling out to `gh`.

## Reference implementation

Applied to `Pikl-Insurance/qab` as the first real adoption of this design:

- Org team `hitch-release`, members are the people who run `hitch release`
  against that repo.
- A repository ruleset on `main` (`deletion` + `non_fast_forward` + `update`
  rules, bypass = the `hitch-release` team, no `pull_request` rule).
- PR merge methods left as-is (GitHub won't allow disabling all of them, and
  it wouldn't have closed the residual gap above regardless) — the team chose
  process discipline for that residual risk over switching to a bot identity.

Use this shape (team + ruleset) as the template when adopting the workflow on
another hitch-managed repo; adjust the team's members and the repo/ruleset
names per repo.

## Open questions

- Is there an org/compliance policy that forbids PRs even *targeting* `main`?
  (That is the only scenario where a separate PR-target branch would be
  reconsidered.)
- For future repos: bot/deploy-key bypass identity, or a human-membership team
  with accepted process-discipline risk (as chosen for qab)?
