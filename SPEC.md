# Hitch CLI Tool Specification

Hitch is a Git branch management tool for environment-based deployments. It provides structured promotion workflows, environment locking, and rebuild automation for deployment branches (like `dev`, `qa`, `main`).

## Global Flags

- `--verbose` – print detailed step-by-step logs for commands.
- `--no-push` – skip automatic pushes when metadata is committed. Can be used in `lock`, `unlock`, `add`, `remove`, `promote`, `demote`, `rebuild`, etc.

---

## Reusable Blocks

### pre-check()

- Ensure the current directory is a Git repository.
- Ensure the working tree is clean (no unstaged or uncommitted changes).

### access_metadata_read_only(closure)

- Make sure `hitch-metadata` is up to date: `git fetch origin hitch-metadata`.
- Use `git show hitch-metadata:hitch.json` to read metadata without branch switching.
- Parse `hitch.json`.
- Execute the provided closure with the metadata object.
- Works even with unclean git states and doesn't create unnecessary commits.

**Usage Example:**

```rust
let config = access_metadata_read_only(&context, |config: &HitchConfig| {
    Ok(config.clone()) // Return a copy for use in display
})?;
```

### modify_metadata(closure)

- Fetch latest `hitch-metadata` from remote: `git fetch origin hitch-metadata`.
- Temporarily switch to the `hitch-metadata` branch using `switch-to`.
- Load and parse `hitch.json`.
- Execute the provided closure with the metadata object (for modification).
- Commit and optionally push metadata (warn if push fails or skip with `--no-push`).
- Always switch back to the original branch afterward.

**Usage Example:**

```rust
modify_metadata(&context, |config: &mut HitchConfig| {
    config.environments.push(new_env);
    Ok(())
})?;
```

### switch-to(branch, closure)

- Record current branch.
- Checkout target branch.
- Execute the provided closure while on the target branch.
- Always attempt to switch back to the original branch after the closure runs, even if it fails.
- In Rust, this can be implemented with a closure that returns `Result`.

### with_locked_env(env_name, closure)

- Calls `lock(env_name)` → executes closure → calls `unlock(env_name)` even if closure fails.
- Ensures environment is safely locked during modifications.
- Automatically handles warnings if push fails.

### lock(env_name)

- `modify_metadata(|metadata| { ... })`

  - Mark `env_name` as locked in `hitch.json`.
  - Set `lockedBy` to `git config user.email`.
  - Set `lockedAt` to current timestamp.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

### unlock(env_name)

- `modify_metadata(|metadata| { ... })`

  - Mark `env_name` as unlocked.
  - Clear `lockedBy` and `lockedAt`.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

### rebuild(environment)

- **Preconditions:**

  - Ensure the environment exists and is not currently locked.

- **Step 1: Lock the environment**

  - Call `lock(environment)` to prevent other operations during rebuild.

- **Step 2: Prepare temp branch**

  - Create a temporary branch named `hitch-tmp-${base}-${timestamp}` from the environment's base branch.

- **Step 3: Merge branches into temp branch**

  - Merge each branch listed in `environment.branches` in order using squash-merge.
  - **If merging into the temp branch fails:**

    - Abort the rebuild.
    - Delete the temp branch.
    - Unlock the environment (`unlock(environment)`).
    - No changes are made to the real environment branch.

- **Step 4: Merge temp branch into the real environment branch**

  - Rename the current environment branch to `hitch-backup-${env}-${timestamp}` for safety.
  - Create a new branch with the original environment name from the temp branch.
  - **If merging into the real environment branch fails:**

    - Restore the backup branch to the original environment name.
    - Delete the temp branch.
    - Unlock the environment (`unlock(environment)`).
    - Abort rebuild.

  - **If merging into the real environment branch succeeds:**

    - Delete the temp branch.
    - Delete the renamed backup branch `hitch-backup-${env}-${timestamp}` as it is no longer needed.
    - Update `rebuiltAt` timestamp in `hitch.json`.
    - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

- **Step 5: Unlock the environment**

  - Call `unlock(environment)` to ensure the environment is unlocked even if rebuild partially fails.

- **Optional:** Could use `safe_branch_operation(temp_branch, closure)` to wrap temp/backup branch handling and rollback logic.

---

## Commands

