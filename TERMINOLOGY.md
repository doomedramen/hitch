# Hitch Terminology Reference

**Purpose:** This document defines the canonical terms used throughout Hitch code, documentation, and user-facing messages. Use this as the single source of truth for consistent terminology.

**Last Updated:** 2025-10-17

---

## Core Concepts

### Hitched Branch

**Definition:** A git branch that is automatically rebuilt by Hitch from a base branch plus selected features. The branch is "hitched" (coupled/linked) to a specific combination of features - like a trailer hitched to a truck, the branch is connected to its feature list.

**Examples:** `dev`, `qa`, `staging`, `production`

**Key Properties:**
- Ephemeral (can be deleted and rebuilt)
- Force-pushed after every rebuild
- Should not be edited directly
- Represents a specific combination of features for testing/deployment

**When to use:**
- ✅ Technical documentation and architecture docs
- ✅ Code comments explaining the concept
- ✅ When emphasizing the "don't edit directly" aspect
- ✅ Error messages about direct commits/pushes

**Usage examples:**
```
✅ "dev is a hitched branch managed by Hitch"
✅ "Rebuilding hitched branch dev..."
✅ "Hitched branches: dev, qa"
✅ "Cannot push directly to hitched branch"
❌ "The dev integration branch" (use hitched branch OR environment)
❌ "The dev managed branch" (too generic)
```

**In code:**
```go
// Good: Clear what this is
errorMsg("Cannot push to hitched branch " + branchName)

// Good: On-brand terminology
info("Rebuilding hitched branch " + envName + "...")

// Avoid: Mixed terminology
warning("Pushing to managed environment branch " + name)
```

---

### Environment

**Definition:** A deployment stage (dev, qa, production) where features are tested. In Hitch, each environment is represented by a hitched branch of the same name.

**When to use:**
- ✅ User-facing commands and flags
- ✅ Command descriptions and help text
- ✅ CLI output and status messages
- ✅ When the focus is on deployment/testing context

**Usage examples:**
```
✅ "hitch promote <branch> to <environment>"
✅ "Lock an environment to prevent modifications"
✅ "Environment: dev (unlocked)"
✅ "Rebuilding dev environment..."
❌ "Promote to dev branch" (just say "to dev")
❌ "Lock the dev hitched branch" (just say "environment")
```

**In code:**
```go
// Good: User-facing, simple
cmd.Use = "promote <branch> to <environment>"

// Good: Matches user mental model
fmt.Printf("Environment: %s\n", envName)

// Good: Clear context
errorMsg("Environment is locked")
```

**Important:** "Environment" in user-facing text usually means the deployment context. When you need to be explicit about the git branch, say "hitched branch" or "environment branch."

---

### Environment Branch

**Definition:** The explicit combination of both concepts - the git branch that represents an environment. Use when clarity requires distinguishing from environment configuration or other meanings.

**When to use:**
- 🟡 Only when ambiguity exists
- 🟡 When explaining how environments map to branches
- 🟡 In technical explanations

**Usage examples:**
```
✅ "The dev environment branch is rebuilt nightly"
✅ "Environment branches (dev, qa) are force-pushed"
🟡 "Lock the dev environment" (prefer just "environment")
❌ "Environment branch dev is locked" (too wordy)
```

**Guideline:** Use "environment" in most cases. Only add "branch" when needed for clarity.

---

### Feature Branch

**Definition:** A git branch containing work on a specific feature, bug fix, or change. Created by developers and promoted to environments for testing.

**Naming convention:** `feature/*`, `bugfix/*`, `fix/*`, or any developer-chosen name

**Key Properties:**
- Created and edited by developers
- Originates from base branch (main)
- Never modified by Hitch
- Can be promoted to multiple environments

**When to use:**
- ✅ Everywhere - this is the standard term

**Usage examples:**
```
✅ "Create a feature branch"
✅ "Promote feature/user-auth to dev"
✅ "Feature branches: feature/login, bugfix/validation"
❌ "Topic branch" (not our term)
❌ "User branch" (confusing)
```

---

### Base Branch

**Definition:** The branch that serves as the starting point for feature branches and hitched branches. Usually `main` or `master`.

