# Hitch Safety Guarantees

This document describes the safety mechanisms built into Hitch to prevent data loss and ensure a predictable user experience.

## Core Safety Principles

1. **Non-destructive by default** - Original branches are preserved until operations succeed
2. **Always return to original state** - Your working directory is never left in an unexpected state
3. **Fail safely** - Errors abort operations cleanly without partial changes
4. **Transparent operations** - You always know what Hitch is doing

---

## 1. Metadata Branch Isolation

### Guarantee

The `hitch-metadata` branch contains **ONLY** Hitch-specific files, never your application code.

### Implementation

When `hitch init` runs:

```bash
git checkout --orphan hitch-metadata  # Create branch with no history
git rm -rf .                          # Remove ALL files from index
# Create hitch.json from scratch
git add hitch.json
git commit -m "Initialize Hitch metadata"
git push -u origin hitch-metadata
```

### What This Means

- ✅ Metadata branch is tiny (only JSON files)
- ✅ Fast checkout (no large application files)
- ✅ No shared history with application code
- ✅ Can't accidentally pollute with application files
- ✅ Clean separation of concerns

### Edge Cases Handled

- **Running `hitch init` in repo with thousands of files**: Only `hitch.json` is added, all other files are excluded
- **Re-running `hitch init`**: Detects existing metadata branch and asks for confirmation before overwriting

---

## 2. Always Return to Original Branch

### Guarantee

**No matter what command you run, you will end up on the same branch you started on.**

### Implementation

Every Hitch command uses this pattern:

```go
func ExecuteCommand() error {
    // 1. Capture current state FIRST
    originalBranch, _ := git.CurrentBranch()
    isDetached := git.IsDetachedHead()
    originalCommit := ""
    if isDetached {
        originalCommit, _ = git.CurrentCommitSHA()
    }

    // 2. ALWAYS return (even on error, panic, interrupt)
    defer func() {
        if isDetached {
            git.Checkout(originalCommit)
        } else {
            git.Checkout(originalBranch)
        }
    }()

    // 3. Do actual work
    // ... checkout hitch-metadata, main, dev, etc ...

    return nil
}
```

### What This Means

- ✅ You start on `feature/my-work`, you end on `feature/my-work`
- ✅ Works even if command fails mid-execution
- ✅ Works even with Ctrl+C interrupt (defer still runs)
- ✅ Preserves detached HEAD state
- ✅ Never requires manual cleanup

### Edge Cases Handled

| Starting State | Hitch Operation | Ending State |
|----------------|-----------------|--------------|
| `feature/xyz` | `hitch promote feature/abc to dev` | `feature/xyz` |
| `main` | `hitch rebuild qa` | `main` |
| Detached HEAD at `a1b2c3d` | `hitch status` | Detached HEAD at `a1b2c3d` |
| `feature/xyz` with uncommitted changes | `hitch promote ...` | `feature/xyz` with uncommitted changes |
| `hitch-metadata` (unusual) | `hitch rebuild dev` | `hitch-metadata` |

### What About Uncommitted Changes?

**Hitch never stashes or touches your uncommitted changes.**

Git's checkout is smart enough to preserve uncommitted changes when switching branches if there are no conflicts. If there would be conflicts, Git will refuse the checkout and Hitch will error (which is correct behavior).

Example:
```bash
$ git status
On branch feature/my-work
Changes not staged for commit:
  modified: src/app.js

$ hitch promote feature/other to dev
# ... hitch works ...

$ git status
On branch feature/my-work  # ← Back on original branch
Changes not staged for commit:
  modified: src/app.js     # ← Uncommitted changes preserved
```

---

## 3. Safe Environment Rebuilding

### Guarantee

**Environment branches (dev, qa) are never destroyed until rebuild succeeds.**

### The Problem This Solves

Naive approach (DANGEROUS):
```bash
git checkout -B dev  # ← Immediately destroys existing dev branch!
git merge feature/a  # ← If this fails, dev is now broken!
```

### Hitch's Safe Approach (ALWAYS USED)

```bash
git checkout main
git checkout -b dev-hitch-temp  # ← Create TEMP branch
git merge feature/a             # ← Merge into temp
git merge feature/b             # ← If this fails, temp is broken but dev is fine!
# ... if all succeed ...
git branch -D dev               # ← Only NOW delete original dev
git branch -m dev-hitch-temp dev  # ← Rename temp to dev
```

### What This Means

- ✅ Original `dev` branch untouched until ALL merges succeed
- ✅ If ANY merge fails, original `dev` is preserved
- ✅ Temp branch is automatically cleaned up on error
- ✅ No "half-broken" state possible

### Implementation

