# Plan: `hitch setup` command family

Status: **proposed** (not yet implemented)

## Motivation

Onboarding a new repo onto hitch's GitHub PR workflow (see
[`github-pr-workflow-plan.md`](github-pr-workflow-plan.md)) currently means a
human doing a sequence of `gh api` calls by hand: work out which branches need
protecting, discover that a single person can't be a ruleset bypass actor,
create a team, build a ruleset in the right shape, sanity-check it, then flip
it live.

`hitch setup` automates this — plus, with a public GitHub App, closes the
residual gap where bypass-listed humans could still click "Merge" in the UI.

## Architecture: public GitHub App "Hitch"

**One public GitHub App, installed per-repo, reused by all hitch users.**

- **App identity**: a public GitHub App called "Hitch", created once and
  installed on any repo via the GitHub apps directory or direct install URL.
- **Permissions**: Contents: Write, Metadata: Read — the minimum needed to
  push to protected branches.
- **Client ID**: public, compiled into the hitch binary.
- **Private key**: held by a minimal backend service (Cloudflare Worker or
  similar). Never distributed — the CLI never sees it.
- **Bypass**: the ruleset on `main` (and any other protected branches) bypasses
  the Hitch GitHub App. Since the app is a non-human identity and only
  `hitch release` pushes as the app, no human can click merge in the UI — the
  residual gap is closed.

### Token flow

```
hitch setup                              hitch release
    │                                        │
    ├─ OAuth device flow ──────────────────► │
    │  (GitHub authorizes user)              │
    │                                        │
    ├─ Open browser: install app on repo ──► │
    │                                        │
    ├─ Exchange OAuth token ──► Backend ──►  ├─ Request installation token ──► Backend
    │  for setup config         (Worker)     │  (identifies self via setup config)
    │  ◄── setup config (signed)              │  ◄── installation token (1hr, JWT-signed)
    │                                        │
    ├─ Store config locally                  ├─ GIT_ASKPASS="hitch askpass"
    │  (~/.config/hitch/<repo>.json)         │  git push ──► uses token
    │                                        │
    ├─ Create ruleset (gh api)               │
    │  bypass: Hitch GitHub App              │
    └─ Activate                              └─
```

### Token backend

Single endpoint on a serverless function (e.g. Cloudflare Worker):

```
POST /token
  Body: { "setup_token": "<signed-setup-payload>" }
  → Verifies the setup token is valid (HMAC-signed by the same backend)
  → Signs a JWT with the Hitch app's private key
  → POST /app/installations/{id}/access_tokens (GitHub API)
  → Returns { "token": "<installation-access-token>", "expires_at": "..." }
```

The setup token is what the CLI stores locally after `hitch setup` completes.
It contains the installation ID and repo identity, signed by the backend so it
can't be forged. The CLI sends it to the backend on every `hitch release` to get
a fresh installation token.

No user data is stored on the backend. It's stateless — every request carries
its own proof. The only persistent state is the GitHub App's private key
(environment variable in the Worker).

## `hitch setup` command

Top-level namespace for one-time repo configuration:

```
hitch setup
```

Interactive, single command (no subcommands for v1):

### 1. Preconditions

- Hitch is initialized (`hitch-metadata` exists).
- `gh` is installed and authenticated (reuse `hitch doctor` checks).

### 2. Discover and select branches to protect

- Read `hitch.json`. Collect distinct `base` values across all environments
  (e.g. `main`). These are pre-checked.
- List all branches in the repo. Pre-checked bases appear at the top.
- User can check/uncheck any branch to protect.
- All checked branches get the same ruleset (block all pushes, bypass = Hitch app).

### 3. GitHub App authentication

- Open browser to the Hitch app's installation URL:
  `https://github.com/apps/hitch/installations/new`
- User installs the app on the repo (or selects repos if org-wide).
- After installation, redirect to a hitch-owned page that displays a one-time
  setup code.
- User pastes the setup code back into the CLI.
- CLI exchanges the code for a signed setup token via the backend.
- CLI stores the setup token in `~/.config/hitch/<repo-hash>.json`.

(Alternative: OAuth device flow. CLI starts device flow, user opens URL and
enters code, then installs the app. Backend correlates the OAuth grant with
the app installation. Simpler UX but requires the backend to track pending
authorizations for a short window. TBD which flow is cleaner — both are
standard GitHub patterns.)

### 4. Check existing GitHub state

- Existing classic branch protection on each selected branch: surface it.
- Existing rulesets targeting the branch: if one already looks like this
  pattern, offer to view/adjust rather than create a duplicate.

### 5. Build the ruleset, disabled

- `target: branch`, conditions matching the checked branch(es).
- Rules: `update` (blocks all pushes including merge-button clicks), plus
  `deletion` and `non_fast_forward`.
- Bypass: the Hitch GitHub App, `bypass_mode: always`.
- No `pull_request` rule.
- Create with `enforcement: disabled`. Print for confirmation.
- On explicit confirm, flip to `active`.

