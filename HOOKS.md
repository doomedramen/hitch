# Hitch Git Hooks Integration

Hitch provides hook commands that you can integrate into your existing Git hooks workflow.

## Philosophy

**Hitch does NOT install hooks automatically** to avoid conflicts with your existing setup.

Instead, Hitch provides commands you can call from your hooks:
- `hitch hook pre-push` - Check if branch is locked before pushing

## The `hitch hook` Commands

### `hitch hook pre-push`

Checks if the current branch is safe to push to.

**What it checks:**
1. Is the current branch a managed environment (dev, qa)?
2. Is the environment locked?
3. If locked, are you the lock holder?

**Exit codes:**
- `0` - Safe to push
- `1` - Push blocked (locked by someone else)

**Example output:**
```bash
$ hitch hook pre-push

# If locked by someone else:
❌ Cannot push to dev

Locked by: dev-m@example.com
Locked at: 2025-10-16 10:30:00

Wait for unlock or contact dev-m@example.com

# Exit code: 1

# If locked by you:
# (silent, exits 0)

# If not locked but is hitched branch:
⚠️  Pushing directly to hitched branch dev
This may be overwritten by: hitch rebuild dev

# Exit code: 0 (with warning)
```

---

## Integration Methods

### 1. Manual Git Hooks

**Create or edit `.git/hooks/pre-push`:**

```bash
#!/bin/bash

# Your existing checks
npm run lint || exit 1
npm test || exit 1

# Add Hitch check
hitch hook pre-push || exit 1

# More checks...
```

**Make it executable:**
```bash
chmod +x .git/hooks/pre-push
```

**Pros:**
- ✅ Simple
- ✅ No dependencies
- ✅ Full control

**Cons:**
- ❌ Not shared with team (hooks in `.git/` are not committed)
- ❌ Each developer must set up manually

---

### 2. Husky Integration