```go
func RebuildEnvironment(env string) error {
    tempBranch := env + "-hitch-temp"

    // Create temp from main
    git.Checkout("main")
    git.Pull("origin", "main")
    git.CheckoutNewBranch(tempBranch)

    // Try merging all features
    for _, feature := range features {
        err := git.Merge(feature, "--no-ff")
        if err != nil {
            // CONFLICT! Cleanup and abort
            git.Checkout("main")
            git.DeleteBranch(tempBranch, "--force")
            return ConflictError{feature, env}
        }
    }

    // ALL merges succeeded! Now swap
    git.Checkout("main")
    git.DeleteBranch(env, "--force")        // Safe now
    git.RenameBranch(tempBranch, env)       // Promote temp
    git.Push("--force-with-lease", "origin", env)

    return nil
}
```

### Edge Cases Handled

| Scenario | Behavior |
|----------|----------|
| First feature merges, second conflicts | Temp deleted, original `dev` unchanged, error reported |
| All merges succeed, push fails | Local `dev` updated, error suggests manual push |
| Network interruption during push | Local `dev` updated, can re-push manually |
| User interrupts with Ctrl+C during merge | Temp branch may remain, can be safely deleted with `git branch -D dev-hitch-temp` |

---

## 4. Optimistic Concurrency Control

### Guarantee

**Multiple developers can't overwrite each other's metadata changes.**

### The Problem

Two developers run commands simultaneously:
```
Dev A: Read metadata → Modify → Write
Dev B: Read metadata → Modify → Write  ← Overwrites Dev A's changes!
```

### Hitch's Solution: Force-with-Lease

```bash
# Read metadata at commit abc123
git checkout hitch-metadata  # at commit abc123

# Modify metadata
# ... edit hitch.json ...
git commit -m "Updated metadata"  # creates commit def456

# Push with lease
git push --force-with-lease=hitch-metadata:abc123 origin hitch-metadata

# This push ONLY succeeds if remote is still at abc123
# If someone else pushed in the meantime, push fails
```

### What This Means

- ✅ Lost updates impossible
- ✅ Last-write doesn't silently win
- ✅ Conflicts detected and reported
- ✅ Users prompted to retry

### Error Message

```bash
$ hitch promote feature/xyz to dev
Error: Failed to update metadata

Another user modified the metadata since you started this operation.

Please retry the command. Your changes were not applied.
```

---

## 5. Environment Locking

### Guarantee

**Only one operation can modify an environment at a time.**

### Implementation

Metadata includes lock state:
```json
{
  "environments": {
    "dev": {
      "locked": true,
      "locked_by": "dev-m@example.com",
      "locked_at": "2025-10-16T10:30:00Z"
    }
  }
}
```

Lock acquisition:
```go
func AcquireLock(env string) error {
    meta := ReadMetadata()

    if meta.Environments[env].Locked {
        lockAge := time.Since(meta.Environments[env].LockedAt)

        if lockAge > 15*time.Minute {
            return StaleLockError{env, meta.Environments[env].LockedBy}
        }

        if meta.Environments[env].LockedBy != currentUser {
            return LockedError{env, meta.Environments[env].LockedBy}
        }
    }

    // Acquire lock
    meta.Environments[env].Locked = true
    meta.Environments[env].LockedBy = currentUser
    meta.Environments[env].LockedAt = time.Now()
    WriteMetadata(meta)

    return nil
}
```

### What This Means

- ✅ Race conditions prevented
- ✅ Stale locks detected (> 15 minutes)
- ✅ Same user can re-acquire their own lock
- ✅ Manual override available if needed

---

## 6. Force-Push Safety

### Guarantee

**Force pushes include safety checks to prevent overwriting unexpected changes.**

### Implementation

All force pushes use `--force-with-lease`:

```bash
# BAD: Overwrites whatever is on remote
git push --force origin dev

# GOOD: Only pushes if remote is at expected commit
git push --force-with-lease origin dev
```

### What This Means

If someone else pushed to `dev` between when you started and finished your rebuild, the push fails:

```bash
$ hitch rebuild dev
Error: Failed to push dev branch

The remote dev branch has changed since rebuild started.
This usually means another developer pushed to dev.

Your local dev branch has been rebuilt successfully.
To force push anyway: git push --force origin dev
```

---

## 7. Clean State Validation

### Guarantee

**Hitch fails if critical branches have uncommitted changes.**

### Which Branches Must Be Clean

| Branch | Must Be Clean? | Why |
|--------|----------------|-----|
| Your current feature branch | ❌ No | Hitch doesn't modify it, Git preserves your changes |
| `hitch-metadata` | ✅ Yes | Hitch commits to this branch |
| `main` | ✅ Yes | Hitch pulls from origin, unsafe with uncommitted changes |
| `dev` / `qa` | ✅ Yes | Hitch deletes/recreates, uncommitted changes = broken workflow |

### Pre-Flight Check

Before every operation, Hitch validates:

