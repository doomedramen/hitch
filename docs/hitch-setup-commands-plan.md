# Plan: `hitch setup` command family

Status: **proposed** (not yet implemented)

## Motivation

Onboarding a new repo onto hitch's GitHub PR workflow (see
[`github-pr-workflow-plan.md`](github-pr-workflow-plan.md)) currently means a
human doing a sequence of `gh api` calls by hand: work out which branches need
protecting, discover that a single person can't be a ruleset bypass actor,
create a team, build a ruleset in the right shape, sanity-check it, then flip
it live. That's exactly what happened setting this up for `Pikl-Insurance/qab`
in practice — a careful, correct, but entirely manual GitHub-CLI session.

`hitch setup` is a namespace for interactive commands that do this kind of
one-time repo configuration, discovering what they can from the repo's actual
state and asking for the parts that are genuinely a human decision. The qab
session is the worked example / informal spec for the first one.

First command: **`hitch setup rules`** — configure the GitHub-side push
restriction that makes the PR workflow safe.

## What `hitch setup rules` does

Modeled directly on the manual qab sequence, in the same order:

1. **Preconditions.** Hitch is initialized (`hitch-metadata` exists). `gh` is
   installed, authenticated, and has the scopes needed to manage rulesets and
   teams — reuse `hitch doctor`'s checks here rather than duplicating them;
   extend `hitch doctor` if it needs a scope check beyond `repo` (managing
   rulesets/teams needs `admin:org` and repo-admin access, which `gh auth
   status` scopes alone won't fully capture — see Open Questions).

2. **Discover candidate branches.** Read `hitch.json` and collect the distinct
   `base` values across all environments (e.g. `main`). These are the
   branches that PRs will eventually need to land on and that must stay
   "hitch-release-only." Present them as a checklist — don't assume every base
   needs protecting (e.g. a base that's itself another hitch environment's
   branch, if nesting is ever supported, would already be force-pushed by
   hitch and shouldn't get this treatment).

3. **Check existing GitHub state before proposing anything.**
   - Existing classic branch protection on the target branch: surface it
     (required reviews, status checks, `enforce_admins`). Rulesets layer with
     classic protection — most restrictive combination wins — so silently
     adding a ruleset on top without showing what's already there risks
     confusing double-enforcement (e.g. review counts stacking).
   - Existing rulesets targeting the branch: if one already looks like this
     pattern, offer to view/adjust it rather than create a duplicate.
   - Existing teams that look like a bypass team for this repo (naming
     convention TBD, e.g. `<repo>-release` or a hitch-authored marker in the
     team description) — offer to reuse rather than create a new one on every
     re-run.

4. **Ask who bypasses — never infer this.** This is the one genuinely human
   decision: which accounts are allowed to run `hitch release` against this
   branch. Do not guess it from commit history, existing admin lists, or
   anything else — org/repo admin status is not the same thing as "should be
   allowed to release," as the qab session's own admin audit showed (7 repo
   admins, 3 org owners, but only 3 people who should actually bypass).
   Offer:
   - Select an existing team (list org teams).
   - Create a new team, prompting for a name and member logins.
   - Note (don't auto-configure): a GitHub App or deploy key is also a valid
     bypass actor and is the only way to fully close the "bypass actor can
     still click merge" gap (see step 6) — point at this as an option for
     teams that want it, without building bot/App provisioning in v1.

5. **Build the ruleset, disabled, and show it before touching enforcement.**
   Same shape learned from qab:
   - `target: branch`, conditions matching the chosen branch(es).
   - Rules: `update` (the actual push restriction — blocks direct pushes
     *and* merge-button clicks for non-bypass actors), plus `deletion` and
     `non_fast_forward` for completeness.
   - Bypass: the team/actor chosen in step 4, `bypass_mode: always`.
   - Explicitly **not** the `pull_request` ("require a pull request before
     merging") rule — that would let non-bypass actors merge via an approved
     PR instead of being blocked, and would separately fight `hitch release`'s
     own push unless bypassed too.
   - Create with `enforcement: disabled`, print the equivalent of `gh ruleset
     view` for confirmation, and require an explicit confirm before a second
     call flips it to `active`. Never create directly into `active` — this
     two-step create-then-activate is the safety property that mattered most
     in the manual session (it's what let the exact rule/bypass JSON get
     eyeballed before it did anything).

6. **Disclose the residual gap, don't silently accept or silently fix it.**
   Print, plainly: bypass-listed accounts can still technically click "Merge"
   in the GitHub UI, because GitHub can't distinguish a CLI push from a
   merge-button push for the same identity. This is not something the command
   should try to paper over (disabling all PR merge methods isn't even
   possible — GitHub requires at least one enabled, and it wouldn't help
   bypass actors anyway). State the two real mitigations (bot/deploy-key
   bypass identity vs. accepted process discipline) and let the user decide,
   the same choice that came up for qab.

## Idempotency

Re-running `hitch setup rules` on an already-configured repo should detect
the existing team + ruleset and offer to review/adjust rather than duplicate.
This matters for onboarding UX specifically — someone re-running it to add a
teammate to the bypass list, or to point it at a second base branch, shouldn't
end up with two rulesets fighting each other via layering.

## Implementation notes

- **No new GitHub API dependency.** Everything here is achievable by shelling
  out to `gh api`, exactly as this session did by hand and as `hitch pr` /
  `hitch doctor` already do (`src/utils/gh.rs`). Extend that module with
  small typed helpers (list org teams, create team, add member, list/create/
  update rulesets, get branch protection) rather than pulling in an HTTP
  client + auth-token management for a second, parallel path to the same API.
- **Interactivity.** The existing `Confirm` trait (`src/utils/confirm.rs`) is
  narrowly scoped to one yes/no prompt (`confirm_force_push_rebuild`) and is
  mockable in tests (`AlwaysYesConfirm`/`AlwaysNoConfirm`). `hitch setup rules`
  needs several distinct confirmations and a "pick from a list" prompt (team
  selection, branch selection) — generalize `Confirm` into a small prompt
  utility (`confirm(prompt) -> bool`, `select(prompt, options) -> usize`)
  rather than adding more single-purpose methods to the existing trait, and
  keep it mockable the same way for tests.
- **Command shape.** `Commands::Setup(SetupCommand)` with its own
  `#[command(subcommand)]` enum (`Rules(...)`, room for more later). Read-only
  discovery steps (listing branches, teams, existing rulesets) don't need the
  repo-wide lock; the actual create/activate calls are the mutating part.
- **Testability.** The GitHub-side calls should go through a small trait
  (like `GitOperations` already does for git) so command logic can be unit
  tested against a fake, rather than only integration-tested against a real
  `gh`/GitHub API.

## Non-goals for v1

- Bot account / GitHub App / deploy-key *provisioning* — offer it as a bypass
  option if the user already has one, don't create one.
- Org-wide rulesets across many repos (Enterprise-only feature).
- Automatically touching repo-level PR merge-method settings.
- Custom repository roles as a bypass mechanism (Enterprise-only; not
  available on GitHub Team, which is what qab is on).

## Open questions

- Exact `gh` OAuth scopes needed to manage teams and rulesets via `gh api`
  (beyond `repo`) — `hitch doctor` may need a second check specific to
  `hitch setup rules`, separate from the `hitch pr` check it already does.
- Naming convention for the auto-created bypass team, so re-runs can reliably
  recognize "this is hitch's team for this repo" versus an unrelated
  similarly-named team.
- Whether `hitch setup rules` should also read/write anything into
  `hitch.json` (e.g. recording the ruleset id it created) so `hitch doctor`
  could later verify the GitHub-side state matches, not just the local `gh`
  setup.
