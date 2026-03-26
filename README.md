<p align="center">
  <img src="hitch.svg" alt="Hitch Logo" width="120" height="120">
</p>

# Hitch

> Git branch management for environment-based deployments

Hitch is a CLI tool that treats environment branches as composable layers on top of base branches. Promote feature branches to environments, and Hitch rebuilds the environment branch by squashing them together. Demote to remove. Lock to freeze. Require approvals for production.

## 🚀 Quick Start

```bash
# Initialize Hitch in your Git repository
hitch init

# Add environments for your deployment pipeline
hitch add dev --source main
hitch add qa --source main
hitch add production --source main

# Promote branches to environments
hitch promote feature/user-auth dev
hitch promote feature/user-auth qa
hitch promote feature/user-auth production

# Check the status of all environments
hitch status
```

## 🎯 What Problem Does Hitch Solve?

If you're using branch-based deployments, you've likely faced these challenges:

- **"What branches are actually in `dev` right now?"** – You promoted 5 features, but which ones made it?
- **"Who promoted `feature/api-endpoints` to production?"** – No audit trail.
- **"Can we freeze production during the holiday sale?"** – Nothing stops accidental deployments.
- **"How do I rebuild `qa` with the latest from all promoted features?"** – Manual merging is error-prone.
- **"How do we require team approval before production deployments?"** – No built-in workflow.

Hitch solves these by treating environment branches as **composable layers**:

- Each environment (`dev`, `qa`, `production`) has a base branch (usually `main`)
- Promote feature branches to environments – Hitch squashes them together and rebuilds the environment branch
- Demote to remove a layer – Hitch rebuilds without it
- Full audit trail: who promoted what, when, and to which environment

## 📋 Use Cases

### Branch-Based Deployment Strategies
Perfect for teams that deploy branches to different environments:
- `main` → Production
- `develop` → Staging
- `feature/*` → Development

### GitOps Workflows
Integrates seamlessly with GitOps processes requiring:
- Structured promotion workflows
- Audit trails for deployments
- Environment locking during critical periods

### Multi-Environment Applications
Ideal for applications with complex deployment pipelines:
- Development → QA → Staging → Production
- Feature flags and environment-specific configurations
- Rollback and promotion tracking

## 🔧 Core Concepts

### Environments as Composable Layers

Think of each environment branch as a stack built on top of a base:

```
production = main + feature/auth
qa         = main + feature/auth + feature/payments
dev        = main + feature/auth + feature/payments + feature/ui + feature/api
```

Each environment has:
- **Base branch** – The foundation (usually `main`)
- **Promoted branches** – Feature branches layered on top
- **Lock state** – Freeze promotions during sensitive periods
- **Rebuild timestamp** – When the branch was last regenerated

When you promote or demote a branch, Hitch rebuilds the environment by squashing all promoted branches together. This means:
- Environment branches are **regenerated**, not manually merged
- You can add/remove layers at will before finally releasing to `main`
- No merge conflicts accumulating over time – each rebuild is fresh

### Promotion Workflow
```bash
# Promote a feature branch through environments
hitch promote feature/new-api dev     # Deploy to development
hitch promote feature/new-api qa      # Deploy to QA testing
hitch promote feature/new-api production  # Deploy to production
```

### Approval Workflow
For sensitive environments like production, you can require multi-person approval before promotions are applied. This adds an extra safety layer to prevent unauthorized or premature deployments.

#### Configuration
Add approval requirements to your environment in `hitch.json`:
```json
{
  "environments": {
    "production": {
      "base": "main",
      "branches": [],
      "requires_approval": true,
      "min_approvals": 2,
      "approvers": ["alice@company.com", "bob@company.com", "charlie@company.com"]
    }
  }
}
```

#### Approval Workflow Example
```bash
# Request promotion (creates approval request instead of direct promotion)
hitch promote feature/new-api production
# ✓ Approval request 3f8a92 created - requires 2 approvals

# View pending approval requests
hitch approvals list --status pending
# Shows: Request 3f8a92 (0/2 approvals)

# Approvers approve the request
hitch approvals approve 3f8a92 "Tested in QA, looks good"
# ✓ Approval recorded (1/2)

hitch approvals approve 3f8a92 "LGTM, ready for production"
# ✓ Approval threshold reached (2/2)
# ✓ Promotion applied automatically to production!

# View detailed request status
hitch approvals status 3f8a92

# Reject a request with reason
hitch approvals reject 3f8a92 "Needs more testing on edge cases"

# Cancel your own request
hitch approvals cancel 3f8a92

# Clean up old requests
hitch approvals cleanup --older-than 90
```

**Key Features:**
- **Snapshot Validation:** Captures branch SHAs when requested; rejects if branches change before approval
- **Authorization:** Only designated approvers can approve/reject
- **Self-Approval Prevention:** Cannot approve your own requests
- **Duplicate Prevention:** Cannot approve the same request twice
- **Audit Trail:** Tracks who requested, who approved, and when
- **Automatic Application:** Promotion applies automatically when threshold is reached

#### Branch Protection Setup (Recommended)

While Hitch enforces approvals at the application level, you should also configure branch protection rules on your Git hosting platform to prevent bypassing Hitch with direct `git push` commands.

**GitHub:**
```
Settings → Branches → Branch protection rules

For environment branches (e.g., production, staging):
☑ Require pull request reviews before merging (optional, for PR-based workflows)
☑ Require status checks to pass before merging
☑ Require branches to be up to date before merging
☑ Do not allow bypassing the above settings
☑ Restrict who can push to matching branches
  → Add: CI/CD service account or Hitch automation user
```