```go
func ValidateCleanState() error {
    // 1. Check hitch-metadata
    if git.HasUncommittedChanges("hitch-metadata") {
        return Error{
            Message: "hitch-metadata branch has uncommitted changes",
            Solution: "Commit or stash changes before running Hitch",
        }
    }

    // 2. Check main
    if git.HasUncommittedChanges("main") {
        return Error{
            Message: "main branch has uncommitted changes",
            Solution: "Commit or stash changes in main before running Hitch",
        }
    }

    // 3. Check hitched branches
    for _, env := range config.Environments {
        if git.BranchExists(env) && git.HasUncommittedChanges(env) {
            return Error{
                Message: fmt.Sprintf("%s branch has uncommitted changes", env),
                Details: "Direct commits to hitched branches are not allowed",
            }
        }
    }

    return nil
}
```

### Error Example

```bash
$ git checkout dev
$ echo "oops" >> file.txt
$ git checkout feature/my-work

$ hitch promote feature/xyz to dev
Error: dev branch has uncommitted changes

The dev branch has uncommitted changes. This suggests someone
committed directly to the hitched branch, which breaks Hitch's workflow.

To preserve these changes:
  1. git checkout dev
  2. git checkout -b feature/preserve-dev-changes
  3. git push origin feature/preserve-dev-changes
  4. git checkout feature/my-work
  5. hitch promote feature/preserve-dev-changes to dev

To discard these changes:
  1. git checkout dev
  2. git reset --hard origin/dev
  3. git checkout feature/my-work
  4. Retry hitch command
```

### What About Your Current Branch?

**Your current branch CAN have uncommitted changes:**

```bash
$ git status
On branch feature/my-work
Changes not staged for commit:
  modified: src/app.js

$ hitch promote feature/other to dev
✓ Success!

$ git status
On branch feature/my-work
Changes not staged for commit:
  modified: src/app.js  # ← Still there!
```

Git's checkout is smart - it preserves uncommitted changes when possible.

---

## 8. Dry Run Mode

### Guarantee

**You can preview any destructive operation without making changes.**

### Commands Supporting Dry Run

- `hitch rebuild --dry-run`
- `hitch cleanup --dry-run`
- `hitch promote --dry-run` (future)
- `hitch release --dry-run` (future)

### What Dry Run Does

- ✅ Reads metadata
- ✅ Checks branch existence
- ✅ Analyzes merge conflicts
- ✅ Shows what WOULD happen
- ❌ Does NOT create branches
- ❌ Does NOT modify metadata
- ❌ Does NOT push to remote

### Example

```bash
$ hitch rebuild dev --dry-run

Dry run: simulating rebuild of dev environment

✓ Would checkout main (current commit: a1b2c3d)
✓ Would create temp branch: dev-hitch-temp
✓ Checking if features are mergeable:
  - feature/user-auth (mergeable, no conflicts predicted)
  - feature/dashboard (⚠ potential conflict in src/app.js)
  - bug/fix-login (mergeable, no conflicts predicted)

Warning: feature/dashboard may conflict. Consider rebasing on main first.

Dry run complete. No changes made.
```

---

## 8. Atomic Operations

### Guarantee

**Operations either complete fully or not at all. No partial states.**

### What Is Atomic

Each Hitch command is atomic:
- `hitch promote`: Either feature is promoted OR it's not
- `hitch release`: Either feature is merged to main OR it's not
- `hitch rebuild`: Either environment is rebuilt OR it's unchanged

### What Is NOT Atomic (Intentionally)

Cleanup of multiple branches:
```bash
$ hitch cleanup
Branches safe to delete:
  ✓ feature/a (merged 10 days ago)
  ✓ feature/b (merged 8 days ago)

Delete 2 branches? (y/N): y

Deleting branches...
  ✓ Deleted feature/a
  ✗ Failed to delete feature/b (network error)

Partially complete. 1 of 2 branches deleted.
Run 'hitch cleanup' again to retry.
```

This is intentional - cleanup is idempotent and safe to retry.

---

## 9. Idempotent Operations

### Guarantee

**Running the same command twice has the same effect as running it once.**

### Examples

```bash
# Promote already-promoted feature
$ hitch promote feature/xyz to dev
# ✓ feature/xyz is already in dev, no change needed

# Rebuild already-rebuilt environment
$ hitch rebuild dev
# ✓ Rebuilds dev (same result as current state)

# Delete already-deleted branch
$ hitch cleanup
# ✓ No branches to delete

# Release already-released feature
$ hitch release feature/xyz
# ✗ Error: feature/xyz is already merged to main
```

---

## 10. Error Recovery

### Guarantee

**Clear error messages with recovery instructions.**

### Example: Merge Conflict

