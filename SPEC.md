# Hitch CLI Tool Specification

## Global Flags

- `--verbose` – print detailed step-by-step logs for commands.
- `--no-push` – skip automatic pushes when metadata is committed. Can be used in `lock`, `unlock`, `add`, `remove`, `promote`, `demote`, `rebuild`, etc.

---

## Reusable Blocks

### pre-check()

- Ensure the current directory is a Git repository.
- Ensure the working tree is clean (no unstaged or uncommitted changes).

### access_metadata(closure = null)

- Fetch latest `hitch-metadata` from remote: `git fetch origin hitch-metadata`.
- Temporarily switch to the `hitch-metadata` branch using `switch-to`.
- Load and parse `hitch.json`.
- If a closure is provided, execute it with the metadata object (for modification).
  - Any changes should be committed and optionally pushed (warn if push fails or skip with `--no-push`).
- Return metadata (updated if modified, or original if read-only).
- Always switch back to the original branch afterward.

**Usage Examples:**

- Display metadata (read-only):

```rust
let metadata = access_metadata(None);
display(metadata);
```

- Modify metadata (add/remove/promote/demote/lock/unlock):

```rust
access_metadata(Some(|metadata| {
    metadata.environments.push(new_env);
    commit_and_push(metadata)?;
}));
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

- `access_metadata(Some(|metadata| { ... }))`

  - Mark `env_name` as locked in `hitch.json`.
  - Set `lockedBy` to `git config user.email`.
  - Set `lockedAt` to current timestamp.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

### unlock(env_name)

- `access_metadata(Some(|metadata| { ... }))`

  - Mark `env_name` as unlocked.
  - Clear `lockedBy` and `lockedAt`.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

### rebuild(environment)

- **Preconditions:**

  - Ensure the environment exists and is not currently locked.

- **Step 1: Lock the environment**

  - Call `lock(environment)` to prevent other operations during rebuild.

- **Step 2: Prepare temp branch**

  - Create a temporary branch named `hitch-tmp-${source}-${timestamp}` from the environment's source branch.

- **Step 3: Merge branches into temp branch**

  - Merge each branch listed in `environment.branches` in order using squash-merge.
  - **If merging into the temp branch fails:**

    - Abort the rebuild.
    - Delete the temp branch.
    - Unlock the environment (`unlock(environment)`).
    - No changes are made to the real environment branch.

- **Step 4: Merge temp branch into the real environment branch**

  - Rename the current environment branch to `${env}_backup_${timestamp}` for safety.
  - Create a new branch with the original environment name from the temp branch.
  - **If merging into the real environment branch fails:**

    - Restore the backup branch to the original environment name.
    - Delete the temp branch.
    - Unlock the environment (`unlock(environment)`).
    - Abort rebuild.

  - **If merging into the real environment branch succeeds:**

    - Delete the temp branch.
    - Delete the renamed backup branch `${env}_backup_${timestamp}` as it is no longer needed.
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
4. `access_metadata(Some(|metadata| { ... }))`

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

### add <env_name> [--source <branch>]

- Example: `hitch add staging --source main`
- `pre-check()`
- `access_metadata(Some(|metadata| { ... }))`

  - Add environment to `hitch.json` with optional source.
  - Initialize `locked = false`, `lockedBy = null`, `lockedAt = null`, `rebuiltAt = null`.
  - Commit and optionally push metadata (warn if push fails or skip with `--no-push`).

---

### remove <env_name> [--force]

- Example: `hitch remove staging --force`
- `pre-check()`
- `access_metadata(Some(|metadata| { ... }))`

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
- Read environment branch names from `access_metadata(None)`.
- Abort or warn if the current branch matches any environment branch.

---

### status

- Example: `hitch status`
- Call `access_metadata(None)`
- Display formatted environment information: name, source, branches, locked state, lockedBy, lockedAt, rebuiltAt.

---

### Notes on Reusable Steps

- `pre-check()` – used by all commands.
- `switch-to()` with closure – centralizes branch switching and automatic return.
- `access_metadata()` – unified block for all metadata access, read-only or read-write.
- `with_locked_env()` – ensures environment is locked during modifications and rebuilds, unlocking even on failure.
- `rebuild()` – triggered automatically by `promote` / `demote` and ensures environment is locked during operation with clear handling for temp branches, backup branches, success and failure scenarios.
- `safe_branch_operation()` – optional wrapper to consolidate temp/backup branch handling logic inside `rebuild()`.
