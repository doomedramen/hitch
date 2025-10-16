# Hitch Architecture

This document describes the technical architecture and design decisions for Hitch.

## Overview

Hitch is a CLI tool written in Go that wraps Git operations to provide a higher-level abstraction for managing multi-environment workflows.

## Core Concepts

### 1. Metadata Branch

Hitch uses a special Git branch called `hitch-metadata` to store state. This branch:

- Is an **orphan branch** (no shared history with other branches)
- Contains a single file: `hitch.json`
- Tracks which features are in which environments
- Records branch lifecycle information
- Stores configuration

**Why an orphan branch?**
- ✅ Lives in the same repository (no external dependencies)
- ✅ Synchronized via normal Git push/pull
- ✅ Version controlled with full history
- ✅ Isolated from working branches (won't appear in merge conflicts)
- ✅ Can be accessed from any branch

### 2. Ephemeral Integration Branches

`dev` and `qa` are **rebuilt** rather than merged into:

```
Traditional approach (problematic):
feature/a → dev
feature/b → dev
dev → qa
feature/c → dev
qa → main  (now dev and main have diverged!)

Hitch approach (clean):
dev = fresh main + [feature/a, feature/b, feature/c]
qa = fresh main + [feature/a, feature/b]
main = source of truth
```

Integration branches are **force-pushed** after every rebuild. This means:
- ⚠️ Never commit directly to `dev` or `qa`
- ⚠️ Never make feature branches from `dev` or `qa`
- ✅ Always branch from `main`

### 3. Locking Mechanism

To prevent race conditions when multiple developers run Hitch commands simultaneously:

1. **Acquire lock** (write to metadata: `"locked": true, "locked_by": "user@example.com"`)
2. **Perform operation** (rebuild branch, merge, etc.)
3. **Release lock** (write to metadata: `"locked": false`)

If a lock is held:
- Other Hitch commands wait or fail gracefully
- Lock includes timestamp to detect stale locks
- Manual override available: `hitch unlock --force`

### 4. Branch Lifecycle Tracking

Hitch tracks each feature branch through its lifecycle:

```
Created → Promoted to dev → Promoted to qa → Released to main → Eligible for cleanup
```

States are recorded in metadata with timestamps, allowing:
- Stale branch detection
- Cleanup after merge + retention period
- Audit trail of what was deployed when

## Data Model

### Metadata Schema

See [METADATA.md](./METADATA.md) for the complete JSON schema. Key sections:

- **environments**: Lists features in each environment with lock state
- **branches**: Tracks lifecycle and promotion history
- **config**: User-configurable settings

### Git Operations

Hitch performs these Git operations:

1. **Checkout hitch-metadata**: `git checkout hitch-metadata`
2. **Read metadata**: Parse `hitch.json`
3. **Modify metadata**: Update JSON in memory
4. **Write metadata**: Write `hitch.json`, commit, push
5. **Build environment**:
   ```bash
   git checkout main
   git checkout -B dev  # Create/reset branch
   git merge --no-ff feature/a
   git merge --no-ff feature/b
   git push --force-with-lease origin dev
   ```

## Project Structure

```
hitch/
├── cmd/
│   └── hitch/
│       └── main.go              # CLI entry point, command parsing
├── internal/
│   ├── metadata/
│   │   ├── metadata.go          # Core metadata types
│   │   ├── reader.go            # Read from hitch-metadata branch
│   │   ├── writer.go            # Write to hitch-metadata branch
│   │   └── lock.go              # Locking logic
│   ├── git/
│   │   ├── repo.go              # Git repository wrapper
│   │   ├── branch.go            # Branch operations
│   │   ├── merge.go             # Merge operations
│   │   └── remote.go            # Push/pull operations
│   ├── environment/
│   │   ├── builder.go           # Rebuild environment branches
│   │   ├── promote.go           # Promote feature to environment
│   │   └── demote.go            # Remove feature from environment
│   ├── release/
│   │   └── release.go           # Merge feature to main
│   ├── cleanup/
│   │   ├── scanner.go           # Find stale branches
│   │   └── pruner.go            # Delete branches
│   └── config/
│       └── config.go            # Configuration management
├── pkg/
│   └── types/
│       └── types.go             # Public types
├── hooks/
│   └── pre-push                 # Git hook to enforce locks
├── go.mod
├── go.sum
└── README.md
```

## Safety Guarantees

### 1. Metadata Branch Initialization

When `hitch init` runs, it creates a **clean orphan branch** with NO files from the working directory:

```bash
# What hitch init does:
git checkout --orphan hitch-metadata
git rm -rf .                    # Remove ALL files from index
# Create hitch.json from scratch
git add hitch.json
git commit -m "Initialize Hitch metadata"
git push -u origin hitch-metadata

# Then return to original branch
git checkout main
```

**Why orphan branch:**
- No shared history with application code
- Contains ONLY `hitch.json` (and optionally `README.md`)
- Fast to checkout (no large files)
- Clean separation of concerns

### 2. Always Return to Original Branch

**CRITICAL:** Every Hitch command must restore the user's working state.

```go
func ExecuteCommand() error {
    // 1. Capture current state
    originalBranch, err := git.CurrentBranch()
    if err != nil {
        return err
    }

    isDetached := git.IsDetachedHead()
    originalCommit := ""
    if isDetached {
        originalCommit, _ = git.CurrentCommitSHA()
    }

    // 2. ALWAYS return to original state (even on error)
    defer func() {
        if isDetached {
            git.Checkout(originalCommit)
        } else {
            git.Checkout(originalBranch)
        }
    }()

    // 3. Perform Hitch operations
    // ... metadata reads, branch rebuilds, etc ...

    return nil
}
```

**Edge cases handled:**
- ✅ Detached HEAD state (return to commit SHA)
- ✅ Command fails mid-operation (defer ensures return)
- ✅ Uncommitted changes (preserved, no stashing needed)
- ✅ User on `hitch-metadata` branch (returns there)

### 3. Safe Environment Rebuilding with Temp Branches

**CRITICAL: This is the DEFAULT and ONLY way Hitch rebuilds environments.**

Every rebuild uses a temporary branch to ensure the original environment stays intact until success:

```go
func RebuildEnvironment(env string) error {
    // 1. Lock environment
    lock := AcquireLock(env)
    defer lock.Release()

    // 2. Read metadata
    meta := ReadMetadata()
    features := meta.Environments[env].Features

    // 3. Create TEMP branch from main (not env directly)
    tempBranch := env + "-hitch-temp"
    git.Checkout("main")
    git.Pull("origin", "main")
    git.CheckoutNewBranch(tempBranch)

    // 4. Merge features into TEMP branch
    for _, feature := range features {
        err := git.Merge(feature, "--no-ff")
        if err != nil {
            // CONFLICT! Cleanup temp and abort
            // Original env branch is UNCHANGED
            git.Checkout("main")
            git.DeleteBranch(tempBranch, "--force")
            return ConflictError{
                Branch: feature,
                Environment: env,
                Message: "Merge conflict - original branch unchanged",
            }
        }
    }

    // 5. All merges succeeded! Now swap branches
    git.Checkout("main")

    // Delete old env branch locally
    git.DeleteBranch(env, "--force")

    // Rename temp to env
    git.RenameBranch(tempBranch, env)

    // 6. Force push (with lease for safety)
    err := git.Push("--force-with-lease", "origin", env)
    if err != nil {
        return PushError{Environment: env}
    }

    return nil
}
```

**Safety features:**
- ✅ Original `dev`/`qa` branch untouched until rebuild succeeds
- ✅ If ANY merge fails, temp is deleted, original preserved
- ✅ Temp branch can be inspected if needed for debugging
- ✅ `--force-with-lease` prevents overwriting remote changes

### 4. Dry Run Support

All destructive operations support `--dry-run`:

```go
func RebuildEnvironment(env string, dryRun bool) error {
    // ... same setup ...

    for _, feature := range features {
        if dryRun {
            // Simulate merge without actually merging
            conflicts := git.CheckMergeable(feature)
            if conflicts {
                fmt.Printf("✗ Would conflict: %s\n", feature)
            } else {
                fmt.Printf("✓ Would merge: %s\n", feature)
            }
        } else {
            // Actually merge
            err := git.Merge(feature)
            if err != nil {
                return ConflictError{feature, env}
            }
        }
    }

    if dryRun {
        fmt.Println("\nDry run complete. No changes made.")
        return nil
    }

    // ... continue with actual operations ...
}
```

## Key Algorithms

### Environment Rebuild (Production Version)

See "Safety Guarantees" above for the safe temp-branch approach. Legacy algorithm shown for reference:

```go
// LEGACY - DO NOT USE
// This rebuilds directly on env branch (unsafe)
func RebuildEnvironmentUnsafe(env string) error {
    git checkout main
    git pull origin main
    git checkout -B {{env}}  // Destroys existing env!

    for _, feature := range features {
        git.Merge(feature)  // If this fails, env is broken!
    }

    git.Push("--force-with-lease", "origin", env)
    return nil
}
```

### Promote Feature

```go
func PromoteFeature(branch, env string) error {
    // 1. Validate branch exists
    if !git.BranchExists(branch) {
        return BranchNotFoundError{branch}
    }

    // 2. Lock and read metadata
    lock := AcquireLock(env)
    defer lock.Release()

    meta := ReadMetadata()

    // 3. Add to feature list (if not already present)
    features := meta.Environments[env].Features
    if !Contains(features, branch) {
        features = append(features, branch)
        meta.Environments[env].Features = features
    }

    // 4. Update branch tracking
    if meta.Branches[branch] == nil {
        meta.Branches[branch] = &BranchInfo{
            CreatedAt: Now(),
        }
    }
    meta.Branches[branch].PromotedTo = AddUnique(
        meta.Branches[branch].PromotedTo,
        env,
    )

    // 5. Write metadata
    WriteMetadata(meta)

    // 6. Rebuild environment
    return RebuildEnvironment(env)
}
```

### Stale Branch Detection

```go
func FindStaleBranches() []string {
    meta := ReadMetadata()
    stale := []string{}

    for branch, info := range meta.Branches {
        // Safe to delete if merged + past retention period
        if info.MergedToMainAt != nil {
            daysSinceMerge := Now().Sub(info.MergedToMainAt).Days()
            if daysSinceMerge > meta.Config.RetentionDaysAfterMerge {
                // Check not in any environment
                if !IsInAnyEnvironment(branch, meta) {
                    stale = append(stale, branch)
                }
            }
        }

        // Warn about inactive branches
        if info.MergedToMainAt == nil {
            lastCommit := git.GetLastCommitDate(branch)
            daysSinceCommit := Now().Sub(lastCommit).Days()
            if daysSinceCommit > meta.Config.StaleDaysNoActivity {
                stale = append(stale, branch)
            }
        }
    }

    return stale
}
```

## Error Handling

### Merge Conflicts

When rebuilding an environment, merge conflicts may occur:

1. **Detect**: `git merge` returns non-zero exit code
2. **Abort**: `git merge --abort` to restore clean state
3. **Report**: Show which branch caused conflict
4. **Suggest**: Developer should rebase feature branch on main

```bash
$ hitch promote feature/xyz to qa
Error: Merge conflict when adding feature/xyz to qa

The branch feature/xyz conflicts with the current qa environment.

To resolve:
  1. git checkout feature/xyz
  2. git rebase main
  3. Resolve conflicts
  4. git push --force-with-lease
  5. hitch promote feature/xyz to qa
```

### Concurrent Modifications

If two developers run Hitch commands simultaneously:

1. **First command** acquires lock
2. **Second command** sees locked state, waits briefly
3. **If still locked**, fails with:
   ```
   Error: qa environment is locked by dev-m@example.com (since 2025-10-16 10:30:00)

   If this lock is stale, run: hitch unlock qa --force
   ```

### Stale Locks

Lock includes timestamp. If lock age > 15 minutes:

```bash
$ hitch status
Warning: qa environment has stale lock from dev-m@example.com (locked 45 minutes ago)

Run: hitch unlock qa --force
```

## Dependencies

- **go-git**: Pure Go git implementation
- **cobra**: CLI framework
- **survey**: Interactive prompts
- **chalk**: Terminal colors

## Testing Strategy

1. **Unit tests**: Test each package in isolation
2. **Integration tests**: Test against real Git repositories (in temp directories)
3. **End-to-end tests**: Shell scripts that exercise full workflows
4. **Conflict simulation**: Deliberately create conflicts to verify error handling

## Security Considerations

- **Force push**: Integration branches are force-pushed. Prevent accidental work loss via:
  - Clear documentation
  - Git hooks that warn on direct commits
  - Branch protection rules on server

- **Lock bypass**: `--force` flags should be used cautiously
  - Log all force operations
  - Require confirmation

- **Metadata integrity**: Protect `hitch-metadata` branch via:
  - Branch protection on server
  - Only allow Hitch to modify it

## Performance

- **Metadata reads**: Cached in memory during single command execution
- **Git operations**: Use `go-git` library (no shell execution overhead)
- **Parallel rebuilds**: Not supported (locking prevents)
- **Large repos**: All operations are O(n) where n = number of features in environment

## Future Enhancements

- **Conflict prediction**: Analyze commits before merging to predict conflicts
- **Rollback**: Quickly remove last-added feature from environment
- **Web UI**: Visualize environment state and branch relationships
- **Notifications**: Slack/email when environment rebuilt
- **GitHub/GitLab integration**: Update PR status when promoted
- **Cherry-pick support**: Allow selective commit inclusion