```bash
$ hitch promote feature/xyz to qa
Error: Merge conflict when adding feature/xyz to qa

feature/xyz conflicts with the current qa environment.

Conflicting files:
  - src/auth/login.js
  - src/components/Header.tsx

Original qa branch is unchanged.
Temp branch qa-hitch-temp has been deleted.

To resolve:
  1. git checkout feature/xyz
  2. git rebase main
  3. Resolve conflicts in the files above
  4. git rebase --continue
  5. git push --force-with-lease origin feature/xyz
  6. hitch promote feature/xyz to qa

For help: hitch help promote
```

### Example: Stale Lock

```bash
$ hitch rebuild qa
Error: qa environment is locked

Locked by: dev-m@example.com
Locked at: 2025-10-16 10:30:00 (45 minutes ago)

This lock appears stale (older than 15 minutes).

To force unlock: hitch unlock qa --force
To wait: retry this command in a few minutes
```

---

## 11. Git Hook Safety

### Guarantee

**Hitch-installed hooks prevent direct commits to hitched branches.**

### Pre-Push Hook

```bash
#!/bin/bash
# Installed by Hitch

current_branch=$(git rev-parse --abbrev-ref HEAD)

# Check if pushing to hitched branch
if [[ "$current_branch" == "dev" ]] || [[ "$current_branch" == "qa" ]]; then
    echo "ERROR: Direct commits to $current_branch are not allowed"
    echo ""
    echo "Hitch manages this branch automatically."
    echo ""
    echo "Instead:"
    echo "  1. Create a feature branch: git checkout -b feature/my-feature"
    echo "  2. Make your changes there"
    echo "  3. Promote to $current_branch: hitch promote feature/my-feature to $current_branch"
    echo ""
    exit 1
fi

# Check if pushing to hitch-metadata manually
if [[ "$current_branch" == "hitch-metadata" ]]; then
    echo "WARNING: You're pushing to hitch-metadata manually"
    echo ""
    echo "Normally, Hitch manages this branch automatically."
    echo "Manual edits can break Hitch functionality."
    echo ""
    read -p "Are you sure? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi
```

---

## 12. Backup and Recovery

### Automatic Metadata Backups

Hitch can create automatic daily backups:

```bash
# Hitch creates tags automatically
hitch-backup-20251016
hitch-backup-20251017
# ... etc, retained for 30 days
```

### Manual Backup

```bash
# Create backup
git checkout hitch-metadata
git tag hitch-manual-backup-$(date +%Y%m%d-%H%M%S)
git push origin --tags
```

### Recovery from Backup

```bash
# List backups
git tag -l "hitch-backup-*"

# Restore
git checkout hitch-metadata
git reset --hard hitch-backup-20251016
git push --force-with-lease origin hitch-metadata
```

---

## Summary: What Could Go Wrong?

| Scenario | Is It Possible? | Why Not? |
|----------|-----------------|----------|
| Lose work on feature branch | ❌ No | Hitch never touches feature branches |
| Lose uncommitted changes | ❌ No | Hitch never stashes or modifies working directory |
| Break `dev` during failed rebuild | ❌ No | Temp branch approach - original preserved |
| Overwrite another dev's metadata | ❌ No | Force-with-lease prevents lost updates |
| Get stuck on wrong branch | ❌ No | Always returns to original branch |
| Corrupt `hitch-metadata` | ⚠️ Rare | Backups available, can restore |
| Accidentally commit to `dev` | ⚠️ Possible | Git hooks warn but can be bypassed |
| Force unlock stale lock incorrectly | ⚠️ Possible | User must use `--force` intentionally |

---

## Best Practices

### Do

- ✅ Trust Hitch to manage `dev` and `qa` branches
- ✅ Always branch from `main` for features
- ✅ Use `--dry-run` before destructive operations
- ✅ Run `hitch cleanup` regularly
- ✅ Install git hooks (`hitch init --install-hooks`)

### Don't

- ❌ Commit directly to `dev` or `qa`
- ❌ Branch from `dev` or `qa`
- ❌ Manually edit `hitch-metadata` branch
- ❌ Force unlock without checking with team
- ❌ Bypass git hooks without good reason

---

## Emergency Recovery

If something goes catastrophically wrong:

### 1. Your feature branch is safe
```bash
git checkout feature/my-work
# Your work is always preserved
```

### 2. Rebuild environments from scratch
```bash
# Environments can always be rebuilt from metadata
hitch rebuild dev
hitch rebuild qa
```

### 3. Restore metadata from backup
```bash
git checkout hitch-metadata
git reset --hard hitch-backup-20251016
git push --force-with-lease origin hitch-metadata
```

### 4. Nuclear option: Re-initialize
```bash
# Last resort: start fresh
hitch init --force
# Then manually re-promote features to environments
```

**Remember:** Feature branches and `main` are never touched by Hitch. The worst case is rebuilding hitched branches, which is always possible.
