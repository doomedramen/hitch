# Hitch - Agent Skill

## Overview

**Hitch** is a Git branch management CLI tool for environment-based deployments. It treats environment branches as composable layers — promote feature branches to environments, and Hitch rebuilds the environment branch by squashing them together.

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

### Key Principles
- Environment branches are **regenerated**, not manually merged
- Promoting adds a branch to an environment's list
- Demoting removes a branch from an environment's list
- `rebuild` reconstructs the environment branch from all promoted branches
- Configuration is stored in `hitch.json` on a `hitch-metadata` branch

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
hitch resolve                           # Show rebuild conflict status (if paused)
hitch resolve --continue                # Finish rebuild after you resolve + stage conflicts
hitch resolve --abort                   # Abort paused rebuild and restore original branch
hitch release production main           # Release environment to target branch
hitch diff                              # Preview commits each branch would add
```

### Shared Conflict Resolutions (rerere)
Hitch can share rebuild conflict resolutions across clones by exporting/importing Git’s
`rerere` cache (`.git/rr-cache`) into `hitch-metadata`.

```bash
hitch rebuild dev --reuse-resolutions   # Import shared rr-cache, enable rerere for the run

hitch resolutions status                # Show shared cache size + top prune candidates
hitch resolutions prune                 # Prune shared cache down to 200MB cap (manual only)
hitch resolutions prune --max-size 50   # Prune down to 50MB
```

Notes:
- Safety: rerere only reapplies a resolution when the exact conflict “preimage” matches.
- Sharing depends on `hitch-metadata`: without `--no-push`, Hitch will push updates so other
  clones can reuse them. With `--no-push`, exports stay local.
- Storage in `hitch-metadata`: `hitch/rr-cache/entries/<id>/...` + `hitch/rr-cache/index.json`.

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
- Use `hitch resolve` / `--continue` / `--abort` to manage a rebuild paused by merge conflicts
- Use `hitch rebuild <env> --reuse-resolutions` to reuse shared conflict resolutions (rerere)
- Use `hitch resolutions prune` only when you explicitly want to cap shared cache size

## Installation

```bash
cargo install hitch
# or
brew install doomedramen/homebrew-hitch/hitch
```

## Repository

Source: https://github.com/doomedramen/hitch
