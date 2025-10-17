# Hitch

**A Git workflow manager for multi-environment development teams**

Hitch simplifies managing feature branches across multiple deployment environments (dev, qa, production) by treating environment branches as **hitched branches** - ephemeral, reconstructible branches that are "hitched" to a specific feature list rather than having permanent independent histories.

## The Problem

Traditional multi-environment Git workflows lead to:

- **Divergent histories** between `dev`, `qa`, and `main` branches
- **Merge conflicts** when trying to sync environments
- **Stale feature branches** that fall out of sync with main
- **Manual cherry-picking** to untangle mixed changes
- **Periodic branch deletion** to reset environments and restore sanity

### Example of the Mess

```
Developer M: feature/xyz → dev → qa → main
Developer S: feature/abc → dev
Developer R: feature/lmp (branched from old main, now out of sync!)
```

After a few iterations, `dev` and `qa` have different histories than `main`, feature branches are stale, and the team resorts to deleting and recreating environment branches.

## The Hitch Solution

Hitch treats `dev` and `qa` as **hitched branches** - branches that are "hitched" to a specific feature list:

```
qa = main + feature/xyz + feature/lmp + bug/473
dev = main + feature/xus + feature/lmp + bug/473
```

Key principles:

1. **`main` is the source of truth** - all feature branches originate from main
2. **Hitched branches are rebuilt** - `dev` and `qa` are reconstructed on-demand from their feature lists
3. **Metadata tracks state** - a special `hitch-metadata` branch stores which features are in each environment
4. **Automatic locking** - prevents concurrent modifications during rebuilds
5. **Lifecycle management** - identifies and cleans up stale branches

## Features

- 🔄 **Environment promotion**: Move features between dev → qa → main
- 🔒 **Automatic locking**: Prevents race conditions during rebuilds
- 🧹 **Stale branch cleanup**: Identifies branches safe to delete
- 📊 **Status overview**: See which features are in each environment
- 🎯 **Merge conflict detection**: Alerts when features conflict
- 🪝 **Git hook integration**: Optional `hitch hook` commands for your existing hooks

## Installation

```bash
# Download pre-built binary (coming soon)
curl -sSL https://github.com/DoomedRamen/hitch/releases/latest/download/hitch-$(uname -s)-$(uname -m) -o /usr/local/bin/hitch
chmod +x /usr/local/bin/hitch

# Or build from source
git clone https://github.com/DoomedRamen/hitch.git
cd hitch
go build -o hitch ./cmd/hitch
mv hitch /usr/local/bin/
```

## Quick Start

```bash
# Initialize Hitch in your repository
cd your-repo
hitch init

# Create a feature branch
git checkout -b feature/new-login
# ... make changes ...
git push origin feature/new-login

# Promote to dev environment
hitch promote feature/new-login to dev

# Check status
hitch status

# Promote to qa
hitch promote feature/new-login to qa

# Release to main
hitch release feature/new-login

# Clean up stale branches
hitch cleanup --dry-run
hitch cleanup
```

## Core Commands

- `hitch init` - Initialize Hitch in a repository
- `hitch status` - Show current environment state
- `hitch promote <branch> to <env>` - Add a feature to an environment
- `hitch demote <branch> from <env>` - Remove a feature from an environment
- `hitch release <branch>` - Merge a feature to main
- `hitch rebuild <env>` - Reconstruct an environment from scratch
- `hitch cleanup` - Delete stale branches
- `hitch lock <env>` / `hitch unlock <env>` - Manual lock management
- `hitch hook pre-push` - Git hook integration (optional)

See [COMMANDS.md](./COMMANDS.md) for full reference and [HOOKS.md](./HOOKS.md) for hook integration.

## How It Works

Hitch maintains a special `hitch-metadata` orphan branch containing:

```json
{
  "environments": {
    "dev": {
      "base": "main",
      "features": ["feature/xus", "feature/lmp", "bug/473"],
      "locked": false
    },
    "qa": {
      "base": "main",
      "features": ["feature/xyz", "feature/lmp", "bug/473"],
      "locked": false
    }
  },
  "branches": {
    "feature/xyz": {
      "created_at": "2025-10-01T09:00:00Z",
      "promoted_to": ["dev", "qa"],
      "merged_to_main_at": null
    }
  },
  "config": {
    "retention_days_after_merge": 7,
    "stale_days_no_activity": 30
  }
}
```

When you run `hitch promote feature/xyz to qa`, Hitch:

1. Locks the `qa` environment
2. Checks out a fresh `main` branch
3. Merges all features in order: `main` + `feature/xyz` + `feature/lmp` + `bug/473`
4. Force-pushes the rebuilt `qa` branch
5. Updates metadata
6. Unlocks the environment

See [ARCHITECTURE.md](./ARCHITECTURE.md) for technical details.

## Why "Hitch"?

A hitch is a type of knot used to temporarily attach a rope to an object. Like the tool, it's about temporary connections (features to environments) that are easy to tie and untie, rather than permanent tangles.

## Contributing

Contributions welcome! Please read [CONTRIBUTING.md](./CONTRIBUTING.md) first.

## License

MIT License - see [LICENSE](./LICENSE)

## Author

Martin Page ([@DoomedRamen](https://github.com/DoomedRamen)) - m@rtin.page
