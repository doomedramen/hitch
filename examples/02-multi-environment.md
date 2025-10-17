# Example 2: Multi-Environment Flow

**Duration:** 15 minutes
**Difficulty:** Beginner-Intermediate

## Overview

This example demonstrates managing multiple features across different environments simultaneously - a common real-world scenario.

## Scenario

Your team is working on three features:
1. `feature/new-dashboard` - UI redesign (ready for QA)
2. `feature/api-refactor` - Backend changes (in dev testing)
3. `feature/bug-fix` - Critical fix (needs fast-track to production)

## Prerequisites

- Completed [Example 1: Basic Workflow](./01-basic-workflow.md)
- Repository with Hitch initialized

## Setup

```bash
# Ensure clean starting state
git checkout main
hitch status
```

## Creating Multiple Features

### Feature 1: Dashboard (Large Feature)

```bash
git checkout -b feature/new-dashboard
echo "New dashboard UI" > dashboard.js
git add dashboard.js
git commit -m "feat: redesign dashboard UI"
git push origin feature/new-dashboard
```

### Feature 2: API Refactor (Medium Feature)

```bash
git checkout main
git checkout -b feature/api-refactor
echo "Refactored API" > api.js
git add api.js
git commit -m "refactor: improve API structure"
git push origin feature/api-refactor
```

### Feature 3: Bug Fix (Small, Urgent)

```bash
git checkout main
git checkout -b feature/bug-fix
echo "Fixed critical bug" > bugfix.js
git add bugfix.js
git commit -m "fix: resolve login timeout issue"
git push origin feature/bug-fix
```

## Progressive Promotion

### Stage 1: All to Dev

Promote all features to dev for initial testing:

```bash
hitch promote feature/new-dashboard to dev
hitch promote feature/api-refactor to dev
hitch promote feature/bug-fix to dev
```

Check the state:

```bash
hitch status
```

**Output:**
```
Environment: dev (base: main)
  Features (3):
    ✓ feature/new-dashboard
    ✓ feature/api-refactor
    ✓ feature/bug-fix

Environment: qa (base: main)
  Features (0):
    (empty)
```

### Stage 2: Dashboard to QA

Dashboard passes dev testing, move to qa:

```bash
hitch promote feature/new-dashboard to qa
```

**Current state:**
- Dev: All 3 features
- QA: Dashboard only

### Stage 3: Fast-track Bug Fix

Bug fix is urgent - promote directly to qa:

```bash
hitch promote feature/bug-fix to qa
```

**Current state:**
- Dev: All 3 features
- QA: Dashboard + Bug fix

### Stage 4: Remove API Refactor from Dev

API refactor needs rework - remove from dev:

```bash
hitch demote feature/api-refactor from dev
```

**Output:**
```
Demoting feature/api-refactor from dev...

✓ Acquired lock on dev
✓ Removed feature/api-refactor from feature list
✓ Rebuilt dev environment
✓ Pushed dev branch to remote
✓ Updated metadata

Success! feature/api-refactor removed from dev

Remaining dev features:
  - feature/new-dashboard
  - feature/bug-fix
```

## Viewing Status

Check detailed status:

```bash
hitch status
```

**Output:**
```
Environment: dev (base: main)
  Features (2):
    ✓ feature/new-dashboard
    ✓ feature/bug-fix

Environment: qa (base: main)
  Features (2):
    ✓ feature/new-dashboard
    ✓ feature/bug-fix

Tracked Branches (3):
  feature/new-dashboard
    Created: 2025-10-17 14:00:00
    Promoted to: dev, qa
    Status: Active

  feature/bug-fix
    Created: 2025-10-17 14:15:00
    Promoted to: dev, qa
    Status: Active

  feature/api-refactor
    Created: 2025-10-17 14:10:00
    Promoted to: (none)
    Status: Not promoted
```

## Selective Release

### Release Bug Fix First

```bash
hitch release feature/bug-fix
```

