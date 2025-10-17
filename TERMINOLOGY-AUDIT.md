# Hitch Terminology Audit

This document lists all terms we currently use throughout the codebase and documentation, grouped by concept.

## 1. The dev/qa Branches (MOST IMPORTANT)

**Current variations found:**
- "managed branch" (most common in code/hooks)
- "environment branch" (used in SAFETY.md)
- "integration branch" (used in README, ARCHITECTURE)
- "ephemeral integration branch" (README, ARCHITECTURE)
- "computed integration branch" (README)
- "deployment environment" (README)
- "special branch" (early discussion)

**Context examples:**
- "managed branch dev" (hook.go line 123)
- "environment branches (dev, qa)" (SAFETY.md line 130)
- "integration branches as ephemeral" (README line 5)
- "Ephemeral Integration Branches" (ARCHITECTURE.md heading)
- "rebuilding environment branches" (SAFETY.md line 750)

**Issues:**
- We use "environment" to mean BOTH the branch (dev) AND the config object
- "managed branch" is clear but not descriptive of purpose
- "integration branch" is industry standard but we add "ephemeral" modifier
- "environment branch" conflates deployment environment with git branch

---

## 2. The Concept of "Environment"

**Current usage:**
- Environment = deployment stage (dev, qa, production) ✅ Primary meaning
- Environment = the branch itself (conflated usage)
- Environment = metadata config object (Environments map[string]Environment)

**In code:**
- `meta.Environments[envName]` - config object
- "rebuild dev environment" - could mean branch OR deployment stage

**In docs:**
- "Show current state of all environments" (status command)
- "Lock an environment to prevent modifications" (lock command)

---

## 3. Feature Branches

**Current terms:**
- "feature branch" ✅ (consistent, good)
- "feature-branch" (hyphenated in some places)
- "topic branch" (not used, but could be alternative)

**Context:**
- "feature branches" (most documentation)
- "feature/xyz" (example naming)
- Features list in Environment struct

**Status:** Pretty consistent, minimal issues

---

## 4. The hitch-metadata Branch

**Current terms:**
- "metadata branch" ✅ (consistent)
- "hitch-metadata" (the actual branch name)
- "hitch-metadata branch" (full reference)

**Status:** Consistent and clear

---

## 5. The Base Branch

**Current terms:**
- "base branch" ✅ (most common)
- "main branch" (when specifically referring to 'main')
- "trunk" (not used in docs)
- "baseline" (not used)

**Context:**
- `--base <branch>` flag
- "base branch name (main, master, etc.)"
- config.base_branch

**Status:** Consistent, "base branch" is good generic term

---

## 6. Operations

### Promote
**Terms:**
- "promote" ✅ (verb)
- "promotion" (noun)
- "promoted to" (past tense)
- "promoting features to environments"

**Status:** Consistent

### Demote
**Terms:**
- "demote" ✅ (verb)
- "demotion" (noun)

**Status:** Consistent

### Rebuild
**Terms:**
- "rebuild" ✅ (most common)
- "reconstruction" (noun form)
- "reconstructed" (past tense)
- "recompute" (not used, but could be alternative)

**Status:** Consistent with "rebuild"

---

## 7. Locking

**Terms:**
- "lock" ✅ (verb)
- "unlock" (verb)
- "locked" (state)
- "lock holder" (person who locked)
- "stale lock" (lock past timeout)

**Status:** Consistent and clear

---

## 8. Branch Lifecycle States

**Current terms:**
- "stale branch" (merged + past retention)
- "inactive branch" (no recent commits)
- "safe to delete" (ready for cleanup)
- "merged to main" (state)
- "eligible for cleanup"

**Status:** Multiple overlapping terms, could be simplified

---

## 9. Safety Mechanisms

**Current terms:**
- "temp branch" (dev-hitch-temp pattern)
- "dry run" (simulation mode)
- "force-with-lease" (Git operation)
- "clean state" (no uncommitted changes)
- "uncommitted changes" / "unstaged changes"

**Status:** Mostly consistent

---

## Key Issues to Resolve

### 🔴 CRITICAL: What do we call dev/qa branches?

**Option A: "Environment Branch"**
- ✅ Descriptive of purpose (maps to deployment environment)
- ❌ Conflates "environment" (deployment stage) with "branch" (Git concept)
- ❌ "environment" already heavily used for config object

**Option B: "Integration Branch"**
- ✅ Industry standard term
- ✅ Describes purpose (integrating features)
- ⚠️ Need to emphasize "ephemeral" nature
- ❌ Traditional integration branches aren't force-pushed/rebuilt