### 6. No residual gap

Since the bypass actor is a non-human GitHub App, and only `hitch release`
pushes as the app, no human can click merge in the UI. No caveat to disclose.

## `hitch release` changes

When pushing to a protected branch, `hitch release` authenticates as the
Hitch GitHub App:

1. Read setup token from `~/.config/hitch/<repo-hash>.json`.
2. POST to the token backend to get a fresh installation token.
3. Set `GIT_ASKPASS` to a hitch-provided helper that prints the token.
4. Push as normal (`git push origin <branch>`).
5. If the push URL is SSH, rewrite it to HTTPS for this push (the token only
   works over HTTPS).

Token caching: the installation token is valid for 1 hour. `hitch release` could
cache it in memory for the session, but since `hitch release` is a single
invocation, the simplest thing is to fetch a fresh token each time. The backend
call is a single HTTP round-trip.

Fallback: if the backend is unreachable or no setup token exists, fall back to
the user's normal git credentials (with a warning that the push may be blocked
by the ruleset).

## `hitch pr` changes

No changes needed. `hitch pr` creates PRs targeting protected branches, which
is a read-only operation (zero risk). It continues to use the user's `gh` CLI
auth.

## Idempotency

Re-running `hitch setup` on an already-configured repo should:
- Detect existing rulesets targeting the same branches.
- Offer to view/adjust rather than create duplicates.
- Re-authenticate if the setup token is missing or expired.
- Add/remove protected branches without recreating the ruleset from scratch.

## Implementation plan

### Phase 1: GitHub App + backend (me, outside this repo)

- Create the Hitch GitHub App in my GitHub account / org.
  - Name: "Hitch"
  - Callback URL: `https://hitch.dev/callback` (or similar)
  - Permissions: Contents: Write, Metadata: Read
  - Webhook: none (no events needed)
  - Request user authorization (OAuth) during installation: yes
- Deploy the token backend (Cloudflare Worker).
  - `/token` — exchanges setup code/token for installation token.
  - `/setup` — handles the OAuth callback, correlates installation, returns
    setup code to user.
- Test end-to-end with a test repo.

### Phase 2: `hitch setup` (CLI changes)

- [ ] `Commands::Setup(SetupCommand)` in `src/cli.rs`.
- [ ] `src/commands/setup.rs` — main setup logic.
- [ ] `src/utils/setup.rs` — shared helpers (config store, token exchange, etc.).
- [ ] Branch discovery from `hitch.json` + `git branch`.
- [ ] Interactive branch selection (extend `src/utils/confirm.rs`).
- [ ] OAuth / installation flow (open browser, accept pasted code).
- [ ] Store setup token locally (`~/.config/hitch/`).
- [ ] Ruleset creation via `gh api` (extend `src/utils/gh.rs`).
- [ ] Ruleset activation (two-step: create disabled, then activate on confirm).
- [ ] Idempotency: detect existing setup, offer to adjust.

### Phase 3: `hitch release` changes

- [ ] Load setup token from local config.
- [ ] Call backend for installation token.
- [ ] Build `GIT_ASKPASS` helper (or `git -c credential.helper=`).
- [ ] Handle SSH → HTTPS URL rewrite for the push.
- [ ] Fallback to user credentials if backend is down.
- [ ] Warn if pushing to a protected branch without setup (will be blocked).

### Phase 4: polish

- [ ] `hitch doctor` — add checks for setup token validity, backend reachability,
  ruleset existence.
- [ ] `hitch setup --check` — non-interactive mode that verifies the current
  setup is correct (for CI / onboarding scripts).
- [ ] `hitch setup --remove` — tear down rulesets and remove local config.

## Out of scope for v1

- Self-hosted backend (for users who want to run their own token service).
- Enterprise GitHub Server support.
- Deploy key or user-team as alternative bypass actors (the app covers it).
- Multi-repo / org-level setup in one command.

## Open questions

- **OAuth device flow vs. web callback flow**: Device flow is simpler (user
  enters code from terminal into browser), but GitHub App installation is
  typically a web redirect flow. Can we chain them? Device flow authorizes the
  OAuth app → backend gets user token → backend checks if the app is installed →
  if not, redirects user to installation page → after install, backend
  correlates via the in-progress device flow session.
- **Token storage location**: `~/.config/hitch/<sha256(repo-url)>.json` —
  portable, no dependency on keychain crates. Or use the OS keychain via
  `security` (macOS), `secret-tool` (Linux), etc. — more secure but platform-
  specific.
- **Setup token expiry**: Should it expire? If so, the user re-runs `hitch setup`
  to get a new one. The installation itself doesn't expire. TTL of 90 days?
  Infinite? Trade-off between security (token theft window) and UX (re-auth
  friction).
- **Backend domain**: Need a domain for the OAuth callback + token endpoint.
  `hitch.dev`? Hosted on the same gh-pages as the hitch site, with the Worker
  at `api.hitch.dev`?
