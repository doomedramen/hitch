# Hitch Command Reference

Complete reference for all Hitch CLI commands.

## Global Flags

All commands support these flags:

- `--help`, `-h` - Show help for command
- `--version`, `-v` - Show Hitch version
- `--verbose` - Enable verbose output
- `--quiet`, `-q` - Suppress non-error output
- `--no-color` - Disable colored output

## Important Guarantees

**Hitch always returns you to your original branch:**
- No matter what operation runs, you'll end up on the same branch you started on
- Works even if the command fails or is interrupted
- Preserves detached HEAD state if that's where you were
- Your uncommitted changes are never touched

**Example:**
```bash
$ git branch
* feature/my-work

$ hitch promote feature/other to dev
# ... hitch does work on hitch-metadata, main, dev branches ...

$ git branch
* feature/my-work  # ← Back where you started!
```

## Commands

### `hitch init`

Initialize Hitch in the current Git repository.

```bash
hitch init [flags]
```

**What it does:**
1. Verifies current directory is a Git repository
2. Creates `hitch-metadata` orphan branch
3. Writes initial `hitch.json` with default configuration
4. Pushes metadata branch to remote (unless `--no-push` specified)
5. Returns to your original branch

**Flags:**
- `--environments <list>` - Comma-separated list of environments (default: "dev,qa")
- `--base <branch>` - Base branch name (default: "main")
- `--retention-days <int>` - Days to keep branches after merge (default: 7)
- `--stale-days <int>` - Days before warning about inactive branches (default: 30)
- `--no-push` - Don't push hitch-metadata to remote (local only)

**Example:**
```bash
# Initialize with defaults
hitch init

# Initialize with custom environments
hitch init --environments dev,staging,qa,prod --base main

# Initialize without pushing to remote
hitch init --no-push
```

**Output (with default push):**
```
✓ Hitch initialized successfully
✓ Pushed hitch-metadata to origin

Environments configured: dev, qa
Base branch: main

Next steps:
  1. Create a feature branch: git checkout -b feature/my-feature
  2. Promote to dev: hitch promote feature/my-feature to dev
  3. Check status: hitch status
```

**Output (with --no-push):**
```
✓ Hitch initialized successfully
ℹ Skipped push to remote (--no-push specified)
To push later, run:
  git push -u origin hitch-metadata

Environments configured: dev, qa
Base branch: main

Next steps:
  1. Create a feature branch: git checkout -b feature/my-feature
  2. Promote to dev: hitch promote feature/my-feature to dev
  3. Check status: hitch status
```

---

### `hitch status`

Show current state of all environments and branches.

```bash
hitch status [flags]
```

**What it does:**
1. Reads metadata from `hitch-metadata` branch
2. Displays which features are in each environment
3. Shows lock status
4. Optionally shows stale branches

**Flags:**
- `--stale` - Include stale branch analysis
- `--json` - Output as JSON
- `--env <name>` - Show only specific environment

**Example:**
```bash
# Basic status
hitch status

# Show stale branches
hitch status --stale

# JSON output for scripting
hitch status --json
```

**Output:**
```
Hitch Status

Environment: dev (unlocked)
  Base: main
  Features:
    - feature/user-auth (promoted 2 days ago)
    - feature/dashboard (promoted 1 day ago)
    - bug/fix-login (promoted 3 hours ago)

Environment: qa (locked by dev-m@example.com since 10:30:00)
  Base: main
  Features:
    - feature/user-auth (promoted 2 days ago)
    - feature/dashboard (promoted 5 hours ago)

Main branch: 45 commits ahead of last rebuild

Stale branches (use --stale for details):
  2 branches safe to delete
  1 branch with no recent activity
```

---

### `hitch promote`

Add a feature branch to an environment.

```bash
hitch promote <branch> to <environment> [flags]
```

**What it does:**
1. Validates branch exists
2. Acquires lock on environment
3. Adds branch to environment's feature list
4. Rebuilds environment from base + all features (using safe temp branch)
5. Force-pushes rebuilt hitched branch
6. Updates metadata
7. Releases lock
8. Returns you to your original branch

**Safety:** Uses temporary branch for rebuild - original environment preserved until success!

**Flags:**
- `--no-rebuild` - Add to metadata but don't rebuild (manual rebuild later)
- `--strategy <merge|rebase>` - Merge strategy (default: merge)

**Example:**
```bash
# Promote to dev
hitch promote feature/user-auth to dev

# Promote to qa
hitch promote feature/user-auth to qa

# Add to metadata but don't rebuild yet
hitch promote feature/dashboard to dev --no-rebuild
```

