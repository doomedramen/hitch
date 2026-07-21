# Plan: GitHub PR workflow for hitch-managed repositories

Status: **proposed** (design agreed, not yet implemented)

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

A **ruleset on `main`** that *restricts who can push*, with the deploy identity
that runs `hitch release` (ideally a bot account or deploy key, not humans) as
the only bypass actor.

- The merge button is disabled for everyone else — an approved PR cannot reach
  production through GitHub.
- Required approvals / required reviews can be layered on as desired; they
  gate nothing hitch does, but keep review discipline visible.
- Do **not** use "require a pull request before merging" — that rule would
  reject `hitch release`'s own push to `main`.

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

## Open questions

- Is there an org/compliance policy that forbids PRs even *targeting* `main`?
  (That is the only scenario where a separate PR-target branch would be
  reconsidered.)
- Which identity runs `hitch release` in practice (human release managers vs a
  deploy bot), i.e. who goes on the ruleset bypass list?