**When to use:**
- ✅ Configuration and setup
- ✅ Technical documentation
- ✅ When being generic (not assuming "main")

**Usage examples:**
```
✅ "Base branch: main"
✅ "Feature branches start from the base branch"
✅ "--base <branch> - Base branch name (default: main)"
❌ "Trunk" (not our term)
❌ "Source branch" (ambiguous)
```

**Note:** When specifically referring to `main`, just say "main" unless you need to be generic.

---

### Metadata Branch

**Definition:** The special orphan branch `hitch-metadata` that stores Hitch's state (environments, features, locks, etc.) in `hitch.json`.

**Branch name:** Always `hitch-metadata`

**When to use:**
- ✅ Technical documentation
- ✅ Architecture explanations
- ✅ Error messages about metadata

**Usage examples:**
```
✅ "The hitch-metadata branch"
✅ "Metadata stored in hitch-metadata"
✅ "Failed to read metadata branch"
❌ "The metadata" (too vague)
❌ "State branch" (not our term)
```

---

## Operations

### Promote / Promotion

**Definition:** Add a feature branch to an environment's feature list and optionally rebuild.

**Usage examples:**
```
✅ "Promote feature/xyz to dev"
✅ "Promoting feature/abc to qa..."
✅ "Promotion complete"
❌ "Deploy to dev" (not our term - promotion isn't deployment)
❌ "Add feature to environment" (too verbose, use "promote")
```

---

### Demote / Demotion

**Definition:** Remove a feature branch from an environment's feature list and optionally rebuild.

**Usage examples:**
```
✅ "Demote feature/xyz from dev"
✅ "Demoting feature/abc..."
❌ "Remove from dev" (use "demote")
❌ "Unpromote" (not a word)
```

---

### Rebuild / Recompile

**Definition:** Reconstruct a hitched branch from scratch using its base branch plus all promoted features.

**When to use:**
- "Rebuild" for most user-facing text
- "Recompile" when emphasizing the compilation metaphor
- Both are acceptable

**Usage examples:**
```
✅ "Rebuilding dev environment..."
✅ "Recompiling qa branch..."
✅ "hitch rebuild dev"
✅ "Automatic rebuild on promote"
❌ "Regenerate" (not our term)
❌ "Recreate" (too vague)
```

---

### Lock / Unlock

**Definition:** Temporarily prevent modifications to an environment to coordinate operations or reserve for testing.

**Usage examples:**
```
✅ "Lock the dev environment"
✅ "Environment is locked by user@example.com"
✅ "Unlock dev to allow changes"
❌ "Reserve environment" (not our term)
❌ "Environment is busy" (informal)
```

---

## Lifecycle States

### Stale Branch

**Definition:** A feature branch that has been merged to the base branch and has passed the retention period, making it safe to delete.

**Usage examples:**
```
✅ "Stale branches (merged 30+ days ago)"
✅ "feature/old-feature is stale"
❌ "Old branch" (too vague)
❌ "Expired branch" (not our term)
```

---

### Inactive Branch

**Definition:** A feature branch with no recent commits that might be abandoned. Unlike stale branches, these are NOT automatically safe to delete.

**Usage examples:**
```
✅ "Inactive branches (no commits in 60+ days)"
✅ "feature/abandoned may be inactive"
❌ "Dead branch" (informal)
❌ "Unused branch" (ambiguous)
```

---

## Safety Mechanisms

### Temp Branch

**Definition:** A temporary branch (pattern: `{env}-hitch-temp`) used during rebuilds to test merges before replacing the actual hitched branch.

**Usage examples:**
```
✅ "Creating temp branch: dev-hitch-temp"
✅ "Merging features into temp branch"
✅ "Swapping temp branch → dev"
❌ "Test branch" (ambiguous)
❌ "Staging branch" (confusing with 'staging' environment)
```

---

### Dry Run

**Definition:** Simulate an operation without making actual changes.

**Usage examples:**
```
✅ "hitch rebuild dev --dry-run"
✅ "Dry run: no changes made"
❌ "Simulation" (too formal)
❌ "Preview" (not our term)
```

---

## Style Guidelines

### Hyphenation Rules

**Branch names and Git hooks (always hyphenated):**
- ✅ `hitch-metadata`
- ✅ `pre-push`
- ✅ `dev-hitch-temp`