**Output:**
```
Promoting feature/user-auth to dev...

✓ Locked dev environment
✓ Added feature/user-auth to dev feature list
✓ Rebuilding dev environment...
  - Checked out main (commit: a1b2c3d)
  - Merged feature/user-auth (no conflicts)
  - Merged feature/dashboard (no conflicts)
✓ Pushed dev branch to remote
✓ Updated metadata
✓ Unlocked dev environment

Success! feature/user-auth is now in dev

View deployment: https://dev.example.org
```

**Error handling:**
```bash
# Merge conflict
Error: Merge conflict when adding feature/user-auth to dev

feature/user-auth conflicts with the current dev environment.

Conflicting files:
  - src/auth/login.js
  - src/components/Header.tsx

To resolve:
  1. git checkout feature/user-auth
  2. git rebase main
  3. Resolve conflicts and continue rebase
  4. git push --force-with-lease
  5. hitch promote feature/user-auth to dev

# Environment locked
Error: dev environment is locked by dev-s@example.com (since 10:25:00)

Wait for the lock to release or use: hitch unlock dev --force
```

---

### `hitch demote`

Remove a feature branch from an environment.

```bash
hitch demote <branch> from <environment> [flags]
```

**What it does:**
1. Acquires lock on environment
2. Removes branch from environment's feature list
3. Rebuilds environment without that branch
4. Force-pushes rebuilt hitched branch
5. Updates metadata
6. Releases lock

**Flags:**
- `--no-rebuild` - Remove from metadata but don't rebuild

**Example:**
```bash
# Remove from dev
hitch demote feature/user-auth from dev

# Remove from qa
hitch demote feature/user-auth from qa
```

**Output:**
```
Demoting feature/user-auth from dev...

✓ Locked dev environment
✓ Removed feature/user-auth from dev feature list
✓ Rebuilding dev environment...
✓ Pushed dev branch to remote
✓ Updated metadata
✓ Unlocked dev environment

Success! feature/user-auth is no longer in dev
```

---

### `hitch release`

Merge a feature branch to the base branch (typically `main`).

```bash
hitch release <branch> [flags]
```

**What it does:**
1. Validates branch is in at least one environment (safety check)
2. Merges branch into base branch (main)
3. Pushes base branch to remote
4. Removes branch from all environments
5. Records merge timestamp in metadata
6. Optionally deletes branch after retention period

**Flags:**
- `--no-delete` - Don't delete branch after merge (default: false, branch marked for cleanup)
- `--message <text>` - Custom merge commit message
- `--squash` - Squash commits before merging

**Example:**
```bash
# Release to main
hitch release feature/user-auth

# Release with custom message
hitch release feature/user-auth --message "Add OAuth authentication"

# Release and squash commits
hitch release feature/user-auth --squash
```

**Output:**
```
Releasing feature/user-auth to main...

✓ Validated feature/user-auth is in qa environment
✓ Checked out main
✓ Merged feature/user-auth into main
✓ Pushed main to remote
✓ Removed feature/user-auth from all environments
✓ Updated metadata (marked merged_to_main_at)

Success! feature/user-auth is now in main

The branch will be eligible for cleanup in 7 days.
Use 'hitch cleanup' to delete stale branches.
```

---

### `hitch rebuild`

Rebuild an environment from scratch.

```bash
hitch rebuild <environment> [flags]
```

**What it does:**
1. Acquires lock on environment
2. Checks out fresh base branch (main)
3. Creates temporary branch (e.g., `dev-hitch-temp`)
4. Merges all features into temp branch
5. **Only if ALL merges succeed:** swaps temp branch to become the new hitched branch
6. Force-pushes rebuilt hitched branch
7. Releases lock
8. Returns you to your original branch

**Safety (always enabled):**
- Original hitched branch is **never touched** until rebuild succeeds
- If ANY merge fails, temp branch is deleted and original is preserved
- This is the ONLY way Hitch rebuilds - there is no "unsafe mode"

**Flags:**
- `--dry-run` - Simulate rebuild without making changes
- `--force` - Rebuild even if environment is locked

**Example:**
```bash
# Rebuild dev
hitch rebuild dev

# Preview rebuild without making changes
hitch rebuild dev --dry-run

# Force rebuild qa (bypass lock)
hitch rebuild qa --force
```

**Output:**
```
Rebuilding dev environment...

✓ Locked dev environment
✓ Checked out main (commit: a1b2c3d)
✓ Created temp branch: dev-hitch-temp
✓ Merging features into temp branch:
  - feature/user-auth (no conflicts)
  - feature/dashboard (no conflicts)
  - bug/fix-login (no conflicts)
✓ All merges successful
✓ Swapped dev-hitch-temp → dev
✓ Pushed dev branch to remote
✓ Unlocked dev environment

Success! dev environment rebuilt with 3 features
```