**GitLab:**
```
Settings → Repository → Protected Branches

For environment branches:
- Branch: production
- Allowed to merge: Maintainers
- Allowed to push: No one (or CI/CD service account only)
- Allowed to force push: No
```

**Bitbucket:**
```
Repository settings → Branch permissions

For environment branches:
- Type: Branch
- Branch: production
- Prevent all changes except from: [CI/CD user or specific users]
- Prevent deletion: Yes
- Require approvals: Yes (if using PR workflow)
```

**Why This Matters:**
- Hitch's approval workflow can be bypassed with `git push --force` or `git push --no-verify`
- Branch protection rules enforce permissions at the repository level
- Provides defense-in-depth: Hitch + Git hosting platform protection
- Essential for production environments with compliance requirements

**Recommended Setup:**
1. Configure Hitch approval workflow for sensitive environments
2. Add `hitch guard` to pre-commit hooks (prevents local commits)
3. Enable branch protection rules on your Git hosting platform (prevents remote pushes)
4. Use a dedicated service account for CI/CD with push permissions

### Environment Locking
```bash
# Lock production during critical periods
hitch lock production

# Production is now locked - no promotions allowed
hitch promote feature/hotfix production  # ❌ Will fail

# Unlock when ready
hitch unlock production
```

### Environment Rebuilding
```bash
# Rebuild environment with all promoted branches
hitch rebuild production

# This creates a new commit on production's base branch
# containing all changes from promoted branches
```

### Environment Releasing
```bash
# Release all promoted branches from an environment to a target branch
hitch release production main

# Force release even if environment is locked
hitch release staging main --force

# This creates a permanent release with auto-tags
```

## 📊 Status Output

Hitch provides a comprehensive overview of your environments:

```
🚀 Hitch Environment Status
──────────────────────────────────────────────────
📊 3 environments: 3 total, 1 locked, 1 need rebuild, 0 never rebuilt

📍 Current branch: main

┌─ dev 🔓 base: main
│  🔓 Environment is unlocked
├─ Branches (2 promoted):
│  ✅ 1. feature/user-auth
│  ✅ 2. feature/api-endpoints
├─ Rebuilt:
│  • 2025-01-15 14:30 UTC (2 hours ago)
└─ Status:
   ✅ Up to date

┌─ qa 🔒 base: main
│  🔒 Locked by admin@company.com at 2025-01-15 12:00 UTC
│  ⚠️ Environment is locked - no changes allowed
├─ Branches (1 promoted):
│  ✅ 1. feature/user-auth
├─ Rebuilt:
│  • 2025-01-15 10:00 UTC (6 hours ago)
└─ Status:
   ⚠️ Rebuild needed (feature/api-endpoints has newer commits)
   💡 Run 'hitch rebuild qa' to update

┌─ production 🔓 base: main
│  🔓 Environment is unlocked
├─ Branches (1 promoted):
│  ✅ 1. feature/user-auth
├─ Rebuilt:
│  • Never
└─ Status:
   ⚠️ Never rebuilt
   💡 Run 'hitch rebuild production' to initialize
```

## 🛡️ Branch Protection with `hitch guard`

Prevent direct commits to environment branches by integrating `hitch guard` into your git hooks. The guard command:
- **Blocks commits** on environment branches (e.g., `dev`, `qa`, `production`)
- **Allows commits** on feature branches, even if they're promoted
- **Exits with error** if hitch is not initialized, blocking commits in hitch-enabled repos

### Manual Git Hook

```bash
# Add to your .git/hooks/pre-commit
#!/bin/sh
hitch guard || exit 1
```

### Lefthook

```yaml
# lefthook.yml
pre-commit:
  commands:
    hitch-guard:
      run: hitch guard
```

### Husky

```json
// package.json
{
  "husky": {
    "hooks": {
      "pre-commit": "hitch guard"
    }
  }
}
```

### pre-commit

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: hitch-guard
        name: Hitch Guard
        entry: hitch guard
        language: system
        stages: [commit]
```

### Example Workflow

```bash
# Environment branches are protected
git checkout qa
echo "fix" > hotfix.txt && git add . && git commit -m "hotfix"
# ❌ Error: Current branch 'qa' conflicts with environment(s): qa

# Feature branches are allowed
git checkout feature/my-fix
echo "fix" > hotfix.txt && git add . && git commit -m "hotfix"
# ✅ Commit succeeds

# Hitch commands bypass protection (they use --no-verify internally)
hitch rebuild qa  # ✅ Works even with guard installed
```

## 📦 Installation

### Cargo (Recommended)
```bash
cargo install hitch
```

### From Source
```bash
git clone https://github.com/doomedramen/hitch
cd hitch
cargo install --path .
```

### Homebrew (macOS)
```bash
brew install doomedramen/homebrew-hitch/hitch
```

## 🔧 Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for development setup and guidelines.

## 🔗 Links

- [Development Guide](DEVELOPMENT.md)
- [CI/CD Pipeline](CI_CD.md)
- [Technical Specification](SPEC.md)

## 📄 License

MIT License - see [LICENSE](LICENSE) file for details.

## 🤝 Contributing

Contributions are welcome! Please read [DEVELOPMENT.md](DEVELOPMENT.md) for the process for submitting pull requests.