**Output:**
```
Releasing feature/bug-fix to main...

✓ Validated feature/bug-fix is in qa environment
✓ Checked out main
✓ Merged feature/bug-fix into main
✓ Pushed main to remote
✓ Removed feature/bug-fix from all environments
✓ Updated metadata

Success! feature/bug-fix is now in main
```

**Current state:**
- Dev: Dashboard only
- QA: Dashboard only
- Main: Bug fix merged

### Release Dashboard

After QA approval:

```bash
hitch release feature/new-dashboard
```

## Reworking the API Refactor

Fix issues and re-promote:

```bash
# Make fixes on feature branch
git checkout feature/api-refactor
echo "Improved API (v2)" > api.js
git add api.js
git commit -m "refactor: address review feedback"
git push origin feature/api-refactor

# Promote back to dev
git checkout main
hitch promote feature/api-refactor to dev
```

## Advanced: Environment Comparison

See what's different between environments:

```bash
# What's in dev but not in qa?
hitch status | grep -A 10 "dev"
hitch status | grep -A 10 "qa"

# Manual comparison
echo "Dev features:"
git show hitch-metadata:hitch.json | jq '.environments.dev.features'

echo "QA features:"
git show hitch-metadata:hitch.json | jq '.environments.qa.features'
```

## Real-World Patterns

### Pattern 1: Feature Flags

Use Hitch with feature flags for even more control:

```bash
# Feature in dev with flag enabled
hitch promote feature/experimental to dev
# Flag disabled in production
hitch release feature/experimental
# Enable flag when ready
```

### Pattern 2: Hotfix Flow

Critical production issues:

```bash
# 1. Create hotfix from main
git checkout main
git checkout -b feature/hotfix-critical

# 2. Fix and test locally
# ... make changes ...

# 3. Fast-track to qa
hitch promote feature/hotfix-critical to qa

# 4. Skip dev, release immediately after qa
hitch release feature/hotfix-critical
```

### Pattern 3: Long-Running Features

Features that take weeks:

```bash
# Keep promoting to dev as you work
hitch promote feature/big-refactor to dev

# Other features can come and go
hitch promote feature/small-fix to dev
hitch release feature/small-fix

# Your feature stays in dev
hitch status  # Still shows feature/big-refactor in dev
```

## Cleanup Strategy

### Check Cleanup Status

```bash
hitch cleanup --dry-run
```

**Output:**
```
Branches eligible for cleanup (1):
  feature/bug-fix
    Merged to main: 7 days ago
    Last activity: 8 days ago

Branches NOT eligible yet (2):
  feature/new-dashboard
    Merged to main: 3 days ago
    Eligible in: 4 days

  feature/api-refactor
    Not merged
    Active in: dev
```

### Clean Up Merged Branches

```bash
hitch cleanup
```

## Summary

You've learned:

1. ✅ Managing multiple features simultaneously
2. ✅ Progressive promotion (dev → qa → main)
3. ✅ Selective releases (release one feature at a time)
4. ✅ Demoting features that need rework
5. ✅ Fast-tracking urgent fixes
6. ✅ Environment comparison

## Common Scenarios

### "I need to remove a feature from QA"

```bash
hitch demote feature/problematic from qa
```

### "I want to see what's in each environment"

```bash
hitch status
# or
git show hitch-metadata:hitch.json | jq .
```

### "Can I promote the same feature to multiple environments?"

Yes! That's the normal flow:

```bash
hitch promote feature/xyz to dev
hitch promote feature/xyz to qa
hitch release feature/xyz
```

### "What if two features conflict?"

Hitch will detect this during rebuild:

```bash
hitch promote feature/b to dev
# Error: Merge conflict when adding feature/b
# Resolution: Rebase feature/b on main
```

## Next Steps

- [Example 3: Team Collaboration](./03-team-collaboration.md)
- [Example 4: Handling Conflicts](./04-handling-conflicts.md)
- [Workflows Guide](../WORKFLOWS.md)
