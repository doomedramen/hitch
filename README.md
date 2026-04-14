<p align="center">
  <img src="hitch.svg" alt="Hitch Logo" width="120" height="120">
</p>

# Hitch

> Git branch management for environment-based deployments

Hitch treats environment branches as composable layers. Promote feature branches to environments, and Hitch rebuilds the environment branch by squashing them together. Demote to remove. Lock to freeze. Require approvals for production.

## 🚀 Quick Start

```bash
# Initialize Hitch in your Git repository
hitch init

# Add environments for your deployment pipeline
hitch add dev --base main
hitch add qa --base main
hitch add production --base main

# Promote branches to environments
hitch promote feature/user-auth dev
hitch promote feature/user-auth qa
hitch promote feature/user-auth production

# Check the status of all environments
hitch status
```

## 🎯 What Problem Does Hitch Solve?

- **"What branches are actually in `dev` right now?"** – Full audit trail of promotions
- **"How do I rebuild `qa` with the latest from all promoted features?"** – `hitch rebuild` regenerates environment branches
- **"Can we freeze production during critical periods?"** – `hitch lock production`
- **"How do we require team approval before production deployments?"** – Approval workflow with configurable thresholds

## 🔧 Core Concepts

### Environments as Composable Layers

Each environment branch is a stack built on top of a base:

```
production = main + feature/auth
qa         = main + feature/auth + feature/payments
dev        = main + feature/auth + feature/payments + feature/ui + feature/api
```

When you promote or demote, Hitch rebuilds the environment by squashing all promoted branches together. Environment branches are **regenerated**, not manually merged.

## 📋 Key Commands

```bash

# Create a new feature branch and set up promotion targets
hitch branch feature/foo develop --to dev --to qa

# Promote/demote branches through environments
hitch promote feature/new-api dev
hitch demote feature/new-api dev

# Rebuild environment with all promoted branches
hitch rebuild production

# Lock/unlock environments (freeze promotions)
hitch lock production
hitch unlock production

# View environment status
hitch status

# View branch hierarchy
hitch tree

# Release environment to target branch
hitch release production main

# Guard environment branches from direct commits
hitch guard

# Update environment configuration
hitch set dev --base main           # Change base branch
hitch set production --requires-approval true
hitch set production --min-approvals 2
hitch set production --add-approver alice@example.com
```

## 🛡️ Approval Workflow

For sensitive environments, require multi-person approval before promotions:

```json
// hitch.json
{
  "environments": {
    "production": {
      "requires_approval": true,
      "min_approvals": 2
    }
  }
}
```

```bash
# Request promotion (requires approval before applying)
hitch promote feature/new-api production

# Approvers approve the request
hitch approvals approve <request-id> "Ready for production"

# View pending requests
hitch approvals list --status pending
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for detailed approval workflow documentation.

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

MIT License
