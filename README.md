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

## 🛡️ Pre-commit Protection

Protect your environment branches from direct commits:

```bash
# Add to your .git/hooks/pre-commit
hitch guard

# Now attempts to commit directly to environment branches will fail
git checkout qa
git commit -m "hotfix"  # ❌ Blocked by hitch guard
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

## 📄 License

MIT License - see [LICENSE](LICENSE) file for details.

## 🤝 Contributing

Contributions are welcome! Please read [DEVELOPMENT.md](DEVELOPMENT.md) for details on our code of conduct and the process for submitting pull requests.

## 🔗 Links

- [Documentation](DEVELOPMENT.md)
- [CI/CD Pipeline](CI_CD.md)
- [Specification](SPEC.md)