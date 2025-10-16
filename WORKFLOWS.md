# Hitch Workflows

Common workflows and usage patterns for Hitch.

## Table of Contents

- [Initial Setup](#initial-setup)
- [Standard Feature Development](#standard-feature-development)
- [Multi-Developer Coordination](#multi-developer-coordination)
- [Emergency Hotfix](#emergency-hotfix)
- [Removing Problematic Features](#removing-problematic-features)
- [Long-Running Features](#long-running-features)
- [Release Day](#release-day)
- [Maintenance Tasks](#maintenance-tasks)
- [Troubleshooting](#troubleshooting)

---

## Initial Setup

### First Time Repository Setup

```bash
# Clone repository
git clone git@github.com:yourorg/yourproject.git
cd yourproject

# Initialize Hitch
hitch init --environments dev,qa

# Verify setup
hitch status
```

**Expected output:**
```
✓ Hitch initialized successfully

Environment: dev (unlocked)
  Base: main
  Features: (none)

Environment: qa (unlocked)
  Base: main
  Features: (none)
```

### Team Member Setup

If Hitch is already initialized in the repo:

```bash
# Clone repository
git clone git@github.com:yourorg/yourproject.git
cd yourproject

# Fetch hitch-metadata branch
git fetch origin hitch-metadata:hitch-metadata

# Check current status
hitch status
```

---

## Standard Feature Development

### Scenario: Implementing a New Feature

**Dev M is implementing a new login feature**

```bash
# 1. Start from main
git checkout main
git pull origin main

# 2. Create feature branch
git checkout -b feature/oauth-login

# 3. Implement feature
# ... make changes ...
git add .
git commit -m "Implement OAuth login"
git push origin feature/oauth-login

# 4. Deploy to dev environment
hitch promote feature/oauth-login to dev

# 5. Test in dev (https://dev.example.org)
# ... manual testing ...

# 6. Promote to qa
hitch promote feature/oauth-login to qa

# 7. QA testing
# ... QA team tests on https://qa.example.org ...

# 8. Release to production
hitch release feature/oauth-login

# 9. Verify deployment
git checkout main
git pull origin main
```

**Timeline:**
- Day 1: Create branch, develop feature
- Day 2: Promote to dev, test
- Day 3: Promote to qa, QA testing
- Day 4: Release to main

---

## Multi-Developer Coordination

### Scenario: Three Developers, Multiple Features

**Dev M, Dev S, and Dev R are working on different features simultaneously**

#### Dev M: OAuth Login

```bash
git checkout -b feature/oauth-login
# ... implement ...
git push origin feature/oauth-login
hitch promote feature/oauth-login to dev
```

#### Dev S: Dashboard Widget

```bash
git checkout -b feature/dashboard-widget
# ... implement ...
git push origin feature/dashboard-widget
hitch promote feature/dashboard-widget to dev
```

#### Dev R: Bug Fix

```bash
git checkout -b bug/fix-pagination
# ... implement ...
git push origin bug/fix-pagination
hitch promote bug/fix-pagination to dev
```

#### Result: dev Environment

```bash
$ hitch status
Environment: dev (unlocked)
  Base: main
  Features:
    - feature/oauth-login
    - feature/dashboard-widget
    - bug/fix-pagination
```

#### Promoting to QA (Different Timings)

```bash
# Dev M's feature is ready first
hitch promote feature/oauth-login to qa

# Dev R's bug fix is ready
hitch promote bug/fix-pagination to qa

# Dev S's feature needs more work, stays in dev

$ hitch status
Environment: dev (unlocked)
  Base: main
  Features:
    - feature/oauth-login
    - feature/dashboard-widget
    - bug/fix-pagination

Environment: qa (unlocked)
  Base: main
  Features:
    - feature/oauth-login
    - bug/fix-pagination
```

#### Releasing to Production

```bash
# OAuth login approved
hitch release feature/oauth-login

# Bug fix approved
hitch release bug/fix-pagination

# Dashboard widget still in dev, not ready yet

# Rebuild environments to remove released features
hitch rebuild dev
hitch rebuild qa
```

---

## Emergency Hotfix

### Scenario: Production Bug Needs Immediate Fix

**Production is down, need to bypass normal workflow**

```bash
# 1. Create hotfix branch from main
git checkout main
git pull origin main
git checkout -b hotfix/critical-payment-bug

# 2. Implement fix
# ... fix bug ...
git commit -am "Fix critical payment processing bug"
git push origin hotfix/critical-payment-bug

# 3. Skip dev, go straight to qa for quick verification
hitch promote hotfix/critical-payment-bug to qa

# 4. Quick smoke test on qa
# ... verify fix works ...

# 5. Release immediately to production
hitch release hotfix/critical-payment-bug

# 6. Backfill to dev
hitch rebuild dev
```

**Timeline:**
- 0 minutes: Bug discovered
- 10 minutes: Fix implemented
- 15 minutes: Deployed to qa
- 20 minutes: Verified, released to production

---

## Removing Problematic Features

### Scenario: Feature Causes Issues in QA

**Dev M promoted feature, but it breaks QA**

```bash
$ hitch promote feature/new-api to qa
Error: Merge conflict when adding feature/new-api to qa

# Feature conflicts with existing qa environment
```

#### Option 1: Fix the Conflict

```bash
# Rebase feature on main to resolve conflicts
git checkout feature/new-api
git fetch origin main
git rebase origin/main

# Resolve conflicts
# ... fix conflicts ...
git rebase --continue
git push --force-with-lease origin feature/new-api

# Try promoting again
hitch promote feature/new-api to qa
```

#### Option 2: Remove Other Feature

```bash
# Maybe feature/old-api is causing the conflict
hitch demote feature/old-api from qa

# Now try promoting new feature
hitch promote feature/new-api to qa
```

#### Option 3: Hold Off on Promotion

```bash
# Keep feature in dev for now
# QA can test other features

$ hitch status
Environment: dev (unlocked)
  Base: main
  Features:
    - feature/new-api  (conflicts with qa, staying in dev)
    - feature/other

Environment: qa (unlocked)
  Base: main
  Features:
    - feature/other
```

---

## Long-Running Features

### Scenario: Feature Takes Weeks to Complete

**Dev M is building a complex feature that will take 3 weeks**

#### Week 1: Start Feature

```bash
git checkout -b feature/new-payment-system
# ... initial implementation ...
git push origin feature/new-payment-system
hitch promote feature/new-payment-system to dev
```

#### Week 2: Keep Feature in Sync with Main

```bash
# Main branch has moved forward with other releases
git checkout main
git pull origin main

# Rebase feature to stay current
git checkout feature/new-payment-system
git rebase main

# ... continue development ...
git push --force-with-lease origin feature/new-payment-system

# Rebuild dev to pick up changes
hitch rebuild dev
```

#### Week 3: Feature Complete

```bash
# Final rebase
git checkout feature/new-payment-system
git rebase main
git push --force-with-lease origin feature/new-payment-system

# Promote to qa
hitch promote feature/new-payment-system to qa

# Test, then release
hitch release feature/new-payment-system
```

**Best Practice:**
- Rebase on main weekly (or after each main release)
- Keep feature in dev throughout development
- Only promote to qa when complete

---

## Release Day

### Scenario: Planned Release with Multiple Features

**Team has 3 features ready to release**

#### Pre-Release: Verify QA

```bash
$ hitch status
Environment: qa (unlocked)
  Base: main
  Features:
    - feature/oauth-login
    - feature/dashboard-widget
    - bug/fix-pagination

# All features tested and approved ✓
```

#### Release Process

```bash
# Release features in order
hitch release feature/oauth-login
hitch release feature/dashboard-widget
hitch release bug/fix-pagination

# Verify main branch
git checkout main
git pull origin main
git log -3
```

#### Post-Release: Clean Up Environments

```bash
# Rebuild environments (released features are removed)
hitch rebuild dev
hitch rebuild qa

# Check status
$ hitch status
Environment: dev (unlocked)
  Base: main
  Features: (none)

Environment: qa (unlocked)
  Base: main
  Features: (none)

# Check for stale branches
hitch cleanup --dry-run
```

#### After Retention Period (7 days)

```bash
# Clean up released branches
hitch cleanup

# Output:
# Branches safe to delete (merged to main > 7 days ago):
#   ✓ feature/oauth-login (merged 8 days ago)
#   ✓ feature/dashboard-widget (merged 8 days ago)
#   ✓ bug/fix-pagination (merged 8 days ago)
#
# Delete 3 stale branches? (y/N): y
```

---

## Maintenance Tasks

### Weekly Maintenance

```bash
# Check for stale branches
hitch status --stale

# Clean up old branches
hitch cleanup --dry-run
hitch cleanup

# Verify environments are clean
hitch status
```

### Monthly Audit

```bash
# List all tracked branches
hitch list

# Review inactive branches
hitch list --unmerged

# Review merged branches awaiting cleanup
hitch list --merged

# Check configuration
hitch config show
```

### Keeping Environments Fresh

If `dev` or `qa` has accumulated many features:

```bash
# See what's in each environment
hitch status

# Consider releasing ready features
hitch release feature/ready-feature-1
hitch release feature/ready-feature-2

# Or remove features not ready for testing
hitch demote feature/not-ready from dev

# Rebuild to clean state
hitch rebuild dev
hitch rebuild qa
```

---

## Troubleshooting

### Environment Stuck in Locked State

```bash
$ hitch promote feature/xyz to qa
Error: qa environment is locked by dev-m@example.com (since 10:30:00)

# Check if lock is stale (> 15 minutes)
$ hitch status
Warning: qa has stale lock (locked 45 minutes ago)

# Force unlock
hitch unlock qa --force
```

### Merge Conflicts

```bash
$ hitch promote feature/xyz to qa
Error: Merge conflict when adding feature/xyz to qa

# Option 1: Rebase feature on main
git checkout feature/xyz
git rebase main
# ... resolve conflicts ...
git push --force-with-lease
hitch promote feature/xyz to qa

# Option 2: Remove conflicting feature
hitch demote feature/conflicting from qa
hitch promote feature/xyz to qa
```

### Feature Not Appearing in Environment

```bash
# Promoted but environment not updated
hitch promote feature/xyz to dev

# Manually rebuild
hitch rebuild dev

# Check if feature exists
git branch -a | grep feature/xyz

# Check metadata
git checkout hitch-metadata
cat hitch.json
```

### Lost Metadata

```bash
# Restore from backup tag
git tag -l "hitch-backup-*"
git checkout hitch-metadata
git reset --hard hitch-backup-20251016
git push --force-with-lease origin hitch-metadata

# Or re-initialize (DESTRUCTIVE)
hitch init --force
```

### Out of Sync with Remote

```bash
# Local metadata out of sync
git checkout hitch-metadata
git pull origin hitch-metadata

# Environment out of sync
hitch rebuild dev
hitch rebuild qa
```

---

## Advanced Patterns

### Holding Environment for Testing

```bash
# Lock environment to prevent changes during testing
hitch lock qa --reason "Load testing in progress"

# Test for several hours...

# Unlock when done
hitch unlock qa
```

### Partial Rollback

```bash
# Remove just the problematic feature
hitch demote feature/broken from qa

# Environment automatically rebuilds without it
```

### Feature Toggles vs. Hitch

**When to use Hitch:**
- Large features that need environment isolation
- Features that require separate testing phases
- Changes that affect multiple services/repos

**When to use feature toggles:**
- Small features that can be safely deployed but hidden
- A/B testing
- Gradual rollouts

**Best practice:** Use both!
```bash
# Deploy with feature toggle OFF
hitch promote feature/new-ui to dev

# Test in dev with toggle ON
# Once stable, promote to qa
hitch promote feature/new-ui to qa

# Release to production with toggle OFF
hitch release feature/new-ui

# Gradually enable toggle in production
```

---

## Team Workflows

### Small Team (2-3 developers)

```
main ──────────────────────────────────────
         ↓                    ↓
feature/a ─→ dev ─→ qa ─→ main
feature/b ─→ dev ─→ qa ─→ main
```

**Process:**
- Work on features independently
- Share dev environment
- Test together in qa
- Release when ready

### Medium Team (4-10 developers)

```
main ──────────────────────────────────────
         ↓                    ↓
team-a: feature/a ─→ dev ─→ qa ─→ main
team-b: feature/b ─→ dev ─→ qa ─→ main
team-c: feature/c ─→ dev ─→ qa ─→ main
```

**Process:**
- Multiple features in dev simultaneously
- Coordinate qa promotions
- Weekly releases

### Large Team (10+ developers)

```
main ───────────────────────────────────────
         ↓                     ↓
team-1: features ─→ dev1 ─→ qa1 ─→ main
team-2: features ─→ dev2 ─→ qa2 ─→ main
team-3: features ─→ dev3 ─→ qa3 ─→ main
```

**Process:**
- Multiple dev/qa environments per team
- Separate deployment schedules
- Coordinated releases

Configure multiple environments:
```bash
hitch init --environments dev1,dev2,dev3,qa1,qa2,qa3,staging
```

---

## CI/CD Integration

### GitHub Actions

```yaml
name: Hitch Promote

on:
  pull_request:
    types: [labeled]

jobs:
  promote-to-dev:
    if: contains(github.event.pull_request.labels.*.name, 'promote-to-dev')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Install Hitch
        run: |
          curl -sSL https://github.com/DoomedRamen/hitch/releases/latest/download/hitch-linux-amd64 -o /usr/local/bin/hitch
          chmod +x /usr/local/bin/hitch
      - name: Promote to dev
        run: hitch promote ${{ github.head_ref }} to dev
```

### GitLab CI

```yaml
promote-to-dev:
  stage: deploy
  script:
    - hitch promote $CI_COMMIT_REF_NAME to dev
  only:
    - branches
  when: manual
```

---

## Tips and Best Practices

### Do's

✅ Always branch from `main`
✅ Rebase features on `main` regularly
✅ Test in `dev` before `qa`
✅ Clean up merged branches regularly
✅ Use descriptive branch names
✅ Communicate with team when promoting to shared environments

### Don'ts

❌ Never commit directly to `dev` or `qa`
❌ Never branch from `dev` or `qa`
❌ Don't keep features in environments longer than necessary
❌ Don't forget to rebuild after main changes
❌ Don't force unlock without checking with team first
❌ Don't promote broken features to qa

### Performance

- Keep environments clean (< 5 features per environment)
- Rebase long-running features regularly
- Release features as soon as they're ready
- Run `hitch cleanup` weekly

### Security

- Use branch protection on `hitch-metadata`
- Require pull requests for main branch
- Use hooks to prevent direct commits to managed branches
- Audit lock usage with `hitch status`