### init [--environments <list>]

- Example: `hitch init --environments dev,qa`

1. `pre-check()`
2. Ensure `hitch-metadata` branch does not exist.
3. Create orphan branch `hitch-metadata`.
4. `modify_metadata(|metadata| { ... })`

   - Remove all files/folders except `.git`.
   - Create `.gitignore`:

     ```
     *
     !.gitignore
     !hitch.json
     ```

   - Create `hitch.json` skeleton, optionally pre-populate environments.
   - Stage, commit, and optionally push `.gitignore` and `hitch.json` (warn if push fails or skip with `--no-push`).

---

### release <env_name> <target_branch> [--force]

- Example: `hitch release production main`
- Example: `hitch release staging develop --force`

**Purpose**: Permanently merge all promoted branches from an environment to a target branch. This is a one-way operation that creates a release.

- `pre-check()`
- Validate environment exists and is ready for release
- Resolve target branch (use override or environment base)
- User confirmation (skip with `--force`)
- **Lock the environment** (unless using `--force` on already locked environment)
- Record original branch for return after operation
- Synchronize all branches (promoted branches + target branch)
- Switch to target branch
- For each promoted branch in the environment:
  - Check for merge conflicts
  - If conflicts found: abort release, unlock environment, return to original branch
  - Perform squash merge with release-specific commit message
- Commit the merged changes with release message
- Create auto-tag for release tracking (format: `hitch-release-{env}-to-{target}-{timestamp}`)
- Push changes and tag if enabled
- Return to original branch
- Update release timestamp in environment metadata
- **Unlock the environment**

**Key Differences from rebuild**:
- `rebuild`: Creates/maintains environment branch for ongoing deployments
- `release`: One-way merge to target branch, creates release tags
- `rebuild`: Merges to environment's base branch
- `release`: Merges to specified target branch (can be any branch)

---

### add <env_name> [--base <branch>]

- Example: `hitch add staging --base main`
- `pre-check()`
- `modify_metadata(|metadata| { ... })`

  - Add environment to `hitch.json` with optional base.
  - Initialize `locked = false`, `lockedBy = null`, `lockedAt = null`, `rebuiltAt = null`.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

---

### remove <env_name> [--force]

- Example: `hitch remove staging --force`
- `pre-check()`
- `modify_metadata(|metadata| { ... })`

  - Confirm if `--force` not provided.
  - Remove environment from `hitch.json`.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

- **Note:** does not delete Git branches.

---

### promote <branch> to <env_name>

- Example: `hitch promote feature/login to dev`
- `pre-check()`
- `with_locked_env(env_name, || { ... })`

  - Add branch to `env_name.branches` array.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).
  - Trigger `rebuild(env_name)`.

---

### demote <branch> from <env_name>

- Example: `hitch demote feature/login from dev`
- `pre-check()`
- `with_locked_env(env_name, || { ... })`

  - Remove branch from `env_name.branches` array.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).
  - Trigger `rebuild(env_name)`.

---

### guard

- Example: use in pre-commit hook: `hitch guard`
- Read environment branch names from `access_metadata_read_only(|config| { ... })`.
- Abort or warn if the current branch matches any environment branch.

---

### status

- Example: `hitch status`
- Call `access_metadata_read_only(|config| { ... })`
- Display formatted environment information: name, base, branches, locked state, lockedBy, lockedAt, rebuiltAt.
- Also let us know if there are any environments that need to be rebuilt due to there being changes in one of their branches since the last build.
- Works even with unclean git states since it uses `git show hitch-metadata:hitch.json` approach.

---

### Notes on Reusable Steps

- `pre-check()` – used by all commands.
- `switch-to()` with closure – centralizes branch switching and automatic return.
- `access_metadata_read_only()` – read-only metadata access without branch switching, works with unclean git states.
- `modify_metadata()` – read-write metadata access with automatic branch switching and commit handling.
- `with_locked_env()` – ensures environment is locked during modifications and rebuilds, unlocking even on failure.
- `rebuild()` – triggered automatically by `promote` / `demote` and ensures environment is locked during operation with clear handling for temp branches, backup branches, success and failure scenarios.
- `safe_branch_operation()` – optional wrapper to consolidate temp/backup branch handling logic inside `rebuild()`.
