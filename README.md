<p align="center">
  <img src="hitch.svg" alt="Hitch Logo" width="120" height="120">
</p>

# Hitch

> Git branch management for environment-based deployments

Hitch is a CLI tool that brings environment branch management to Git. It helps you organize and track deployment branches (like `dev`, `qa`, `main`) with proper promotion workflows, locking mechanisms, and rebuild automation—turning chaotic branch-based releases into a structured, auditable process.

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

- **"Which branches are currently deployed to which environments?"**
- **"Who deployed the feature/api-endpoints branch to production?"**
- **"Can we lock production while we fix this critical bug?"**
- **"How do we rebuild staging with all the latest promoted features?"**

Hitch solves these problems by providing a structured metadata layer that tracks:
- Which branches are promoted to which environments
- When environments were last rebuilt
- Who locked environments and when
- Proper promotion/demotion workflows

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

### Environments
Named deployment targets (e.g., `dev`, `qa`, `production`) that have:
- A base source branch (usually `main`)
- Promoted branches that should be included
- Lock/unlock state for deployment control
- Rebuild timestamps for audit trails

### Promotion Workflow
```bash
# Promote a feature branch through environments
hitch promote feature/new-api dev     # Deploy to development
hitch promote feature/new-api qa      # Deploy to QA testing
hitch promote feature/new-api production  # Deploy to production
```

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