**If your team uses [Husky](https://typicode.github.io/husky/):**

**Install Husky** (if not already):
```bash
npm install --save-dev husky
npx husky init
```

**Create pre-push hook:**
```bash
npx husky add .husky/pre-push "hitch hook pre-push"
```

**Or edit `.husky/pre-push` manually:**
```bash
#!/usr/bin/env sh
. "$(dirname -- "$0")/_/husky.sh"

# Existing checks
npm run lint
npm test

# Add Hitch
hitch hook pre-push
```

**Pros:**
- ✅ Shared with team (committed to repo)
- ✅ Auto-installed on `npm install`
- ✅ Popular, well-maintained

---

### 3. Lefthook Integration

**If your team uses [Lefthook](https://github.com/evilmartians/lefthook):**

**Edit `lefthook.yml`:**
```yaml
pre-push:
  commands:
    lint:
      run: npm run lint
    test:
      run: npm test
    hitch:
      run: hitch hook pre-push
```

**Install:**
```bash
lefthook install
```

**Pros:**
- ✅ Shared with team
- ✅ YAML config (easy to read)
- ✅ Parallel execution support

---

### 4. pre-commit Framework

**If your team uses [pre-commit](https://pre-commit.com/):**

**Edit `.pre-commit-config.yaml`:**
```yaml
repos:
  - repo: local
    hooks:
      - id: hitch-lock-check
        name: Check Hitch lock
        entry: hitch hook pre-push
        language: system
        stages: [push]
        pass_filenames: false
```

**Install:**
```bash
pre-commit install --hook-type pre-push
```

**Pros:**
- ✅ Shared with team
- ✅ Language-agnostic
- ✅ Many pre-built hooks available

---

### 5. Custom Hook Manager

**For teams with custom hook solutions:**

Just call `hitch hook pre-push` from wherever your hooks run.

**Example - Shared hooks in repo:**
```
your-repo/
├── .githooks/
│   └── pre-push
└── .git/
    └── hooks/ → symlinks to .githooks/
```

**`.githooks/pre-push`:**
```bash
#!/bin/bash
hitch hook pre-push || exit 1
```

---

## Setup Recommendations

### For Teams

**Recommended: Use a shared hook manager**

Choose one that fits your team:
- **JavaScript/TypeScript teams**: Husky
- **Polyglot teams**: Lefthook or pre-commit
- **Custom needs**: Roll your own

**Add to README:**
```markdown
## Git Hooks

This project uses [Husky] for Git hooks.

Hooks are automatically installed when you run `npm install`.

The pre-push hook checks:
- Lint (ESLint)
- Tests (Jest)
- Hitch lock status
```

### For Individual Developers

**If hooks aren't set up team-wide:**

Create `.git/hooks/pre-push` locally:
```bash
#!/bin/bash
hitch hook pre-push || exit 1
```

---

## Bypassing Hooks

**If you need to bypass the hook** (use with caution):

```bash
git push --no-verify
```

**When this is acceptable:**
- You're the lock holder and want to force-push anyway
- Emergency situation
- You know what you're doing

**When this is NOT acceptable:**
- Branch is locked by someone else
- Just trying to "get around" the check

---

## Troubleshooting

### Hook doesn't run

**Check if hook is executable:**
```bash
ls -la .git/hooks/pre-push
# Should show: -rwxr-xr-x (executable)

chmod +x .git/hooks/pre-push
```

**Check if hitch is in PATH:**
```bash
which hitch
# Should show path to hitch binary

hitch version
# Should show version
```

### Hook runs but always passes

**Test manually:**
```bash
hitch hook pre-push
echo $?
# Should print 0 (pass) or 1 (fail)
```

**Check if on a hitched branch:**
```bash
git branch
# * dev  ← Should show hitched branch

hitch status
# Should show dev as an environment
```

### Hook blocks legitimate push

**Check lock status:**
```bash
hitch status

# If stale lock:
hitch unlock dev --force
```

---

## Advanced: Multiple Hook Commands

Hitch can provide additional hook commands in the future:

### `hitch hook pre-commit` (Future)

Could check for:
- Direct commits to hitched branches
- Large files being committed to hitch-metadata
- Invalid metadata format

### `hitch hook post-merge` (Future)

Could:
- Notify if main has diverged significantly
- Suggest rebuilding environments

### `hitch hook commit-msg` (Future)

Could:
- Validate commit message format
- Auto-add branch name to commits

**Example `.git/hooks/pre-commit`:**
```bash
#!/bin/bash

hitch hook pre-commit || exit 1
npm run lint-staged || exit 1
```

---

## Comparison: Hook vs No Hook

| Scenario | With `hitch hook pre-push` | Without |
|----------|----------------------------|---------|
| Dev A rebuilding, Dev B pushes | ❌ Dev B's push blocked with clear error | ⚠️ Dev B's push succeeds but will be overwritten |
| Lock is stale | ❌ Blocked with suggestion to force-unlock | ⚠️ No warning |
| Direct commit to dev | ⚠️ Warning but allows | ✅ Allowed (no check) |
| Lock held by you | ✅ Allowed (you're doing hitch op) | ✅ Allowed |
| Feature branch | ✅ Allowed (not managed) | ✅ Allowed |

**Recommendation:** Use hooks if your team wants protection. Skip if you prefer simplicity.

---

## Template: Complete Pre-Push Hook

**Copy-paste ready:**

```bash
#!/bin/bash
#
# Git pre-push hook
# Location: .git/hooks/pre-push

set -e

echo "Running pre-push checks..."

# Hitch lock check (if Hitch is installed)
if command -v hitch &> /dev/null; then
    hitch hook pre-push || exit 1
fi

# Add your own checks here:
# npm run lint || exit 1
# npm test || exit 1
# ./scripts/check-migrations.sh || exit 1

echo "✓ All pre-push checks passed"
```

**Install:**
```bash
# Copy to hook location
cp path/to/pre-push .git/hooks/pre-push

# Make executable
chmod +x .git/hooks/pre-push

# Test
git push origin feature/test
```

---

## Server-Side Protection (Optional)

For teams wanting **enforced** protection (cannot be bypassed with `--no-verify`):

### GitHub Branch Protection

**Settings → Branches → Add rule for `dev`:**
- ☑ Require status checks to pass
- ☑ Add status check: "hitch-lock-check"

**`.github/workflows/hitch-lock-check.yml`:**
```yaml
name: Hitch Lock Check

on:
  push:
    branches: [dev, qa]
  pull_request:
    branches: [dev, qa]

jobs:
  check-lock:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0

      - name: Fetch hitch-metadata
        run: git fetch origin hitch-metadata:hitch-metadata

      - name: Check if branch is locked
        run: |
          BRANCH=${GITHUB_REF#refs/heads/}

          # Read metadata
          git checkout hitch-metadata

          LOCKED=$(jq -r ".environments.${BRANCH}.locked // false" hitch.json)

          if [ "$LOCKED" = "true" ]; then
            LOCKED_BY=$(jq -r ".environments.${BRANCH}.locked_by" hitch.json)
            echo "❌ $BRANCH is locked by $LOCKED_BY"
            exit 1
          fi

          echo "✓ $BRANCH is not locked"
```

**Pros:**
- Cannot be bypassed
- Enforced for everyone

**Cons:**
- Requires GitHub Actions
- Slower feedback (after push)
- Only works if workflow is in pushed branch

### GitLab Protected Branches

**Settings → Repository → Protected Branches:**
- Select branch: `dev`
- Allowed to push: "No one"
- Allowed to merge: "Maintainers"

Then use Merge Requests for all changes (Hitch would need API access to push).

---

## FAQ

**Q: Will hooks slow down my workflow?**
A: `hitch hook pre-push` is very fast (<100ms). It just reads metadata.

**Q: What if hitch binary isn't installed on developer's machine?**
A: The hook should check first:
```bash
if command -v hitch &> /dev/null; then
    hitch hook pre-push || exit 1
else
    echo "⚠️  Hitch not installed, skipping lock check"
fi
```

**Q: Can I disable hooks temporarily?**
A: Yes, use `--no-verify`:
```bash
git push --no-verify
```

**Q: Should hooks be required for all team members?**
A: Recommended but not required. Hitch works fine without hooks - they're just an extra safety layer.

**Q: What happens if two people push at the exact same time?**
A: The second push will fail with "updates were rejected because the remote contains work that you do not have locally". This is normal Git behavior.