**Option C: "Managed Branch"**
- ✅ Clear that Hitch manages these
- ✅ Distinguishes from user's feature branches
- ❌ Doesn't convey purpose (why are they managed?)
- ❌ Generic, not descriptive

**Option D: "Staging Branch"**
- ✅ Matches "staging environment" terminology
- ✅ Implies temporary/non-production
- ❌ "staging" usually means pre-production, not dev/qa
- ❌ Might confuse with Git staging area

**Option E: "Target Branch"**
- ✅ You "promote to" targets
- ✅ Neutral, not overloaded
- ❌ Less descriptive
- ❌ Could be confused with PR targets

**Option F: Keep current mixed usage**
- Use "integration branch" in user-facing docs (README, guides)
- Use "environment" internally (code, metadata)
- Use "managed branch" in technical docs (safety, hooks)
- ❌ Inconsistent, confusing

---

### 🟡 MEDIUM: "Environment" is overloaded

**Current uses:**
1. Deployment stage concept (dev, qa, production)
2. The git branch itself (dev branch)
3. The metadata config object (Environment struct)

**Impact:**
- Code: `meta.Environments[envName]` is clear in context
- Docs: "rebuild dev environment" - which meaning?
- Commands: "Lock an environment" - means the config, not the deployment

**Possible solutions:**
- Keep "environment" for deployment stage + config
- Use specific term for the branch (see Critical issue above)
- Be explicit: "dev environment branch" vs "dev environment config"

---

### 🟢 MINOR: Inconsistent hyphenation

**Examples:**
- "feature branch" vs "feature-branch"
- "pre-push" vs "pre push"
- "hitch-metadata" vs "hitch metadata"

**Solution:** Standardize on no hyphens except for:
- Actual names: `hitch-metadata` (branch name)
- Git hooks: `pre-push` (standard Git naming)
- Compound adjectives: "hitch-managed branches" (grammatically correct)

---

## Recommendations for Discussion

1. **Choose ONE canonical term for dev/qa branches**
   - Affects: All docs, code comments, error messages
   - Most impactful decision

2. **Clarify "environment" usage**
   - Document when we mean branch vs deployment stage vs config

3. **Standardize hyphenation**
   - Run through docs with consistent style

4. **Consider aliases/synonyms**
   - Should we support both `hitch promote` and `hitch add`?
   - Are there user-friendly alternatives to technical terms?

---

## Usage Statistics

Based on grep across *.md files:

| Term | Approximate Count |
|------|-------------------|
| "environment" | 200+ occurrences |
| "integration branch" | ~20 occurrences |
| "managed branch" | ~15 occurrences |
| "environment branch" | ~10 occurrences |
| "feature branch" | 50+ occurrences |
| "metadata branch" | ~25 occurrences |
| "rebuild" | 100+ occurrences |
| "promote" | 80+ occurrences |

---

## Code Impact Analysis

### If we choose "integration branch":
- Update hook.go: "managed branch" → "integration branch"
- Update status.go comments
- Keep Environment struct name (fine, it's config)
- All docs: standardize on "integration branch"

### If we choose "environment branch":
- Minimal code changes (already used in some places)
- Need to clarify in docs: "environment branch" vs "environment config"
- More descriptive but more wordy

### If we choose "managed branch":
- Minimal doc changes (already common in code)
- Less descriptive but clearer scope
- Need to explain "managed" in intro docs

---

## Questions for Decision

1. **Primary question:** What should we call dev/qa branches consistently?

2. **Secondary question:** Should we have different terms for:
   - User-facing docs (less technical)
   - Code/comments (more precise)
   - Error messages (most clear)

3. **Philosophy question:** Is it more important that the term is:
   - Industry standard (integration branch)
   - Self-explanatory (environment branch)
   - Scoped to Hitch (managed branch)

4. **Migration question:** If we change terms, do we:
   - Update all at once (big commit)
   - Update gradually (by file)
   - Leave old terms in code but fix docs

---

## Related Concepts from Other Tools

For reference, how similar tools name things:

**Git Flow:**
- develop (integration branch)
- feature/* (feature branches)
- release/* (release branches)

**GitHub Flow:**
- main (base branch)
- feature branches (any branch)
- (no integration branches)

**Trunk-Based Development:**
- trunk/main (base)
- short-lived feature branches
- (no persistent integration branches)

**Our model is unique:** Ephemeral integration branches that are reconstructed. This uniqueness might warrant a Hitch-specific term.
