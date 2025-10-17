# Example 1: Basic Workflow

**Duration:** 10 minutes
**Difficulty:** Beginner

## Overview

This example walks you through the fundamental Hitch workflow:
1. Initialize Hitch in a repository
2. Create and promote a feature
3. Release to production
4. Clean up merged branches

## Prerequisites

- Hitch installed (`hitch --version`)
- Git repository (or create a new one)
- Basic Git knowledge

## Setup

Let's create a test repository:

```bash
# Create new repository
mkdir my-project
cd my-project
git init

# Create initial commit
echo "# My Project" > README.md
git add README.md
git commit -m "Initial commit"

# Create main branch (if not already)
git branch -M main
```

## Step 1: Initialize Hitch

Initialize Hitch with default environments (dev and qa):

```bash
hitch init
```

**Expected output:**
```
✓ Initialized Hitch metadata
✓ Created environment: dev (base: main)
✓ Created environment: qa (base: main)
✓ Metadata committed to hitch-metadata branch
✓ Pushed to origin/hitch-metadata

Hitch is now initialized! 🎉

Environments configured:
  - dev (base: main)
  - qa (base: main)

Next steps:
  1. Create a feature branch: git checkout -b feature/your-feature
  2. Promote to dev: hitch promote feature/your-feature to dev
  3. Check status: hitch status
```

**What happened:**
- Created `hitch-metadata` orphan branch
- Stored configuration in `hitch.json`
- Set up dev and qa environments

## Step 2: Create a Feature

Create a new feature branch:

```bash
# Create and switch to feature branch
git checkout -b feature/add-login

# Make some changes
echo "Login functionality" > login.js
git add login.js
git commit -m "Add login functionality"

# Push to remote (creates remote branch)
git push origin feature/add-login
```

## Step 3: Promote to Dev

Add your feature to the dev environment:

```bash
hitch promote feature/add-login to dev
```

**Expected output:**
```
Promoting feature/add-login to dev...

✓ Acquired lock on dev
✓ Checked out base branch: main
✓ Created temp branch: dev-hitch-temp
✓ Merged feature/add-login (no conflicts)
✓ All merges successful
✓ Swapped dev-hitch-temp → dev
✓ Pushed dev branch to remote
✓ Updated metadata

Success! feature/add-login is now in dev environment

Current dev features:
  - feature/add-login
```

**What happened:**
- Locked dev environment
- Created fresh branch from main
- Merged your feature
- Force-pushed to origin/dev
- Updated metadata

## Step 4: Check Status

View the current state:

```bash
hitch status
```

**Expected output:**
```
Environment: dev (base: main)
  Features (1):
    ✓ feature/add-login

Environment: qa (base: main)
  Features (0):
    (empty)

Tracked Branches (1):
  feature/add-login
    Created: 2025-10-17 14:30:00
    Promoted to: dev
    Status: Active
```

## Step 5: Promote to QA

After testing in dev, promote to qa:

```bash
hitch promote feature/add-login to qa
```

**Expected output:**
```
Promoting feature/add-login to qa...

✓ Acquired lock on qa
✓ Checked out base branch: main
✓ Created temp branch: qa-hitch-temp
✓ Merged feature/add-login (no conflicts)
✓ All merges successful
✓ Swapped qa-hitch-temp → qa
✓ Pushed qa branch to remote
✓ Updated metadata

Success! feature/add-login is now in qa environment

Current qa features:
  - feature/add-login
```

## Step 6: Release to Main

After QA approval, release to production:

```bash
hitch release feature/add-login
```

**Expected output:**
```
Releasing feature/add-login to main...

✓ Validated feature/add-login is in qa environment
✓ Checked out main
✓ Merged feature/add-login into main
✓ Pushed main to remote
✓ Removed feature/add-login from all environments
✓ Updated metadata (marked merged_to_main_at)

Success! feature/add-login is now in main

The branch will be eligible for cleanup in 7 days.
Use 'hitch cleanup' to delete stale branches.
```

**What happened:**
- Validated branch was tested (in qa)
- Merged to main
- Pushed main to remote
- Removed from all environments
- Marked for cleanup in 7 days

## Step 7: Clean Up

After the retention period, delete the branch:

```bash
# Check what will be deleted
hitch cleanup --dry-run
```

**Expected output:**
```
Dry run: simulating cleanup

Branches eligible for cleanup (1):
  feature/add-login
    Merged to main: 7 days ago
    Promoted to: (none - removed after merge)
    Will delete: local and remote branches

No changes made. Run without --dry-run to apply.
```

Delete the branch:

```bash
hitch cleanup
```

**Expected output:**
```
Cleaning up merged branches...

Deleting feature/add-login:
  ✓ Deleted local branch
  ✓ Deleted remote branch

Cleanup complete! Deleted 1 branch(es).
```

## Summary

You've learned the complete Hitch workflow:

1. ✅ **Initialize** - Set up Hitch with environments
2. ✅ **Promote** - Add features to dev and qa
3. ✅ **Release** - Merge to main after testing
4. ✅ **Cleanup** - Delete merged branches

## Next Steps

- [Example 2: Multi-Environment Flow](./02-multi-environment.md) - Work with multiple features
- [Example 3: Team Collaboration](./03-team-collaboration.md) - Collaborate with teammates
- [Commands Reference](../COMMANDS.md) - Learn all commands

## Troubleshooting

### "Not a git repository"

Make sure you're in a Git repository:
```bash
git init
git commit --allow-empty -m "Initial commit"
```

### "Environment is locked"

Someone else is rebuilding. Wait or check:
```bash
hitch status  # See who locked it
hitch unlock dev  # Force unlock (use carefully!)
```

### "Merge conflict detected"

Your feature conflicts with main. Rebase:
```bash
git checkout feature/add-login
git rebase main
# Resolve conflicts
git push --force-with-lease
hitch promote feature/add-login to dev
```

### "Branch not found"

The branch doesn't exist remotely:
```bash
git push origin feature/add-login
```

## Tips

- Run `hitch status` frequently to see current state
- Use `--dry-run` with cleanup to preview changes
- Features must be in an environment before release (safety check)
- Environments are rebuilt fresh each time (always consistent)

## Learn More

- [Architecture](../ARCHITECTURE.md) - How Hitch works internally
- [Workflows](../WORKFLOWS.md) - Advanced patterns
- [Safety](../SAFETY.md) - Built-in safety mechanisms