**Dry run output:**
```bash
$ hitch rebuild dev --dry-run

Dry run: simulating rebuild of dev environment

✓ Would checkout main (current commit: a1b2c3d)
✓ Would create temp branch: dev-hitch-temp
✓ Checking if features are mergeable:
  - feature/user-auth (mergeable, no conflicts predicted)
  - feature/dashboard (mergeable, no conflicts predicted)
  - bug/fix-login (mergeable, no conflicts predicted)
✓ Would swap dev-hitch-temp → dev
✓ Would push dev branch to remote

Dry run complete. No branches created, no changes made.
Run without --dry-run to apply changes.
```

**Note:** `--dry-run` doesn't create any branches or make any changes. It only analyzes mergeability.

**Error handling:**
```bash
# Merge conflict during rebuild
$ hitch rebuild dev
Error: Merge conflict when rebuilding dev environment

feature/dashboard conflicts with existing features.

Conflicting files:
  - src/components/Dashboard.tsx

The original dev branch is unchanged.
Temp branch dev-hitch-temp has been deleted.

To resolve:
  1. git checkout feature/dashboard
  2. git rebase main
  3. Resolve conflicts
  4. git push --force-with-lease
  5. hitch rebuild dev
```

---

### `hitch cleanup`

Delete stale branches.

```bash
hitch cleanup [flags]
```

**What it does:**
1. Scans all tracked branches
2. Identifies branches safe to delete:
   - Merged to main
   - Past retention period
   - Not in any environment
3. Optionally identifies inactive branches:
   - No commits for > X days
   - Not merged to main
4. Prompts for confirmation (unless `--yes`)
5. Deletes branches locally and remotely
6. Removes from metadata

**Flags:**
- `--dry-run` - Show what would be deleted without deleting
- `--yes`, `-y` - Skip confirmation prompts
- `--local-only` - Only delete local branches
- `--remote-only` - Only delete remote branches
- `--include-inactive` - Also prompt to delete inactive branches

**Example:**
```bash
# Preview cleanup
hitch cleanup --dry-run

# Clean up with confirmation
hitch cleanup

# Clean up without confirmation
hitch cleanup --yes

# Include inactive branches in cleanup
hitch cleanup --include-inactive
```

**Output:**
```
Scanning for stale branches...

Branches safe to delete (merged to main > 7 days ago):
  ✓ feature/user-auth (merged 10 days ago)
  ✓ bug/fix-login (merged 8 days ago)

Inactive branches (no commits for > 30 days):
  ? feature/abandoned-idea (last commit 45 days ago, not merged)

Delete 2 stale branches? (y/N): y

Deleting branches...
  ✓ Deleted feature/user-auth (local and remote)
  ✓ Deleted bug/fix-login (local and remote)
  ✓ Updated metadata

Success! Cleaned up 2 branches

To review inactive branches, run: hitch cleanup --include-inactive
```

---

### `hitch lock`

Manually lock an environment.

```bash
hitch lock <environment> [flags]
```

**What it does:**
1. Writes lock status to metadata
2. Records who locked and when

**Use cases:**
- Prevent deployments during incident
- Hold environment for testing
- Emergency freeze

**Example:**
```bash
# Lock qa
hitch lock qa

# Lock with reason
hitch lock qa --reason "Investigating production bug"
```

**Output:**
```
✓ Locked qa environment

Locked by: dev-m@example.com
Locked at: 2025-10-16 10:30:00

To unlock: hitch unlock qa
```

---

### `hitch unlock`

Manually unlock an environment.

```bash
hitch unlock <environment> [flags]
```

**What it does:**
1. Removes lock status from metadata

**Flags:**
- `--force` - Unlock even if locked by another user

**Example:**
```bash
# Unlock qa
hitch unlock qa

# Force unlock (override another user's lock)
hitch unlock qa --force
```

**Output:**
```
✓ Unlocked qa environment

Previously locked by: dev-m@example.com
Previously locked at: 2025-10-16 10:30:00
Lock duration: 5 minutes
```

---

### `hitch list`

> **Coming Soon** - This command is planned for a future release.

List all tracked branches and their status.

```bash
hitch list [flags]
```

**What it does:**
1. Reads metadata
2. Displays all branches with:
   - Creation date
   - Environments it's in
   - Merge status
   - Last activity

**Flags:**
- `--environment <name>` - Filter by environment
- `--merged` - Show only merged branches
- `--unmerged` - Show only unmerged branches
- `--json` - Output as JSON

**Example:**
```bash
# List all branches
hitch list

# List only dev branches
hitch list --environment dev

# List unmerged branches
hitch list --unmerged
```

**Output:**
```
Tracked Branches

feature/user-auth
  Created: 2 days ago
  Environments: dev, qa
  Status: Unmerged
  Last commit: 5 hours ago

feature/dashboard
  Created: 1 day ago
  Environments: dev
  Status: Unmerged
  Last commit: 3 hours ago

bug/fix-login
  Created: 10 days ago
  Environments: none
  Status: Merged to main (8 days ago)
  Last commit: 8 days ago

Total: 3 branches (2 active, 1 merged)
```

