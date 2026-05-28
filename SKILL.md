# Hitch - Agent Skill

## Overview

**Hitch** is a Git branch management CLI tool for environment-based branch composition. It treats environment branches as generated outputs: promote feature or bug branches into an environment's list, then rebuild that environment branch from the base plus the promoted list. It is different from using Git directly because environment branches are regenerated from metadata instead of manually accumulating merges.

## When to Use Hitch

Use Hitch when the user asks to:
- Manage deployment environments (dev, qa, staging, production) using Git branches
- Promote or demote feature branches across environments
- Track which branches are deployed to which environments
- Lock/unlock environments to prevent deployments
- Require approvals for sensitive environment changes
- Rebuild environment branches from promoted branches
- View the status or hierarchy of environments and branches
- Clean up branches after release

## Core Concepts

### Environments as Composable Layers
Each environment branch is built on top of a base branch (usually `main`):
```
production = main + feature/auth
qa         = main + feature/auth + feature/payments
dev        = main + feature/auth + feature/payments + feature/ui
```

### Environment Branches as Generated Outputs
Hitch's core model is a base branch plus an ordered list of promoted branches. The environment branch is rebuilt from that list, so users can add and remove individual branches from an environment without treating the environment branch as hand-maintained merge history.

One possible workflow is:
1. Create a normal feature or bug branch from `main`.
2. Promote that branch to `dev`; the rebuilt `dev` branch triggers dev CI/CD.
3. If dev testing passes, promote the same feature branch to `qa`; the rebuilt `qa` branch triggers QA CI/CD.
4. If QA passes, release to `main` or production; main CI/CD deploys it.

Other teams may not use `release` this way. The important model is explicit branch stacking, not a prescribed deployment process.

### Key Principles
- Environment branches are **regenerated**, not manually merged
- Promoting adds a branch to an environment's list
- Demoting removes a branch from an environment's list
- `rebuild` reconstructs the environment branch from all promoted branches
- `release` merges the promoted branches from an environment into a target branch, for teams that want that workflow
- Configuration is stored in `hitch.json` on a `hitch-metadata` branch
- Hitch is not just a wrapper around normal merges: it stores the desired environment contents, then regenerates the branch
- Merge fixes remain real Git work: when branches do not compose cleanly, users may need to carry fixes back into the feature branch or repeat equivalent fixes elsewhere

## Common Commands

### Setup
```bash
hitch init                              # Initialize Hitch in a Git repo
hitch init --environments dev,qa,production  # Init with environments
hitch add dev --base main               # Add an environment
hitch remove dev                        # Remove an environment
```

### Promote/Demote
```bash
hitch promote feature/auth dev          # Promote branch to environment
hitch promote feature/auth dev --no-rebuild  # Skip rebuild (batch mode)
hitch demote feature/auth dev           # Remove branch from environment
```

### Rebuild & Release
```bash
hitch rebuild production                # Rebuild environment from promoted branches
hitch release production main           # Release environment to target branch
hitch diff                              # Preview commits each branch would add
```

### Status & Inspection
```bash
hitch status                            # Show status of all environments
hitch status --verbose                  # Detailed status
hitch status --diff                     # Show config changes
hitch tree                              # Show branch hierarchy
```

### Lock/Unlock
```bash
hitch lock production                   # Lock environment (freeze deployments)
hitch unlock production                 # Unlock environment
```

### Configuration
```bash
hitch set dev --base main               # Change base branch
hitch set production --requires-approval true
hitch set production --min-approvals 2
hitch set production --add-approver alice @example.com
hitch set production --remove-approver bob @example.com
```

### Approvals
```bash
hitch approvals list                    # List all approval requests
hitch approvals list --status pending   # Filter by status
hitch approvals approve <request-id> "Ready for production"
hitch approvals reject <request-id> "Not ready yet"
hitch approvals cancel <request-id>
```

### Maintenance
```bash
hitch guard                             # Guard env branches from direct commits
hitch cleanup                             # Remove local branches no longer promoted
hitch completion bash > /etc/bash_completion.d/hitch  # Shell completions
```

## Global Flags
- `--verbose` - Print detailed step-by-step logs
- `--no-push` - Skip automatic pushes when metadata is committed

## Approval Workflow

For sensitive environments (e.g., production), Hitch supports a multi-person approval workflow:

1. Configure approval requirements:
   ```bash
   hitch set production --requires-approval true
   hitch set production --min-approvals 2
   hitch set production --add-approver alice @example.com
   ```

2. When promoting to a protected environment, Hitch creates an approval request instead of executing immediately.

3. Approvers review and approve:
   ```bash
   hitch approvals approve <request-id> "Looks good"
   ```

4. Once threshold is met, the promotion is applied automatically.

## Configuration File (hitch.json)

Hitch stores configuration in `hitch.json` on the `hitch-metadata` branch:

```json
{
  "version": "1.0",
  "environments": {
    "production": {
      "base": "main",
      "branches": ["feature/auth"],
      "locked": false,
      "requires_approval": true,
      "min_approvals": 2,
      "approvers": ["alice @example.com", "bob @example.com"]
    }
  },
  "approval_requests": []
}
```

## Important Notes for Agents

- **Always run commands from the Git repository root** where Hitch is initialized
- The `hitch-metadata` branch stores configuration — do not edit it manually
- Environment branches are rebuilt automatically — do not merge into them manually
- Use `--no-rebuild` flag when batching multiple promotions
- Locked environments reject all promotion/demotion attempts
- After `hitch release`, promoted branches may already be merged — use `hitch cleanup` to remove them
- Use `hitch diff` before rebuild to preview what commits would be added
- `hitch rebuild` is all-or-nothing: it preflights compatibility first and refuses (cleanly) if a promoted branch can’t be assembled
- Fix guidance: `git checkout <branch> && git rebase <base>`
- `hitch rebuild` requires a clean working tree (commit or stash first)
- Document Hitch as explicit branch stacking, not as a required release process or as a replacement for conflict management. The main downside is repeated merge fixes across environments unless resolved changes are carried back into the feature branch.

## Installation

```bash
cargo install hitch
# or
brew install doomedramen/homebrew-hitch/hitch
```

## Repository

Source: https://github.com/doomedramen/hitch