**Descriptive terms (usually not hyphenated):**
- ✅ "feature branch"
- ✅ "base branch"
- ✅ "compiled branch"
- ✅ "dry run"
- ❌ "feature-branch" (unless compound adjective)

**Compound adjectives (hyphenated before noun):**
- ✅ "hitch-managed branches"
- ✅ "force-with-lease push"
- ✅ "multi-environment workflow"

---

## Context-Specific Usage

### In CLI Commands
**Priority:** Simplicity and user mental model
- Use: "environment" (not "hitched branch")
- Use: "promote/demote" (active verbs)
- Use: "lock/unlock" (simple)

**Example:**
```bash
hitch promote feature/xyz to dev
hitch lock qa --reason "Testing release"
```

### In Status Output
**Priority:** Clear, scannable information
- Use: "Environment:" (matches user thinking)
- Use: "Features:" (simple list)
- Use: "locked/unlocked" (clear state)

**Example:**
```
Environment: dev (locked by user@example.com)
  Base: main
  Features:
    - feature/xyz (promoted 2 hours ago)
```

### In Error Messages
**Priority:** Actionable, clear, blame-free
- Use: "environment" for user actions
- Use: "hitched branch" when explaining constraints
- Be specific about what's wrong and how to fix

**Example:**
```
❌ Cannot push to dev

dev is a hitched branch managed by Hitch.
Direct commits will be lost on rebuild.

To add your changes:
  1. Create a feature branch: git checkout -b feature/my-work
  2. Commit your changes
  3. Promote to dev: hitch promote feature/my-work to dev
```

### In Documentation
**Priority:** Teaching and understanding
- Use: "hitched branch" in architecture docs
- Use: "environment" in user guides
- Use: "integration branch" when comparing to other workflows
- Define terms on first use

**Example:**
```markdown
## How It Works

Hitch treats dev and qa as **hitched branches** - ephemeral
branches that are rebuilt from a base branch (main) plus selected
features. The branches are "hitched" to a specific feature list
and automatically rebuilt when that list changes.

Each hitched branch represents an **environment** (a deployment
stage) where features are tested.
```

### In Code Comments
**Priority:** Precision for maintainers
- Use: "hitched branch" (clear what it is)
- Use: "metadata branch" (specific)
- Be explicit and technical

**Example:**
```go
// Check if current branch is a hitched branch (dev, qa, etc.)
// Hitched branches are managed by Hitch and should not be pushed directly
_, isHitched := meta.Environments[currentBranch]
if isHitched {
    errorMsg("Cannot push to hitched branch " + currentBranch)
}
```

---

## Quick Reference Table

| Concept | Primary Term | Aliases/Context | Avoid |
|---------|-------------|-----------------|-------|
| dev/qa branches | "hitched branch" (technical)<br>"environment" (UX) | "integration branch" (comparisons only) | "managed branch", "compiled branch", "special branch" |
| User's work | "feature branch" | - | "topic branch", "user branch" |
| main/master | "base branch" | "main" (when specific) | "trunk", "source" |
| hitch-metadata | "metadata branch" | - | "state branch", "config branch" |
| Add to env | "promote" | - | "add", "deploy" |
| Remove from env | "demote" | - | "remove", "unpromote" |
| Reconstruct | "rebuild" or "recompile" | - | "regenerate", "recreate" |
| Prevent changes | "lock" | - | "reserve", "claim" |
| Old + merged | "stale branch" | - | "old branch", "expired" |
| No recent work | "inactive branch" | - | "dead branch", "abandoned" |

---

## Enforcement

When writing or reviewing:

1. **Commands/CLI:** Use "environment" terminology
2. **Technical docs:** Use "hitched branch"
3. **Code comments:** Be explicit - "hitched branch", "metadata branch"
4. **Error messages:** Use "hitched branch" when explaining constraints
5. **Status output:** Use "Environment:" heading

**When in doubt:** Check this document or ask!

---

## Updates

This is a living document. When we discover new terms or ambiguities:

1. Update this document
2. Note the change in git commit
3. Update code/docs to match

**Changes log:**
- 2025-10-17: Initial version, adopted "hitched branch" terminology (on-brand with tool name)