---

### `hitch config`

> **Coming Soon** - This command is planned for a future release.

View or modify Hitch configuration.

```bash
hitch config [subcommand] [flags]
```

**Subcommands:**
- `show` - Display current configuration
- `set <key> <value>` - Set configuration value
- `get <key>` - Get configuration value

**Configurable keys:**
- `retention_days_after_merge` - Days to keep branches after merge
- `stale_days_no_activity` - Days before warning about inactive branches
- `base_branch` - Base branch name (main, master, etc.)
- `environments` - List of environments

**Example:**
```bash
# Show configuration
hitch config show

# Change retention period
hitch config set retention_days_after_merge 14

# Add new environment
hitch config set environments dev,staging,qa,prod
```

**Output:**
```
Hitch Configuration

retention_days_after_merge: 7
stale_days_no_activity: 30
base_branch: main
environments: [dev, qa]
```

---

### `hitch hook`

Git hook integration commands for use in your Git hooks.

```bash
hitch hook <hook-name>
```

**Available hooks:**
- `pre-push` - Check if current branch is safe to push

**Purpose:**
These commands are designed to be called from Git hooks (`.git/hooks/`).
They provide exit codes suitable for hook integration.

**Exit codes:**
- `0` - Safe to proceed
- `1` - Blocked (hook should abort operation)

---

#### `hitch hook pre-push`

Check if the current branch can be safely pushed.

**Checks:**
1. Is current branch a managed environment (dev, qa)?
2. Is the environment locked?
3. If locked, am I the lock holder?

**Usage in `.git/hooks/pre-push`:**
```bash
#!/bin/bash

# Your existing checks
npm run lint || exit 1

# Add Hitch check
hitch hook pre-push || exit 1
```

**Output when blocked:**
```bash
$ hitch hook pre-push
❌ Cannot push to dev

Locked by: dev-m@example.com
Locked at: 2025-10-16 10:30:00

Wait for unlock or contact dev-m@example.com
# Exit code: 1
```

**Output when allowed:**
```bash
$ hitch hook pre-push
# (silent success)
# Exit code: 0
```

**Output when warning:**
```bash
$ hitch hook pre-push
⚠️  Pushing directly to hitched branch dev
This may be overwritten by: hitch rebuild dev
# Exit code: 0 (allows push but warns)
```

**Integration examples:**

See [HOOKS.md](./HOOKS.md) for complete integration guide including:
- Husky
- Lefthook
- pre-commit framework
- Manual Git hooks
- GitHub Actions

**Quick setup:**
```bash
# Create pre-push hook
cat > .git/hooks/pre-push <<'EOF'
#!/bin/bash
hitch hook pre-push || exit 1
EOF

# Make executable
chmod +x .git/hooks/pre-push

# Test
git push origin your-branch
```

---

### `hitch version`

Show Hitch version information.

```bash
hitch version
```

**Output:**
```
Hitch v1.0.0
Build: a1b2c3d
Go version: go1.22.0
OS/Arch: darwin/arm64
```

---

## Exit Codes

- `0` - Success
- `1` - General error
- `2` - Command line usage error
- `3` - Merge conflict
- `4` - Environment locked
- `5` - Branch not found
- `10` - Metadata error

## Environment Variables

- `HITCH_NO_COLOR=1` - Disable colored output
- `HITCH_VERBOSE=1` - Enable verbose logging
- `HITCH_CONFIG_PATH` - Custom path to config (overrides metadata)

## Examples

### Full Feature Workflow

```bash
# Create feature branch
git checkout -b feature/new-login
# ... work on feature ...
git commit -am "Implement new login"
git push origin feature/new-login

# Deploy to dev
hitch promote feature/new-login to dev

# Test in dev, looks good

# Deploy to qa
hitch promote feature/new-login to qa

# QA approval, release to production
hitch release feature/new-login

# Clean up after retention period
hitch cleanup
```

### Emergency Hotfix

```bash
# Create hotfix branch
git checkout main
git checkout -b hotfix/critical-bug
# ... fix bug ...
git commit -am "Fix critical bug"
git push origin hotfix/critical-bug

# Skip dev, go straight to qa
hitch promote hotfix/critical-bug to qa

# Quick test, release immediately
hitch release hotfix/critical-bug

# Rebuild dev to include hotfix
hitch rebuild dev
```

### Removing a Problematic Feature

```bash
# Feature causes issues in qa
hitch demote feature/broken-thing from qa

# Fix the feature
git checkout feature/broken-thing
# ... make fixes ...
git commit -am "Fix issues"
git push

# Re-promote
hitch promote feature/broken-thing to qa
```
