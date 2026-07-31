# Hitch Production-Grade Internals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden hitch's internals to production grade — close the untrusted-input holes, make publishing atomic, and make crash recovery general and provable — without changing a single CLI command, flag, or output contract.

**Architecture:** Three independently shippable phases. Phase 1 closes the trust boundary: hitch.json lives on a branch anyone with push access can write, and its contents flow into `git` argv under a deploy key that bypasses branch protection — so config becomes validated, untrusted input and git subprocesses get a hardening tier. Phase 2 collapses the four separate ref writes of a publish (backup ref, pending-resync ref, environment CAS, later metadata) into one `git update-ref --stdin` transaction, which is all-or-nothing, and adds a `refs/hitch/prev/*` archive so rollback is a one-ref flip. Phase 3 generalizes `pending_resync` into a publish journal that also covers the push step, and adds a crash-fuzz harness that kills the process after every step and asserts recovery converges.

**Tech Stack:** Rust 2021, clap 4.5 derive, anyhow, serde/serde_json, system `git` binary (2.50 local floor, 2.30 supported floor), `just` recipes, integration harness in `tests/test_framework/`.

## Global Constraints

- **The CLI surface must not change.** No new commands, no new flags, no renamed flags, no changed exit codes. Existing user-facing message text may gain new messages but must not lose the "here is the exact command to run next" ending.
- **Every change must keep `just format`, `just format-check && just lint`, `just test` green.** Clippy runs `-D warnings`; a warning is a hard failure.
- **No new git2 usage.** `git2` stays limited to repo discovery.
- **All git/gh subprocesses get `Stdio::null()` stdin**, except `hitch resolve --tool` (`git mergetool`) and the deliberately piped-stdin plumbing calls (`hash-object`, `mktree`, `update-ref --stdin`), which pipe stdin and close it explicitly.
- **Minimum supported git: 2.30.** `git update-ref --stdin -z` batches are atomic against a compare-and-swap failure at every version at or above that floor — but that alone doesn't cover every crash: EOF on stdin is an implicit commit of whatever complete records already arrived, so a process death mid-write between two record boundaries can apply a genuinely partial batch (verified empirically during Phase 2's final review). `ref_transaction` (`src/utils/git_operations.rs`) therefore DOES use the explicit `start`/`commit` transaction verbs — available since git 2.27, comfortably under this floor — framing every batch so a truncated write applies nothing rather than a prefix. Do not remove them.
- **Compare checkout paths with `GitOperations::same_checkout_path`, never `==`.**
- **Do not add `#[serde(deny_unknown_fields)]` to `HitchConfig`.** `HitchConfig::check_write_compatibility` (src/types.rs:433) exists precisely because unknown fields are dropped on read; denying them would make a newer config unreadable instead of merely unwritable, which is a regression.
- **Commit messages must not include a `Co-Authored-By` trailer.**
- Update `AGENTS.md` in the same task that invalidates something it documents.

---

## File Structure

**Phase 1 — trust boundary**
- Modify `src/utils/validation.rs` — `validate_name` gains the refname rules it is missing (control characters, whitespace, `.lock` suffix, dot-leading components, bare `@`); becomes the single refname firewall.
- Create `src/utils/config_validation.rs` — `parse_untrusted_config`: size cap, JSON parse, count caps, refname firewall over every environment name / base / promoted branch. One auditable place where bytes off `hitch-metadata` become a `HitchConfig`.
- Modify `src/utils/prelude.rs:302` and `:407` — both `serde_json::from_str::<HitchConfig>` sites route through `parse_untrusted_config`.
- Modify `src/utils/git_operations.rs` — hardening args on the shared subprocess builder; a `Plumbing` tier for pure object-database calls; fold the two raw `Command::new("git")` push sites and the `hash-object`/`mktree` sites onto one annotated helper.
- Modify `src/utils/gh.rs:234` — `owner_repo_from_remote` runs git with no `current_dir` and no `LC_ALL=C`; fix both.
- Create `clippy.toml` — `disallowed-methods` on `std::process::Command::new` so new bypasses of the choke point must be explicitly annotated.
- Modify `src/utils/resolutions.rs` + `src/types.rs` + `src/utils/prelude.rs` — optional SSH-signature attestation on recorded resolutions, enforced only when `hitch.json` opts in.
- Create `tests/integration/trust_boundary_tests.rs` — red-team suite: the attacks are executed against the built binary and asserted inert.

**Phase 2 — atomic publish**
- Modify `src/utils/git_operations.rs` — `RefEdit` enum + `ref_transaction`.
- Modify `src/utils/prelude.rs:1036` `publish_environment_build` — one transaction replaces backup-ref write + pending-resync write + `update_ref_cas`, and adds `refs/hitch/prev/<env>/<ts>`.
- Modify `src/commands/doctor.rs` — surface `refs/hitch/prev/*` as the rollback path; prune old ones under `--fix`.

**Phase 3 — publish journal**
- Rename `src/utils/pending_resync.rs` → `src/utils/publish_journal.rs`, widened to cover the push step, with backward-compatible reading of old `refs/hitch/pending-resync/*` records.
- Create `tests/integration/crash_recovery_tests.rs` — kill-after-step-N harness.

---

## Phase 1 — Trust boundary

### Task 1: Refname firewall

`validate_name` (src/utils/validation.rs:12) already rejects most option-shaped and git-illegal names, but it is only ever called from CLI argument paths — never at the `hitch.json` boundary — and it misses control characters, whitespace, `.lock` suffixes, dot-leading path components and a bare `@`. This task closes the rule gaps. Task 2 applies it at the boundary.

**Files:**
- Modify: `src/utils/validation.rs:12-66`
- Test: `tests/unit/validation_tests.rs` (create)
- Modify: `tests/unit/mod.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `hitch::utils::validation::validate_name(name: &str, name_type: &str) -> anyhow::Result<()>` — unchanged signature, stricter behaviour.

- [ ] **Step 1: Write the failing test**

Create `tests/unit/validation_tests.rs`:

```rust
//! Unit tests for the refname firewall in `utils::validation`.

use hitch::utils::validation::validate_name;

#[cfg(test)]
mod tests {
    use super::*;

    /// Names that must be accepted — ordinary branch names people really use.
    #[test]
    fn test_validate_name_accepts_ordinary_branches() {
        for name in [
            "main",
            "dev",
            "feature/login",
            "release-1.2.3",
            "user/martin/fix_thing",
            "v2",
        ] {
            assert!(
                validate_name(name, "Branch").is_ok(),
                "expected '{}' to be accepted",
                name
            );
        }
    }

    /// Names that must be rejected. Each is a real git-illegal or
    /// argv-hostile form; a rejection here is what stops hitch.json from
    /// steering a git command line.
    #[test]
    fn test_validate_name_rejects_hostile_names() {
        for name in [
            "",                       // empty
            "-fdx",                   // option-shaped
            "--upload-pack=touch /tmp/pwn", // option injection
            "feature/a..b",           // double dot
            "feature/a b",            // space
            "feature/a\tb",           // tab
            "feature/a\nb",           // newline
            "feature/a\u{7f}b",       // DEL
            "feature/a\u{1b}[31m",    // ANSI escape
            "feature/x.lock",         // .lock suffix
            "feature/.hidden",        // dot-leading component
            "@",                      // bare @
            "feature/a@{0}",          // @{ sequence
            "feature/a:b",            // colon
            "/leading",               // leading slash
            "trailing/",              // trailing slash
            "double//slash",          // consecutive slashes
        ] {
            assert!(
                validate_name(name, "Branch").is_err(),
                "expected '{}' to be rejected",
                name.escape_debug()
            );
        }
    }
}
```

Register it in `tests/unit/mod.rs` by adding the line in alphabetical position:

```rust
pub mod utils_tests;
pub mod validation_tests;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch validate_name_rejects_hostile -- --nocapture`
Expected: FAIL — `expected 'feature/a b' to be rejected` (space, tab, newline, DEL, ANSI, `.lock`, dot-leading and bare `@` all pass today).

- [ ] **Step 3: Write minimal implementation**

In `src/utils/validation.rs`, replace the body of `validate_name` (lines 12-66) with:

```rust
pub fn validate_name(name: &str, name_type: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("{} name cannot be empty", name_type));
    }

    if name.len() > 100 {
        return Err(anyhow::anyhow!(
            "{} name cannot exceed 100 characters",
            name_type
        ));
    }

    // Cannot start with '-': git forbids this for ref names, and more importantly
    // such a value could be mis-parsed as a command-line option when passed to git.
    if name.starts_with('-') {
        return Err(anyhow::anyhow!(
            "{} name cannot start with '-': '{}'",
            name_type,
            name
        ));
    }

    // Control characters, DEL and any whitespace break stderr/stdout parsing and
    // can smuggle terminal escapes into hitch's own output.
    if name
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\u{7f}')
    {
        return Err(anyhow::anyhow!(
            "{} name cannot contain whitespace or control characters: '{}'",
            name_type,
            name.escape_debug()
        ));
    }

    // Check for invalid characters that would cause issues in git
    let invalid_chars = ["..", "@{", ":", "[", "]", "\\", "^", "~", "?", "*"];
    for invalid in &invalid_chars {
        if name.contains(invalid) {
            return Err(anyhow::anyhow!(
                "{} name cannot contain '{}': '{}'",
                name_type,
                invalid,
                name
            ));
        }
    }

    if name == "@" {
        return Err(anyhow::anyhow!(
            "{} name cannot be '@' (reserved by git)",
            name_type
        ));
    }

    // Cannot start or end with slash
    if name.starts_with('/') || name.ends_with('/') {
        return Err(anyhow::anyhow!(
            "{} name cannot start or end with '/': '{}'",
            name_type,
            name
        ));
    }

    // Cannot have consecutive slashes
    if name.contains("//") {
        return Err(anyhow::anyhow!(
            "{} name cannot contain consecutive slashes: '{}'",
            name_type,
            name
        ));
    }

    // Per-component rules git enforces in check-ref-format: no component may
    // begin with '.' or end with '.lock', and none may be empty.
    for component in name.split('/') {
        if component.starts_with('.') {
            return Err(anyhow::anyhow!(
                "{} name cannot have a path component starting with '.': '{}'",
                name_type,
                name
            ));
        }
        if component.ends_with(".lock") {
            return Err(anyhow::anyhow!(
                "{} name cannot have a path component ending with '.lock': '{}'",
                name_type,
                name
            ));
        }
    }

    if name.ends_with('.') {
        return Err(anyhow::anyhow!(
            "{} name cannot end with '.': '{}'",
            name_type,
            name
        ));
    }

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hitch validate_name`
Expected: PASS, both tests.

Run: `just test`
Expected: PASS. If an existing test asserted a now-rejected name was accepted, that test encoded a hole — fix the test, not the rule, and say so in the commit body.

- [ ] **Step 5: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/utils/validation.rs tests/unit/validation_tests.rs tests/unit/mod.rs
git commit -m "fix: reject control characters, .lock and dot components in ref names"
```

---

### Task 2: hitch.json is untrusted input

Everything in `hitch.json` reaches a `git` argv, and `hitch-metadata` is a branch any collaborator can push. Today both parse sites are a bare `serde_json::from_str` with no size bound and no refname check. This task creates one auditable parse function and routes both sites through it.

**Files:**
- Create: `src/utils/config_validation.rs`
- Modify: `src/utils/mod.rs` (add `pub mod config_validation;` after `pub mod confirm;`)
- Modify: `src/utils/prelude.rs:300-310` and `src/utils/prelude.rs:406-415`
- Test: `tests/unit/config_validation_tests.rs` (create), `tests/unit/mod.rs`

**Interfaces:**
- Consumes: `hitch::utils::validation::validate_name` from Task 1.
- Produces:
  - `hitch::utils::config_validation::parse_untrusted_config(raw: &str) -> anyhow::Result<hitch::types::HitchConfig>`
  - `pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;`
  - `pub const MAX_ENVIRONMENTS: usize = 256;`
  - `pub const MAX_BRANCHES_PER_ENV: usize = 1024;`

- [ ] **Step 1: Write the failing test**

Create `tests/unit/config_validation_tests.rs`:

```rust
//! Unit tests for untrusted hitch.json parsing.

use hitch::utils::config_validation::{
    parse_untrusted_config, MAX_BRANCHES_PER_ENV, MAX_CONFIG_BYTES, MAX_ENVIRONMENTS,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_branch(branch: &str) -> String {
        format!(
            r#"{{"version":"1.0","environments":{{"dev":{{"base":"main","branches":["{}"]}}}}}}"#,
            branch
        )
    }

    #[test]
    fn test_parse_untrusted_config_accepts_ordinary_config() {
        let config = parse_untrusted_config(&config_with_branch("feature/login"))
            .expect("ordinary config must parse");
        assert_eq!(config.environments.len(), 1);
        assert_eq!(config.environments["dev"].branches, vec!["feature/login"]);
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_branch() {
        let err = parse_untrusted_config(&config_with_branch("--upload-pack=touch /tmp/pwn"))
            .expect_err("option-shaped branch must be rejected");
        assert!(
            err.to_string().contains("hitch.json"),
            "error must name hitch.json so the user knows where to look: {}",
            err
        );
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_base() {
        let raw = r#"{"version":"1.0","environments":{"dev":{"base":"-fdx","branches":[]}}}"#;
        assert!(parse_untrusted_config(raw).is_err());
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_env_name() {
        let raw = r#"{"version":"1.0","environments":{"--exec=x":{"base":"main","branches":[]}}}"#;
        assert!(parse_untrusted_config(raw).is_err());
    }

    #[test]
    fn test_parse_untrusted_config_rejects_oversized_input() {
        let raw = format!(
            r#"{{"version":"1.0","environments":{{}},"_pad":"{}"}}"#,
            "a".repeat(MAX_CONFIG_BYTES)
        );
        let err = parse_untrusted_config(&raw).expect_err("oversized config must be rejected");
        assert!(err.to_string().contains("too large"), "got: {}", err);
    }

    #[test]
    fn test_parse_untrusted_config_rejects_too_many_environments() {
        let envs: Vec<String> = (0..=MAX_ENVIRONMENTS)
            .map(|i| format!(r#""env{}":{{"base":"main","branches":[]}}"#, i))
            .collect();
        let raw = format!(
            r#"{{"version":"1.0","environments":{{{}}}}}"#,
            envs.join(",")
        );
        assert!(parse_untrusted_config(&raw).is_err());
    }

    #[test]
    fn test_parse_untrusted_config_rejects_too_many_branches() {
        let branches: Vec<String> = (0..=MAX_BRANCHES_PER_ENV)
            .map(|i| format!(r#""feature/b{}""#, i))
            .collect();
        let raw = format!(
            r#"{{"version":"1.0","environments":{{"dev":{{"base":"main","branches":[{}]}}}}}}"#,
            branches.join(",")
        );
        assert!(parse_untrusted_config(&raw).is_err());
    }
}
```

Register in `tests/unit/mod.rs`:

```rust
pub mod approval_system_tests;
pub mod config_validation_tests;
pub mod conflict_report_tests;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch parse_untrusted_config`
Expected: FAIL to compile — `unresolved import hitch::utils::config_validation`.

- [ ] **Step 3: Write minimal implementation**

Create `src/utils/config_validation.rs`:

```rust
//! The one place where bytes read off the `hitch-metadata` branch become a
//! `HitchConfig`.
//!
//! `hitch-metadata` is a normal branch: anyone who can push to the repository
//! can rewrite `hitch.json`, and every environment name, base branch and
//! promoted branch in it ends up in a `git` argv on every machine that runs a
//! rebuild. So this is a trust boundary, not a convenience parser — bound the
//! input before parsing it, and prove every name is ref-shaped before it can
//! steer a command line.
//!
//! Deliberately NOT `#[serde(deny_unknown_fields)]`: `HitchConfig`'s forward
//! compatibility story (`check_write_compatibility`) depends on a newer config
//! still being *readable* by an older binary — denying unknown fields would
//! turn "cannot safely rewrite" into "cannot read at all".

use crate::types::HitchConfig;
use crate::utils::validation::validate_name;
use anyhow::{Context, Result};

/// Refuse to parse a config larger than this. A rebuild reads it on every
/// invocation; there is no legitimate megabyte-scale hitch.json.
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// Upper bounds on the shape of a config. These are not business limits — they
/// are the point past which a crafted config is a denial-of-service rather than
/// a configuration.
pub const MAX_ENVIRONMENTS: usize = 256;
pub const MAX_BRANCHES_PER_ENV: usize = 1024;

/// Parse `raw` as a `hitch.json` that may have been written by anyone.
///
/// Bounds size first, then parses, then proves every name is ref-shaped, then
/// runs the ordinary semantic validation. Every error names `hitch.json` and
/// ends with the command to run next.
pub fn parse_untrusted_config(raw: &str) -> Result<HitchConfig> {
    if raw.len() > MAX_CONFIG_BYTES {
        return Err(anyhow::anyhow!(
            "hitch.json is too large ({} bytes, limit {}). This is not a configuration \
             hitch wrote. Inspect it with:\n  git show hitch-metadata:hitch.json | head",
            raw.len(),
            MAX_CONFIG_BYTES
        ));
    }

    let config: HitchConfig = serde_json::from_str(raw).context(
        "Failed to parse hitch.json from the hitch-metadata branch. Inspect it with:\n  \
         git show hitch-metadata:hitch.json",
    )?;

    if config.environments.len() > MAX_ENVIRONMENTS {
        return Err(anyhow::anyhow!(
            "hitch.json declares {} environments (limit {}). Inspect it with:\n  \
             git show hitch-metadata:hitch.json",
            config.environments.len(),
            MAX_ENVIRONMENTS
        ));
    }

    for (env_name, env) in &config.environments {
        validate_name(env_name, "Environment").context(
            "hitch.json contains an unusable environment name. Fix it on the \
             hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
        )?;

        validate_name(&env.base, "Base branch").with_context(|| {
            format!(
                "hitch.json gives environment '{}' an unusable base branch. Fix it on the \
                 hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
                env_name
            )
        })?;

        if env.branches.len() > MAX_BRANCHES_PER_ENV {
            return Err(anyhow::anyhow!(
                "hitch.json gives environment '{}' {} promoted branches (limit {}). \
                 Inspect it with:\n  git show hitch-metadata:hitch.json",
                env_name,
                env.branches.len(),
                MAX_BRANCHES_PER_ENV
            ));
        }

        for branch in &env.branches {
            validate_name(branch, "Branch").with_context(|| {
                format!(
                    "hitch.json gives environment '{}' an unusable promoted branch. Fix it on \
                     the hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
                    env_name
                )
            })?;
        }
    }

    if let Err(validation_error) = config.validate() {
        return Err(anyhow::anyhow!(
            "Configuration validation failed: {}",
            validation_error
        ));
    }

    Ok(config)
}
```

Add to `src/utils/mod.rs`, keeping alphabetical order:

```rust
pub mod command_helpers;
pub mod config_validation;
pub mod confirm;
```

- [ ] **Step 4: Route both parse sites through it**

In `src/utils/prelude.rs`, replace lines 300-310 (inside `access_metadata_read_only`):

```rust
    // Parse configuration
    let config: HitchConfig =
        serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

    // Validate configuration
    if let Err(validation_error) = config.validate() {
        return Err(anyhow::anyhow!(
            "Configuration validation failed: {}",
            validation_error
        ));
    }
```

with:

```rust
    // Parse and validate configuration. `hitch-metadata` is writable by anyone
    // with push access, so this is a trust boundary — see `config_validation`.
    let config: HitchConfig =
        crate::utils::config_validation::parse_untrusted_config(&config_json)?;
```

And replace lines 406-415 (inside `modify_metadata_impl`):

```rust
        let mut config: HitchConfig =
            serde_json::from_str(&config_json).context("Failed to parse hitch.json")?;

        // Validate configuration
        if let Err(validation_error) = config.validate() {
            return Err(anyhow::anyhow!(
                "Configuration validation failed: {}",
                validation_error
            ));
        }
```

with:

```rust
        let mut config: HitchConfig =
            crate::utils::config_validation::parse_untrusted_config(&config_json)?;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p hitch parse_untrusted_config`
Expected: PASS, 7 tests.

Run: `just test`
Expected: PASS. Watch for a fixture config with a name the firewall now rejects; if one exists, the fixture was exercising a hole — fix the fixture.

- [ ] **Step 6: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/utils/config_validation.rs src/utils/mod.rs src/utils/prelude.rs tests/unit/config_validation_tests.rs tests/unit/mod.rs
git commit -m "feat: treat hitch.json as untrusted input at both parse sites"
```

---

### Task 3: End-of-options on positional refnames

`validate_name` now rejects `-`-leading names at the boundary, but defence in depth is cheap: git accepts `--` as an explicit end-of-options marker on the commands hitch passes refnames to positionally, so a name that ever slips past validation still cannot become a flag.

**Files:**
- Modify: `src/utils/git_operations.rs` (the `update_ref_cas`, `update_ref`, `delete_ref` and `rev_parse_opt` invocations)
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: no signature changes — behaviour only.

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `tests/unit/git_operations_tests.rs`:

```rust
    /// A ref whose *name* is option-shaped must be handled as a name, not a
    /// flag. This is defence in depth behind `validate_name`: `--` makes the
    /// argv unambiguous no matter what reaches it.
    #[test]
    fn test_update_ref_treats_option_shaped_name_as_a_name() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;
            env.fs.write_file("a.txt", "a")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "init"])?.assert_success();

            let git = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            let head = git
                .rev_parse_opt("refs/heads/main")?
                .or(git.rev_parse_opt("refs/heads/master")?)
                .expect("repo must have a HEAD commit");

            // git itself rejects this refname; the point is that it must fail
            // as "bad ref name", never be consumed as an option by git.
            let result = git.update_ref("refs/hitch/--upload-pack=x", &head);
            assert!(result.is_err(), "an option-shaped ref name must not succeed");

            Ok(())
        });

        Ok(())
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch update_ref_treats_option_shaped -- --nocapture`
Expected: this may already pass or may hang/misbehave depending on git's parsing. Record the actual result. If it passes, keep the test (it is the regression guard) and continue to Step 3 anyway — the `--` change is what makes it pass *for the right reason*.

- [ ] **Step 3: Add `--` to the positional-refname invocations**

In `src/utils/git_operations.rs`, in `update_ref_cas` (line ~1845) change:

```rust
        let output = self.run_git_command(&["update-ref", "-m", reason, refname, new_oid, old])?;
```

to:

```rust
        let output =
            self.run_git_command(&["update-ref", "-m", reason, "--", refname, new_oid, old])?;
```

In `update_ref` (line ~1865) change:

```rust
        let output = self.run_git_command(&["update-ref", refname, new_oid])?;
```

to:

```rust
        let output = self.run_git_command(&["update-ref", "--", refname, new_oid])?;
```

In `delete_ref` (line ~2990) and `rev_parse_opt` (line ~1814), apply the same treatment: insert `"--"` immediately before the first positional refname argument. Read each function body before editing so the marker lands after every flag, not before one.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hitch update_ref`
Expected: PASS.

Run: `just test`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/utils/git_operations.rs tests/unit/git_operations_tests.rs
git commit -m "fix: pass -- before positional ref names in update-ref and rev-parse"
```

---

### Task 4: Subprocess hardening tier

hitch's git subprocesses run under a deploy key that bypasses branch protection. A repository-local `core.hooksPath`, an `fsmonitor` hook, or a clean/smudge filter therefore executes attacker-chosen code inside that process. This task adds hardening to the shared builder — deliberately **defaulting to the safe-for-network profile** so no push or fetch can break, with the stricter `GIT_CONFIG_NOSYSTEM` profile opted into only by pure object-database calls.

Why the split, concretely: on macOS the *system* gitconfig commonly sets `credential.helper = osxkeychain`. Setting `GIT_CONFIG_NOSYSTEM` globally would break HTTPS pushes on exactly the platform this is developed on, which is the failure mode AGENTS.md already warns about twice.

**Files:**
- Modify: `src/utils/git_operations.rs:191-214`
- Test: `tests/integration/trust_boundary_tests.rs` (create), `tests/integration/mod.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `GitOperations::run_git_command(&self, args: &[&str]) -> Result<std::process::Output>` — unchanged signature, now hardened (hooks disabled, fsmonitor off, no terminal prompts).
  - `GitOperations::run_git_plumbing_command(&self, args: &[&str]) -> Result<std::process::Output>` — same plus `GIT_CONFIG_NOSYSTEM=1`, for calls that touch only the object database.

- [ ] **Step 1: Write the failing test**

Create `tests/integration/trust_boundary_tests.rs`:

```rust
//! Red-team tests: each one performs an actual attack against the built
//! binary and asserts hitch is inert. These are regression guards for the
//! trust-boundary work, not documentation.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// A repository-local `core.hooksPath` must not get hitch to execute a
    /// script. hitch runs under a deploy key that bypasses branch protection,
    /// so a hook firing inside its process is arbitrary code with push rights.
    #[test]
    fn test_repo_local_hooks_path_does_not_execute_under_hitch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let sentinel = env.temp_dir.join("PWNED");
            let hooks_dir = env.temp_dir.join("evil-hooks");
            std::fs::create_dir_all(&hooks_dir)?;

            // post-commit fires on any commit hitch makes (it commits to
            // hitch-metadata on every mutating command).
            let hook = hooks_dir.join("post-commit");
            std::fs::write(
                &hook,
                format!("#!/bin/sh\ntouch {}\n", sentinel.to_string_lossy()),
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
            }

            env.git
                .run(&["config", "core.hooksPath", &hooks_dir.to_string_lossy()])?
                .assert_success();

            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            assert!(
                !sentinel.exists(),
                "a repo-local hook executed inside hitch's process"
            );

            Ok(())
        });

        Ok(())
    }
}
```

Register in `tests/integration/mod.rs`:

```rust
pub mod status_tests;
pub mod tree_tests;
pub mod trust_boundary_tests;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch repo_local_hooks_path -- --nocapture`
Expected: FAIL with `a repo-local hook executed inside hitch's process`.

- [ ] **Step 3: Write minimal implementation**

In `src/utils/git_operations.rs`, replace `run_git_command` (lines 191-214) with:

```rust
    /// Flags applied to every git subprocess hitch spawns.
    ///
    /// hitch's automation runs under a deploy key that is explicitly allowed to
    /// bypass the `hitch-protection` ruleset, so anything git executes on its
    /// behalf inherits push rights to protected branches. Repository-local
    /// config chooses those programs — `core.hooksPath` picks the hook
    /// directory, `core.fsmonitor` names a program git runs on status — and
    /// repository-local config is writable by anyone who can push. So they are
    /// turned off here rather than trusted.
    const HARDENING_ARGS: &'static [&'static str] = &[
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
    ];

    fn git_command(&self, args: &[&str]) -> Command {
        #[allow(clippy::disallowed_methods)] // the single blessed spawn point
        let mut cmd = Command::new("git");
        cmd.args(Self::HARDENING_ARGS);
        cmd.args(args);
        cmd.current_dir(&self.repo_path);
        // Force a stable, English locale so that the stdout/stderr substring checks
        // used throughout this module (e.g. "nothing to commit", "No local changes
        // to save", fetch "no remote" messages) are not broken by a user's locale.
        cmd.env("LC_ALL", "C");
        cmd.env("LANG", "C");
        // Never let git open an interactive credential prompt: hitch's own
        // prompts all go through the `Confirm` trait, so a prompt here can only
        // be a hang waiting on a terminal that may not exist (CI).
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        // `Command::output()` leaves stdin at its default of inherited, which
        // means git (or anything it shells out to in turn — a GPG/SSH commit
        // signing prompt, a pager, an editor) can block reading from the
        // *caller's* real terminal. Every prompt hitch itself wants goes
        // through the `Confirm` trait, never raw git — so no invocation from
        // here should ever be able to wait on interactive input; give it
        // /dev/null instead so anything that tries fails fast (or errors)
        // rather than hanging indefinitely.
        cmd.stdin(Stdio::null());
        cmd
    }

    pub fn run_git_command(&self, args: &[&str]) -> Result<std::process::Output> {
        self.git_command(args).output().context(format!(
            "Failed to execute git command: git {} in repository at {}",
            args.join(" "),
            self.repo_path
        ))
    }

    /// Like [`run_git_command`], but additionally ignores the *system* git
    /// config.
    ///
    /// Only for calls that touch nothing but the object database
    /// (`merge-tree`, `commit-tree`, `mktree`, `hash-object`, `cat-file`,
    /// `update-ref`): those need no credential helper, and the system config is
    /// one more place a shared CI image can inject behaviour. Network calls
    /// must NOT use this — on macOS the system config is where
    /// `credential.helper = osxkeychain` lives, and dropping it breaks HTTPS
    /// pushes.
    pub fn run_git_plumbing_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut cmd = self.git_command(args);
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.output().context(format!(
            "Failed to execute git plumbing command: git {} in repository at {}",
            args.join(" "),
            self.repo_path
        ))
    }
```

Then switch the pure object-database call sites to the plumbing variant. Find them with:

```bash
grep -n 'run_git_command(&\["\(merge-tree\|commit-tree\|mktree\|cat-file\|hash-object\|update-ref\|rev-parse\|for-each-ref\)' src/utils/git_operations.rs
```

Change each matched `self.run_git_command(` to `self.run_git_plumbing_command(`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hitch repo_local_hooks_path -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS.

- [ ] **Step 5: Manual end-to-end check**

AGENTS.md requires exercising user-visible changes against a real throwaway repo. Subprocess environment bugs are exactly the class that passes in tests and fails on a real machine:

```bash
just build && cd /tmp && rm -rf hitch-smoke && mkdir hitch-smoke && cd hitch-smoke && git init -q && git commit -q --allow-empty -m init && /Users/martin/Developer/hitch/target/release/hitch init && /Users/martin/Developer/hitch/target/release/hitch add dev && /Users/martin/Developer/hitch/target/release/hitch status
```

Expected: `init`, `add` and `status` all succeed with no hang and no credential prompt.

- [ ] **Step 6: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/utils/git_operations.rs tests/integration/trust_boundary_tests.rs tests/integration/mod.rs
git commit -m "feat: disable hooks and fsmonitor in every git subprocess hitch spawns"
```

---

### Task 5: Machine-enforce the single choke point

AGENTS.md says `git_operations.rs` is the only place that shells out to git, and that every git/gh subprocess must null its stdin. Nothing enforces either. This task makes a bypass a compile-time failure and fixes the two real bugs the audit found in `gh.rs`.

**Files:**
- Create: `clippy.toml`
- Modify: `src/utils/git_operations.rs` (the four remaining raw `Command::new("git")` sites at ~555, ~782, ~1925, ~2877, ~2913)
- Modify: `src/utils/gh.rs:26`, `src/utils/gh.rs:234`
- Modify: `tests/test_framework/command_runners.rs:296`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: `GitOperations::git_command` from Task 4.
- Produces: no new public API. `clippy.toml` makes `std::process::Command::new` a denied method that must be `#[allow(clippy::disallowed_methods)]`-annotated at each blessed site.

- [ ] **Step 1: Add the lint and watch it fail the build**

Create `clippy.toml`:

```toml
# Every git/gh subprocess must go through the builders in
# src/utils/git_operations.rs (and, for the test harness, the runners in
# tests/test_framework/command_runners.rs). Those builders are what force
# LC_ALL=C, null stdin, and the hardening flags. A new Command::new anywhere
# else silently opts out of all three — which has already produced two real
# bugs (a suite that hung on inherited stdin, and locale-dependent stderr
# matching). Denying it here means a bypass must be written as an explicit
# #[allow(clippy::disallowed_methods)] that a reviewer can see.
disallowed-methods = [
    { path = "std::process::Command::new", reason = "spawn git/gh via GitOperations::git_command; annotate with #[allow(clippy::disallowed_methods)] if this really is a new blessed spawn point" },
]
```

Run: `just lint`
Expected: FAIL with one `use of a disallowed method` error per raw `Command::new` site — in `src/utils/git_operations.rs`, `src/utils/gh.rs`, and `tests/test_framework/command_runners.rs`.

- [ ] **Step 2: Fold the two deploy-key push sites onto the shared builder**

In `src/utils/git_operations.rs`, `push_with_ssh_identity` (line ~782) and `force_push_with_ssh_identity` (line ~1925) each hand-build a `Command`. Replace each `let mut cmd = Command::new("git"); ...` prologue with the shared builder plus the one thing they need on top. For `push_with_ssh_identity`:

```rust
        let mut cmd = self.git_command(&["push", remote_url, &target]);
        cmd.env(
            "GIT_SSH_COMMAND",
            format!(
                "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
                ssh_identity_file
            ),
        );
```

For `force_push_with_ssh_identity`:

```rust
        let mut cmd = self.git_command(&["push", remote_url, &target, &lease]);
        cmd.env(
            "GIT_SSH_COMMAND",
            format!(
                "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
                ssh_identity_file
            ),
        );
```

Delete the now-duplicated `current_dir` / `LC_ALL` / `LANG` / `stdin` lines in both — `git_command` sets them.

- [ ] **Step 3: Annotate the three piped-stdin plumbing sites**

`stage_file_in_pending_write` (~555), `hash_object_bytes` (~2877) and `make_blob_tree` (~2913) genuinely need `Stdio::piped()` stdin and cannot use `git_command`. Add directly above each `Command::new("git")`:

```rust
        // Piped stdin is the point here (content is written to git, then the
        // pipe is closed), so this cannot use the null-stdin builder.
        #[allow(clippy::disallowed_methods)]
```

- [ ] **Step 4: Fix the two real bugs in gh.rs**

`owner_repo_from_remote` (src/utils/gh.rs:234) runs git with **no `current_dir`** — it answers for whatever directory the process happens to be in, not the repository — and with no `LC_ALL=C`. Replace it:

```rust
/// Extract owner/repo from a git remote URL.
pub fn owner_repo_from_remote() -> Result<(String, String)> {
    // No current_dir here historically, which meant this answered for the
    // process's cwd rather than the repository; run it through GitOperations so
    // it is anchored to the repo and gets the same locale/stdin discipline as
    // every other git call.
    let git = crate::utils::git_operations::GitOperations::new()?;
    let output = git.run_git_command(&["remote", "get-url", "origin"])?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
```

(leave the rest of the function body unchanged.)

For the `gh` spawn at `src/utils/gh.rs:26`, add above it:

```rust
    // `gh` is a separate binary with its own auth flow; stdin is nulled just
    // like git so it can never block on a terminal.
    #[allow(clippy::disallowed_methods)]
```

and confirm the builder already sets `.stdin(std::process::Stdio::null())`. If it does not, add it.

- [ ] **Step 5: Annotate the test harness spawn**

Above `Command::new("git")` in `tests/test_framework/command_runners.rs:296`:

```rust
        // The harness deliberately spawns plain git to simulate what a user
        // types; it is a blessed spawn point with its own null-stdin handling.
        #[allow(clippy::disallowed_methods)]
```

Do the same for the `hitch` binary spawn in the same file (find it with `grep -n 'Command::new' tests/test_framework/command_runners.rs`).

- [ ] **Step 6: Run lint and tests**

Run: `just lint`
Expected: PASS, zero warnings.

Run: `just test`
Expected: PASS.

- [ ] **Step 7: Update AGENTS.md**

In the architecture map entry for `src/utils/git_operations.rs`, replace the sentence beginning "Every git primitive is a named method" with:

```markdown
  Every git primitive is a named method (`merge_tree_compose`, `commit_tree`,
  `update_ref_cas`, ...). All of them build their subprocess through
  `git_command`, which forces `LC_ALL=C`/`LANG=C` (several call sites match
  English stderr substrings), `GIT_TERMINAL_PROMPT=0`, `stdin(Stdio::null())`
  (see the gotcha below), and the hardening flags `core.hooksPath=/dev/null`
  and `core.fsmonitor=false` — hitch runs under a deploy key that bypasses
  branch protection, so anything repo-local config can make git execute
  inherits those rights. `run_git_plumbing_command` adds
  `GIT_CONFIG_NOSYSTEM=1` and is for object-database-only calls; network calls
  must not use it, because on macOS the system config is where
  `credential.helper` lives. `clippy.toml` denies `std::process::Command::new`
  outright, so a new spawn point must carry an explicit
  `#[allow(clippy::disallowed_methods)]` — that annotation is the review
  signal. The `git2` dependency is present but effectively dead — only used
  for repo discovery, never for merges.
```

- [ ] **Step 8: Commit**

```bash
just format && just format-check && just lint && just test
```

```bash
git add clippy.toml src/utils/git_operations.rs src/utils/gh.rs tests/test_framework/command_runners.rs AGENTS.md
git commit -m "refactor: route every git subprocess through one hardened builder"
```

---

### Task 6: Signed resolution attestation

`refs/hitch/resolutions/*` is a silent code-injection channel: a recorded resolution's blobs are spliced into a published environment branch, and `hitch rebuild --replay-resolutions --yes` (the CI shape) applies them without a human in the loop. `ResolutionMeta.recorded_by` is self-reported by whoever wrote the ref, so it proves nothing. This task adds a real signature, verified with git's own SSH signing tools, gated behind an opt-in config flag so existing repositories keep working unchanged.

**Files:**
- Modify: `src/types.rs` (add the opt-in flag to `HitchConfig`)
- Modify: `src/utils/resolutions.rs` (sign on record, verify on load)
- Modify: `src/utils/prelude.rs:1175-1226` (`try_replay_resolution` enforces)
- Modify: `src/utils/git_operations.rs` (a `sign_bytes` / `verify_signature` pair)
- Test: `tests/integration/trust_boundary_tests.rs`

**Interfaces:**
- Consumes: `GitOperations::run_git_command` (Task 4).
- Produces:
  - `hitch::types::HitchConfig::require_signed_resolutions: bool` (serde default `false`)
  - `hitch::utils::resolutions::ResolutionMeta::signature: Option<String>` (serde default `None`)
  - `hitch::utils::resolutions::verify_resolution_signature(git: &GitOperations, res: &Resolution) -> anyhow::Result<bool>`

- [ ] **Step 1: Write the failing test**

Append to `tests/integration/trust_boundary_tests.rs`'s `mod tests`:

```rust
    /// With `require_signed_resolutions` on, a planted resolution ref — the
    /// shape an attacker with push access creates — must not be replayed into
    /// a build, even under `--yes --replay-resolutions`.
    #[test]
    fn test_unsigned_resolution_is_not_replayed_when_signing_required() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Turn the requirement on in hitch.json.
            env.git.run(&["checkout", "hitch-metadata"])?.assert_success();
            let raw = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&raw)?;
            config["require_signed_resolutions"] = serde_json::Value::Bool(true);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?.assert_success();
            env.git
                .run(&["commit", "-m", "test: require signed resolutions"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            // Build two branches that conflict on the same file.
            for (branch, content) in [("feat-a", "A\n"), ("feat-b", "B\n")] {
                env.git.run(&["checkout", "main"])?.assert_success();
                env.git.run(&["checkout", "-b", branch])?.assert_success();
                env.fs.write_file("clash.txt", content)?;
                env.git.run(&["add", "."])?.assert_success();
                env.git
                    .run(&["commit", "-m", &format!("{} edits clash.txt", branch)])?
                    .assert_success();
                env.git.run(&["checkout", "main"])?.assert_success();

                env.hitch
                    .run()
                    .args(&["promote", branch, "dev"])
                    .execute()?
                    .assert_success();
            }

            // Rebuild with replay enabled. There is no signed resolution, so
            // the conflicting branch must be held, not silently composed.
            let result = env
                .hitch
                .run()
                .args(&["rebuild", "dev", "--replay-resolutions", "--yes"])
                .execute()?;

            let combined = format!("{}{}", result.stdout(), result.stderr());
            assert!(
                !combined.contains("Applying recorded resolution"),
                "an unsigned resolution was replayed while signing was required:\n{}",
                combined
            );

            Ok(())
        });

        Ok(())
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch unsigned_resolution_is_not_replayed -- --nocapture`
Expected: FAIL to compile or FAIL at the `require_signed_resolutions` write — the field does not exist yet.

- [ ] **Step 3: Add the config flag**

In `src/types.rs`, inside `pub struct HitchConfig` (line 396), after `approval_requests`:

```rust
    /// Require every recorded conflict resolution to carry a verifiable SSH
    /// signature before `hitch rebuild --replay-resolutions` will apply it.
    ///
    /// Off by default so existing repositories keep working. On, it closes the
    /// one path by which content nobody reviewed lands on a deployable branch:
    /// `refs/hitch/resolutions/*` is writable by anyone with push access, and
    /// `--replay-resolutions --yes` (the CI shape) has no human in the loop.
    #[serde(default)]
    pub require_signed_resolutions: bool,
```

And in `HitchConfig::new()` (line 409), add `require_signed_resolutions: false,` to the struct literal.

- [ ] **Step 4: Add sign/verify primitives**

In `src/utils/git_operations.rs`, add near `hash_object_bytes`:

```rust
    /// Sign `payload` with the configured SSH signing key
    /// (`git config user.signingkey` with `gpg.format = ssh`), returning the
    /// armored signature. Returns `Ok(None)` when the repository has no SSH
    /// signing key configured — signing is opt-in, and a missing key is a
    /// configuration state, not an error.
    pub fn sign_bytes_ssh(&self, payload: &[u8]) -> Result<Option<String>> {
        let format = self.run_git_command(&["config", "--get", "gpg.format"])?;
        if String::from_utf8_lossy(&format.stdout).trim() != "ssh" {
            return Ok(None);
        }
        let key = self.run_git_command(&["config", "--get", "user.signingkey"])?;
        let key = String::from_utf8_lossy(&key.stdout).trim().to_string();
        if key.is_empty() {
            return Ok(None);
        }

        let mut payload_file = tempfile::NamedTempFile::new()
            .context("Failed to create a temporary file for signing")?;
        std::io::Write::write_all(&mut payload_file, payload)
            .context("Failed to write signing payload")?;
        let payload_path = payload_file.path().to_string_lossy().to_string();

        // ssh-keygen writes the armored signature to <payload>.sig beside the
        // payload; there is no stdout mode for `-Y sign`.
        #[allow(clippy::disallowed_methods)] // ssh-keygen is not git; blessed spawn point
        let signed = std::process::Command::new("ssh-keygen")
            .args(["-Y", "sign", "-f", &key, "-n", "hitch-resolution", &payload_path])
            .current_dir(&self.repo_path)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .context("Failed to run 'ssh-keygen -Y sign'")?;
        if !signed.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to sign the resolution with key '{}': {}",
                key,
                String::from_utf8_lossy(&signed.stderr).trim()
            ));
        }

        let sig = std::fs::read_to_string(format!("{}.sig", payload_path))
            .context("ssh-keygen reported success but wrote no signature file")?;
        let _ = std::fs::remove_file(format!("{}.sig", payload_path));
        Ok(Some(sig))
    }

    // NOTE for the implementer: `sign_bytes_ssh` reads `gpg.format` and
    // `user.signingkey` through `run_git_command` before shelling out to
    // `ssh-keygen`. Both reads go through the hardened builder from Task 4, so
    // no extra environment handling is needed here.

    /// Verify `signature` over `payload` against the repository's configured
    /// allowed-signers file (`git config gpg.ssh.allowedSignersFile`).
    ///
    /// Returns `Ok(false)` for any failure to verify — a missing allowed-signers
    /// file, an unknown signer, a tampered payload. Verification failing open
    /// would defeat the entire point.
    pub fn verify_signature_ssh(
        &self,
        payload: &[u8],
        signature: &str,
        signer: &str,
    ) -> Result<bool> {
        let allowed = self.run_git_command(&["config", "--get", "gpg.ssh.allowedSignersFile"])?;
        let allowed = String::from_utf8_lossy(&allowed.stdout).trim().to_string();
        if allowed.is_empty() {
            return Ok(false);
        }

        let mut payload_file = tempfile::NamedTempFile::new()
            .context("Failed to create a temporary file for verification")?;
        std::io::Write::write_all(&mut payload_file, payload)
            .context("Failed to write verification payload")?;

        let mut sig_file = tempfile::NamedTempFile::new()
            .context("Failed to create a temporary file for the signature")?;
        std::io::Write::write_all(&mut sig_file, signature.as_bytes())
            .context("Failed to write signature")?;

        #[allow(clippy::disallowed_methods)] // ssh-keygen is not git; blessed spawn point
        let verified = std::process::Command::new("ssh-keygen")
            .args([
                "-Y",
                "verify",
                "-f",
                &allowed,
                "-I",
                signer,
                "-n",
                "hitch-resolution",
                "-s",
                &sig_file.path().to_string_lossy(),
            ])
            .current_dir(&self.repo_path)
            .env("LC_ALL", "C")
            .stdin(Stdio::from(
                std::fs::File::open(payload_file.path())
                    .context("Failed to reopen the verification payload")?,
            ))
            .output()
            .context("Failed to run 'ssh-keygen -Y verify'")?;

        Ok(verified.status.success())
    }
```

Delete the two placeholder lines marked `placeholder replaced below` and the `let _ = output;` beneath them — they are scaffolding notes, not code; the signing path uses `ssh-keygen` directly.

- [ ] **Step 5: Carry the signature on the resolution**

In `src/utils/resolutions.rs`, add to `ResolutionMeta` (line 63), after `files`:

```rust
    /// Armored SSH signature over the canonical meta payload (this struct
    /// serialized with `signature` set to `None`). `None` for resolutions
    /// recorded before signing existed, or on a machine with no SSH signing
    /// key configured.
    #[serde(default)]
    pub signature: Option<String>,
```

In `record_resolution` (line 137), after building `meta` and before `serde_json::to_vec_pretty(&meta)`:

```rust
    // Sign the meta payload as it stands *without* the signature field, so
    // verification can reconstruct exactly these bytes.
    let mut meta = meta;
    let unsigned = serde_json::to_vec_pretty(&meta)?;
    meta.signature = git.sign_bytes_ssh(&unsigned)?;
```

Add the verification entry point at the end of the file:

```rust
/// Verify a loaded resolution's signature.
///
/// Reconstructs the exact payload that was signed (the meta with `signature`
/// cleared) and checks it against the repository's allowed-signers file, with
/// `recorded_by` as the claimed signer identity. Any failure — no signature,
/// no allowed-signers file, unknown signer, altered content — is `false`.
pub fn verify_resolution_signature(git: &GitOperations, res: &Resolution) -> Result<bool> {
    let Some(signature) = res.meta.signature.as_deref() else {
        return Ok(false);
    };
    let mut unsigned = res.meta.clone();
    unsigned.signature = None;
    let payload = serde_json::to_vec_pretty(&unsigned)?;
    git.verify_signature_ssh(&payload, signature, &res.meta.recorded_by)
}
```

- [ ] **Step 6: Enforce at the replay gate**

In `src/utils/prelude.rs`, in `try_replay_resolution`, immediately after the `let Some(res) = resolutions::load_resolution(...)` block (line ~1191) and before the authorization prompt:

```rust
    // A recorded resolution is content nobody on this machine reviewed, spliced
    // into a branch that deploys. When the repository has opted in, it must
    // carry a signature from a signer the repository trusts — `recorded_by`
    // alone is self-reported by whoever wrote the ref and proves nothing.
    let require_signed = access_metadata_read_only(context, |config| {
        Ok(config.require_signed_resolutions)
    })
    .unwrap_or(false);
    if require_signed && !resolutions::verify_resolution_signature(context.git(), &res)? {
        context.log_warning(&format!(
            "Recorded resolution {} for '{}' is not signed by a trusted signer and this \
             repository requires signed resolutions — holding '{}' instead. To inspect it:\n  \
             hitch resolutions",
            &key[..12.min(key.len())],
            branch,
            branch
        ));
        return Ok(None);
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p hitch unsigned_resolution_is_not_replayed -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS. Existing resolution tests must still pass — `require_signed_resolutions` defaults to `false`, so nothing changes for them.

- [ ] **Step 8: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/types.rs src/utils/resolutions.rs src/utils/prelude.rs src/utils/git_operations.rs tests/integration/trust_boundary_tests.rs
git commit -m "feat: optional signed attestation for replayed conflict resolutions"
```

---

### Task 7: Document the remaining trust gap

One vector is not closed by Task 4 and must not be left as an unknown: `git merge-tree` honours custom merge drivers declared by an in-tree `.gitattributes`. The driver *command* comes from git config rather than the tree, so exploiting it needs both — but a repository that legitimately configures a merge driver has a composition path that runs a program. Document it rather than pretend.

**Files:**
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing executable.

- [ ] **Step 1: Add the gotcha**

Append to the "Concrete gotchas, found the hard way" section of `AGENTS.md`:

```markdown
**Known, deliberately-open trust gap: custom merge drivers.** `merge_tree_compose`
runs ORT, and ORT honours a `merge=<driver>` attribute from an in-tree
`.gitattributes`. The driver's *command* comes from git config (`merge.<driver>.driver`),
not from the tree, so an attacker who can only push commits cannot by itself
choose a program to run — they need the victim's config to already define that
driver. The hardening in `git_command` does not close this (there is no
supported way to disable merge drivers while keeping ORT's real behaviour, and
faking it would break `test_merge_tree_compose_matches_real_merge_across_scenarios`,
which is the load-bearing correctness guarantee). Treat any repository that
configures a merge driver as one where composition executes that program, and
do not add config-based "mitigations" that silently change merge semantics.
```

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs: record the custom-merge-driver trust gap in AGENTS.md"
```

---

## Phase 2 — Atomic publish

### Task 8: Multi-ref transaction primitive

`git update-ref --stdin` applies a whole batch of edits atomically — every edit lands or none do — in every git at or above the 2.30 floor. hitch does not use it. This task adds the primitive and proves the all-or-nothing property before anything depends on it.

**Files:**
- Modify: `src/utils/git_operations.rs`
- Test: `tests/unit/git_operations_tests.rs`

**Interfaces:**
- Consumes: `GitOperations::git_command` (Task 4).
- Produces:
  ```rust
  pub enum RefEdit {
      Update { refname: String, new_oid: String, expected_old: Option<String> },
      Create { refname: String, new_oid: String },
      Delete { refname: String, expected_old: Option<String> },
  }
  impl GitOperations {
      pub fn ref_transaction(&self, edits: &[RefEdit], reason: &str) -> anyhow::Result<()>;
  }
  ```
  `Update { expected_old: None }` means "must not currently exist" (40 zeros), matching `update_ref_cas`'s existing semantics. `Delete { expected_old: None }` means "delete whatever is there".

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `tests/unit/git_operations_tests.rs`:

```rust
    /// The whole point of a transaction: one bad compare-and-swap in the batch
    /// must leave every other ref in the batch untouched.
    #[test]
    fn test_ref_transaction_is_all_or_nothing() -> Result<()> {
        use hitch::utils::git_operations::RefEdit;

        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;
            env.fs.write_file("a.txt", "a")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "one"])?.assert_success();
            let first = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();

            env.fs.write_file("b.txt", "b")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "two"])?.assert_success();
            let second = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();

            let git = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;

            // Edit 1 is valid (create a fresh ref). Edit 2 has a deliberately
            // wrong expected_old, so the batch must be rejected entirely.
            let edits = vec![
                RefEdit::Create {
                    refname: "refs/hitch/test/alpha".to_string(),
                    new_oid: second.clone(),
                },
                RefEdit::Update {
                    refname: "refs/hitch/test/beta".to_string(),
                    new_oid: second.clone(),
                    expected_old: Some(first.clone()),
                },
            ];

            let result = git.ref_transaction(&edits, "hitch: test transaction");
            assert!(result.is_err(), "a bad CAS must fail the whole transaction");

            assert!(
                git.rev_parse_opt("refs/hitch/test/alpha")?.is_none(),
                "the valid edit must have been rolled back with the bad one"
            );

            // And the happy path must actually apply every edit.
            let ok = vec![
                RefEdit::Create {
                    refname: "refs/hitch/test/alpha".to_string(),
                    new_oid: first.clone(),
                },
                RefEdit::Create {
                    refname: "refs/hitch/test/gamma".to_string(),
                    new_oid: second.clone(),
                },
            ];
            git.ref_transaction(&ok, "hitch: test transaction")?;
            assert_eq!(git.rev_parse_opt("refs/hitch/test/alpha")?, Some(first));
            assert_eq!(git.rev_parse_opt("refs/hitch/test/gamma")?, Some(second));

            Ok(())
        });

        Ok(())
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch ref_transaction_is_all_or_nothing`
Expected: FAIL to compile — `no function or associated item named ref_transaction`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/utils/git_operations.rs`, near `update_ref_cas`:

```rust
/// One edit in a [`GitOperations::ref_transaction`] batch.
///
/// `Update { expected_old: None }` requires the ref to not currently exist —
/// the same semantics `update_ref_cas` gives a `None` expected value.
/// `Delete { expected_old: None }` deletes whatever is there.
#[derive(Debug, Clone)]
pub enum RefEdit {
    Update {
        refname: String,
        new_oid: String,
        expected_old: Option<String>,
    },
    Create {
        refname: String,
        new_oid: String,
    },
    Delete {
        refname: String,
        expected_old: Option<String>,
    },
}

const ZERO_OID: &str = "0000000000000000000000000000000000000000";
```

and the method inside `impl GitOperations`:

```rust
    /// Apply every edit in `edits` as one atomic git ref transaction.
    ///
    /// `git update-ref --stdin` applies a batch all-or-nothing: if any
    /// compare-and-swap in it fails, none of the edits land. That is what makes
    /// it worth using over a sequence of `update_ref_cas` calls — publishing an
    /// environment build moves several refs that must agree with each other
    /// (the branch itself, its archived previous tip, the build anchor), and a
    /// crash between separate updates leaves a state nobody designed.
    ///
    /// Input is NUL-delimited (`-z`) so a ref name can never be misread,
    /// whatever it contains. `reason` becomes the reflog message for every
    /// edit in the batch.
    pub fn ref_transaction(&self, edits: &[RefEdit], reason: &str) -> Result<()> {
        if edits.is_empty() {
            return Ok(());
        }

        let mut input: Vec<u8> = Vec::new();
        for edit in edits {
            match edit {
                RefEdit::Update {
                    refname,
                    new_oid,
                    expected_old,
                } => {
                    input.extend_from_slice(b"update ");
                    input.extend_from_slice(refname.as_bytes());
                    input.push(0);
                    input.extend_from_slice(new_oid.as_bytes());
                    input.push(0);
                    input.extend_from_slice(expected_old.as_deref().unwrap_or(ZERO_OID).as_bytes());
                    input.push(0);
                }
                RefEdit::Create { refname, new_oid } => {
                    input.extend_from_slice(b"create ");
                    input.extend_from_slice(refname.as_bytes());
                    input.push(0);
                    input.extend_from_slice(new_oid.as_bytes());
                    input.push(0);
                }
                RefEdit::Delete {
                    refname,
                    expected_old,
                } => {
                    input.extend_from_slice(b"delete ");
                    input.extend_from_slice(refname.as_bytes());
                    input.push(0);
                    input.extend_from_slice(expected_old.as_deref().unwrap_or("").as_bytes());
                    input.push(0);
                }
            }
        }

        // Piped stdin is required here (the batch is written to git, then the
        // pipe is closed), so this cannot use the null-stdin builder.
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"])
            .args(["update-ref", "-m", reason, "-z", "--stdin"])
            .current_dir(&self.repo_path)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn 'git update-ref --stdin'")?;

        child
            .stdin
            .take()
            .expect("stdin was configured as piped")
            .write_all(&input)
            .context("Failed to write the ref transaction to git update-ref")?;

        let output = child
            .wait_with_output()
            .context("Failed to wait for 'git update-ref --stdin'")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Ref transaction of {} edit(s) was rejected — no ref was changed: {}",
                edits.len(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hitch ref_transaction_is_all_or_nothing -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/utils/git_operations.rs tests/unit/git_operations_tests.rs
git commit -m "feat: add atomic multi-ref transaction primitive"
```

---

### Task 9: Publish in one transaction

`publish_environment_build` (src/utils/prelude.rs:1036) currently makes three separate ref writes before the branch moves: the pending-resync record, the backup ref, and the CAS. A crash between any two leaves a state that is at best untidy and at worst a backup ref pointing at a tip nobody recorded. One transaction removes those windows and adds `refs/hitch/prev/<env>/<ts>` as the rollback anchor.

**Files:**
- Modify: `src/utils/prelude.rs:1036-1093`
- Modify: `src/utils/pending_resync.rs` (expose a payload-to-blob helper)
- Test: `tests/integration/rebuild_tests.rs`

**Interfaces:**
- Consumes: `GitOperations::ref_transaction`, `RefEdit` (Task 8).
- Produces:
  - `hitch::utils::pending_resync::record_blob(context: &GlobalContext, pending: &PendingResync) -> anyhow::Result<(String, String)>` returning `(refname, blob_oid)` without writing the ref, so the write can join the transaction.
  - New ref namespace: `refs/hitch/prev/<env>/<timestamp>` holding the tip that was replaced.

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `tests/integration/rebuild_tests.rs`:

```rust
    /// Every rebuild must leave behind the tip it replaced, under
    /// refs/hitch/prev/<env>/<timestamp> — that ref is what makes rollback a
    /// one-ref flip instead of an archaeology exercise in the reflog.
    #[test]
    fn test_rebuild_archives_previous_tip_under_prev_ref() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?.assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "feature 1"])?.assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev", "--no-push", "--yes"])
                .execute()?
                .assert_success();

            let first_tip = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();

            // A second rebuild replaces that tip, so it must be archived.
            env.git.run(&["checkout", "-b", "feature-2"])?.assert_success();
            env.fs.write_file("2.txt", "two")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "feature 2"])?.assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-2", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev", "--no-push", "--yes"])
                .execute()?
                .assert_success();

            let prev_refs = env
                .git
                .run(&["for-each-ref", "--format=%(objectname)", "refs/hitch/prev/dev"])?
                .stdout();

            assert!(
                prev_refs.lines().any(|line| line.trim() == first_tip),
                "expected the replaced tip {} under refs/hitch/prev/dev, got:\n{}",
                first_tip,
                prev_refs
            );

            Ok(())
        });

        Ok(())
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch rebuild_archives_previous_tip -- --nocapture`
Expected: FAIL — `expected the replaced tip ... under refs/hitch/prev/dev, got:` followed by empty output.

- [ ] **Step 3: Split the pending-resync write into blob-then-ref**

In `src/utils/pending_resync.rs`, replace `record` (line 54) with:

```rust
/// Hash the payload for `pending` and return `(refname, blob_oid)` without
/// writing the ref.
///
/// Splitting the write lets the caller put the ref update inside a larger
/// atomic transaction alongside the branch move it describes, instead of doing
/// it as a separate step with a crash window in between.
pub fn record_blob(context: &GlobalContext, pending: &PendingResync) -> Result<(String, String)> {
    let payload = serde_json::to_vec_pretty(pending)?;
    let blob = context.git().hash_object_bytes(&payload)?;
    Ok((ref_name(&pending.branch), blob))
}

/// Record the intent to move `branch` and resync `checkouts`, before anything
/// observable changes. Blob-backed so the payload is readable with
/// `git cat-file -p` when diagnosing by hand.
pub fn record(context: &GlobalContext, pending: &PendingResync) -> Result<()> {
    let (refname, blob) = record_blob(context, pending)?;
    context.git().update_ref(&refname, &blob)
}
```

- [ ] **Step 4: Publish in one transaction**

In `src/utils/prelude.rs`, replace lines 1046-1085 of `publish_environment_build` (from the `// Must happen before the ref moves` comment through the closing brace of the `if let Err(e) = context.git().update_ref_cas(...)` block) with:

```rust
    // Must happen before the ref moves — see `scan_checkouts_on_branch`.
    let checkouts = scan_checkouts_on_branch(context, env_name)?;

    // Everything that must agree with the branch move goes in with it: the
    // resync intent (so a crash after the move is recoverable — see
    // `crate::utils::pending_resync`), and the tip being replaced (so rollback
    // is a one-ref flip). `git update-ref --stdin` applies the batch
    // all-or-nothing, so there is no longer a window where the branch has moved
    // but its intent record or its archive is missing.
    let (resync_ref, resync_blob) = crate::utils::pending_resync::record_blob(
        context,
        &crate::utils::pending_resync::PendingResync {
            branch: env_name.to_string(),
            from_sha: old_env_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
        },
    )?;

    let mut edits = vec![
        crate::utils::git_operations::RefEdit::Update {
            refname: resync_ref,
            new_oid: resync_blob,
            expected_old: None,
        },
        crate::utils::git_operations::RefEdit::Update {
            refname: env_ref.clone(),
            new_oid: new_sha.to_string(),
            expected_old: old_env_sha.clone(),
        },
    ];

    if let Some(ref old_sha) = old_env_sha {
        edits.push(crate::utils::git_operations::RefEdit::Create {
            refname: format!("refs/hitch/prev/{}/{}", env_name, backup_timestamp),
            new_oid: old_sha.clone(),
        });
        edits.push(crate::utils::git_operations::RefEdit::Create {
            refname: format!("refs/hitch/backup/{}/{}", env_name, backup_timestamp),
            new_oid: old_sha.clone(),
        });
    }

    if let Err(e) = context
        .git()
        .ref_transaction(&edits, &format!("hitch: rebuild {}", env_name))
    {
        return Err(anyhow::anyhow!(
            "Failed to publish '{}': {}. The build itself succeeded but could not be \
             published — this usually means another rebuild landed first. Fetch and re-run \
             'hitch rebuild {}'.",
            env_name,
            e,
            env_name
        ));
    }
```

Note the `RefEdit::Update` on `resync_ref` with `expected_old: None`: a leftover record from a previous crashed publish would make that fail. Recovery deletes stale records at startup (`pending_resync::recover` runs before any mutating command), so by the time this executes there is none. If a test proves otherwise, change it to `Create` semantics only after confirming recovery ran.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p hitch rebuild_archives_previous_tip -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS — in particular the existing rebuild, release and recovery tests, which cover the sequence this rewrote.

- [ ] **Step 6: Manual end-to-end check**

```bash
just build && cd /tmp && rm -rf hitch-tx && mkdir hitch-tx && cd hitch-tx && git init -q && git commit -q --allow-empty -m init && /Users/martin/Developer/hitch/target/release/hitch init && /Users/martin/Developer/hitch/target/release/hitch add dev && git checkout -q -b feat && echo x > x.txt && git add . && git commit -q -m feat && git checkout -q main && /Users/martin/Developer/hitch/target/release/hitch promote feat dev && /Users/martin/Developer/hitch/target/release/hitch rebuild dev --no-push --yes && git for-each-ref refs/hitch/
```

Expected: `rebuild` succeeds; `for-each-ref` lists a `refs/hitch/prev/dev/...` ref and no leftover `refs/hitch/pending-resync/...`.

- [ ] **Step 7: Update AGENTS.md**

In the Conventions section, replace the bullet beginning "**Publishing an environment branch is always a CAS `update-ref`**" with:

```markdown
- **Publishing an environment branch is always one atomic ref transaction**
  (`publish_environment_build` in `prelude.rs`, built on
  `GitOperations::ref_transaction`), never a rename-and-recreate dance and never
  a sequence of separate `update-ref` calls. The batch moves `refs/heads/<env>`
  under compare-and-swap, writes the pending-resync intent, and archives the
  replaced tip under `refs/hitch/prev/<env>/<timestamp>` — all or nothing.
  Reuse that function for anything that produces a new environment branch
  commit; don't hand-roll the publish step again.
```

- [ ] **Step 8: Commit**

```bash
just format && just format-check && just lint && just test
```

```bash
git add src/utils/prelude.rs src/utils/pending_resync.rs tests/integration/rebuild_tests.rs AGENTS.md
git commit -m "feat: publish an environment build in one atomic ref transaction"
```

---

### Task 10: Bound the archive refs

**Revised during Phase 2 execution.** The plan originally assumed `hitch
cleanup` already pruned `refs/hitch/backup/*` on some existing retention
policy that Task 10 would just extend to the new `refs/hitch/prev/*`
namespace. That assumption was wrong: `src/commands/cleanup.rs` (read in
full during Phase 2 pre-flight) only ever deletes stale *feature branches*
not currently promoted to any environment — it has never touched
`refs/hitch/backup/*`, which `publish_environment_build` has been writing,
unbounded, since before this plan existed. There is no existing policy to
mirror. Confirmed via the human: design a new one now, rather than defer or
narrow scope, since unbounded ref growth is a real production problem (ref
listing and fetch negotiation both degrade) that both namespaces share.

**Policy**: count-based, not age-based. A hyperactive environment can
rebuild many times in a day (age-based would let it blow past any bound); a
quiet environment's one rollback point is worth keeping even if old
(age-based would prune it). Keep the `N` most recent refs *per environment,
per namespace* — `refs/hitch/backup/<env>/*` and `refs/hitch/prev/<env>/*`
are pruned independently of each other and of every other environment's
refs, each down to its own most-recent-`N`. "Most recent" is determined by
the timestamp segment already embedded in the ref name
(`refs/hitch/{backup,prev}/<env>/<timestamp>`, the same `backup_timestamp`
`publish_environment_build` already generates) — sorting ref names
lexically under a fixed-width timestamp format sorts them chronologically,
so no extra `for-each-ref --sort` or commit-date lookup is needed.
`N = 10` is the default (keeps roughly the last 10 rebuilds' rollback
points per environment, which is generous for manual rollback while still
bounding growth).

**Files:**
- Modify: `src/commands/cleanup.rs`
- Test: `tests/integration/cleanup_tests.rs`

**Interfaces:**
- Consumes: `refs/hitch/prev/<env>/<timestamp>` from Task 9,
  `GitOperations::list_refs_under`/`delete_ref` (both pre-existing).
- Produces: no CLI-visible signature change — `hitch cleanup --apply` now
  also prunes old archive refs (both namespaces) alongside its existing
  stale-branch deletion; dry-run (no `--apply`) lists what it would prune
  the same way it already lists candidate branches.

- [ ] **Step 1: Write the failing test**

Read `src/commands/cleanup.rs` and `tests/integration/cleanup_tests.rs` in
full first, so the new test matches this file's actual test style (helper
functions, `TestSetup` variant used, assertion style) rather than
guessing. Append to the `mod tests` block in
`tests/integration/cleanup_tests.rs`:

```rust
    /// refs/hitch/backup/* and refs/hitch/prev/* both accumulate one ref per
    /// rebuild with no existing bound — `hitch cleanup --apply` must prune
    /// each namespace down to its N-most-recent-per-environment, independent
    /// of the other namespace and of other environments.
    #[test]
    fn test_cleanup_prunes_old_archive_refs_per_env_per_namespace() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();

            let head = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .stdout()
                .trim()
                .to_string();

            // Plant more refs than the retention policy keeps, in both
            // namespaces, across two environments, so the test also proves
            // pruning is scoped per-env (dev's excess doesn't affect qa's
            // count and vice versa).
            for namespace in ["backup", "prev"] {
                for env_name in ["dev", "qa"] {
                    for i in 0..15 {
                        env.git
                            .run(&[
                                "update-ref",
                                &format!(
                                    "refs/hitch/{}/{}/2020010100{:04}",
                                    namespace, env_name, i
                                ),
                                &head,
                            ])?
                            .assert_success();
                    }
                }
            }

            env.hitch
                .run()
                .args(&["cleanup", "--apply"])
                .execute()?
                .assert_success();

            for namespace in ["backup", "prev"] {
                for env_name in ["dev", "qa"] {
                    let remaining = env
                        .git
                        .run(&[
                            "for-each-ref",
                            "--format=%(refname)",
                            &format!("refs/hitch/{}/{}", namespace, env_name),
                        ])?
                        .stdout();
                    let count = remaining.lines().filter(|l| !l.trim().is_empty()).count();
                    assert!(
                        count <= 10,
                        "cleanup left {} refs under refs/hitch/{}/{}, expected <= 10:\n{}",
                        count,
                        namespace,
                        env_name,
                        remaining
                    );
                    assert!(
                        count > 0,
                        "cleanup pruned every ref under refs/hitch/{}/{} — some of the \
                         most-recent 10 must survive",
                        namespace,
                        env_name
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }
```

If `hitch cleanup`'s actual flags differ from `--apply` (check
`src/commands/cleanup.rs`'s `CleanupCommand` struct — as of this plan's
writing it's `--apply` plus an optional `--env`, not `--yes`), use the real
flags. Do not invent a flag.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hitch cleanup_prunes_old_archive_refs -- --nocapture`
Expected: FAIL — some `refs/hitch/{backup,prev}/{dev,qa}` count assertion
reports 15 refs remaining (nothing pruned).

- [ ] **Step 3: Implement the retention policy**

In `src/commands/cleanup.rs`, add a helper and call it from `run` alongside
the existing branch-cleanup logic (same `args.apply`-gated dry-run/apply
split the branch logic already has — list candidates always, only delete
under `--apply`):

```rust
/// Environments whose archive refs get pruned. `--env` scopes this exactly
/// like it already scopes branch cleanup.
fn envs_in_scope(config: &crate::types::HitchConfig, env_filter: Option<&str>) -> Vec<String> {
    config
        .environments
        .keys()
        .filter(|e| env_filter.map(|f| f == e.as_str()).unwrap_or(true))
        .cloned()
        .collect()
}

/// How many of the most recent archive refs to keep per (namespace,
/// environment). Chosen to comfortably cover manual rollback while bounding
/// unconditional growth — see Task 10 in the production-hardening plan for
/// the reasoning against an age-based alternative.
const ARCHIVE_REF_RETENTION: usize = 10;

/// Refs older than the most recent `ARCHIVE_REF_RETENTION` under
/// `refs/hitch/<namespace>/<env>/*`, for every namespace/env pair in scope.
/// Ref names sort chronologically because the timestamp segment is
/// fixed-width, so this needs no extra date lookup.
fn stale_archive_refs(
    context: &GlobalContext,
    envs: &[String],
) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for namespace in ["backup", "prev"] {
        for env_name in envs {
            let prefix = format!("refs/hitch/{}/{}/", namespace, env_name);
            let mut refs = context.git().list_refs_under(&prefix)?;
            refs.sort(); // chronological: fixed-width timestamp suffix
            if refs.len() > ARCHIVE_REF_RETENTION {
                stale.extend(refs.into_iter().rev().skip(ARCHIVE_REF_RETENTION));
            }
        }
    }
    Ok(stale)
}
```

Wire it into `run`: compute `envs_in_scope` and `stale_archive_refs` after
the existing branch-candidate collection, report the count in the dry-run
listing (mirroring how branch candidates are printed), and under
`args.apply` delete each via `context.git().delete_ref(...)`, logging
success/failure per ref the same way branch deletion already logs per
branch. Read the existing `run` function's exact structure before editing
so the new block matches its style (log macros used, ordering of dry-run
vs apply text) rather than diverging.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hitch cleanup_prunes_old_archive_refs -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS.

- [ ] **Step 5: Manual end-to-end check**

```bash
just build && cd /tmp && rm -rf hitch-cleanup && mkdir hitch-cleanup && cd hitch-cleanup && git init -q && git commit -q --allow-empty -m init && /Users/martin/Developer/hitch/target/release/hitch init && /Users/martin/Developer/hitch/target/release/hitch add dev && for i in $(seq -w 1 12); do git update-ref refs/hitch/prev/dev/2020010100000$i $(git rev-parse HEAD); done && /Users/martin/Developer/hitch/target/release/hitch cleanup && /Users/martin/Developer/hitch/target/release/hitch cleanup --apply && git for-each-ref refs/hitch/prev/dev | wc -l
```

Expected: dry-run output mentions the archive refs it would prune; after
`--apply`, at most 10 remain.

- [ ] **Step 6: Format, lint, commit**

```bash
just format && just format-check && just lint
```

```bash
git add src/commands/cleanup.rs tests/integration/cleanup_tests.rs
git commit -m "feat: bound refs/hitch/backup and refs/hitch/prev to 10 most recent per env"
```

---

## Phase 3 — Publish journal and crash-fuzz

### Task 11: Widen the journal to cover the push

After Task 9 the local publish is atomic, so exactly two effects remain outside any transaction: the checkout resync, and the push to origin. `pending_resync` covers the first and nothing covers the second — a process killed after the ref moved but before the push leaves the local branch ahead of origin with no record that a push was owed. This task renames the module to what it now is and adds the push obligation.

Scope discipline: this is a journal, not a planner. There is no `Plan`/`Slot`/executor machinery, because there are two steps, not twenty; a general plan representation here would be abstraction ahead of a second caller.

**Files:**
- Rename: `src/utils/pending_resync.rs` → `src/utils/publish_journal.rs`
- Modify: `src/utils/mod.rs`, `src/main.rs:101`, `src/utils/prelude.rs`, `src/commands/doctor.rs` (every `pending_resync::` reference — find them with `grep -rn pending_resync src/`)
- Test: `tests/integration/crash_recovery_tests.rs` (created in Task 12)

**Interfaces:**
- Consumes: `pending_resync::{PendingResync, record_blob, record, clear, list, recover}` from Phase 2.
- Produces:
  - `hitch::utils::publish_journal::PublishRecord { branch: String, from_sha: Option<String>, to_sha: String, checkouts: Vec<String>, push_owed: bool }`
  - `hitch::utils::publish_journal::{record_blob, record, clear, list, recover}` — same shapes as before, plus `mark_push_done(context: &GlobalContext, branch: &str) -> anyhow::Result<()>`.
  - Reads legacy `refs/hitch/pending-resync/<branch>` records so an in-flight upgrade recovers rather than stranding.

- [ ] **Step 1: Rename with no behaviour change, and commit that alone**

```bash
git mv src/utils/pending_resync.rs src/utils/publish_journal.rs
```

Update `src/utils/mod.rs` (`pub mod pending_resync;` → `pub mod publish_journal;`, keeping alphabetical order), then fix every reference:

```bash
grep -rln pending_resync src/ tests/ | xargs sed -i '' 's/pending_resync/publish_journal/g'
```

Then rename the struct within `src/utils/publish_journal.rs`: `PendingResync` → `PublishRecord`, and update its references the same way. Keep `const REF_PREFIX: &str = "refs/hitch/pending-resync";` unchanged for now — the ref namespace migration is Step 3, and mixing it into the rename makes the diff unreadable.

Run: `just format && just format-check && just lint && just test`
Expected: PASS with no behaviour change.

```bash
git add -A src/ tests/
git commit -m "refactor: rename pending_resync to publish_journal"
```

- [ ] **Step 2: Write the failing test for the push obligation**

Append to `tests/integration/rebuild_tests.rs`'s `mod tests`:

```rust
    /// A publish that has not pushed yet owes a push, and that obligation must
    /// be written down — otherwise a process killed between the ref move and
    /// the push leaves the local branch ahead of origin with nothing recording
    /// why.
    #[test]
    fn test_publish_journal_records_push_obligation() -> anyhow::Result<()> {
        use hitch::utils::publish_journal::PublishRecord;

        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?.assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", "feature 1"])?.assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // --no-push means the obligation is deliberately not incurred, so
            // a completed rebuild must leave no journal record at all.
            env.hitch
                .run()
                .args(&["rebuild", "dev", "--no-push", "--yes"])
                .execute()?
                .assert_success();

            let leftovers = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                .stdout();
            assert!(
                leftovers.trim().is_empty(),
                "a completed publish left a journal record behind:\n{}",
                leftovers
            );

            // The record type must be able to express the obligation.
            let record = PublishRecord {
                branch: "dev".to_string(),
                from_sha: None,
                to_sha: "0".repeat(40),
                checkouts: vec![],
                push_owed: true,
            };
            assert!(record.push_owed);

            Ok(())
        });

        Ok(())
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p hitch publish_journal_records_push_obligation -- --nocapture`
Expected: FAIL to compile — `struct PublishRecord has no field named push_owed`.

- [ ] **Step 4: Implement the widened journal**

In `src/utils/publish_journal.rs`:

Change the ref prefix and add legacy reading:

```rust
const REF_PREFIX: &str = "refs/hitch/publish";

/// The namespace this journal used before it covered the push step. Read on
/// recovery so a hitch upgrade partway through a publish still finishes it.
const LEGACY_REF_PREFIX: &str = "refs/hitch/pending-resync";
```

Add the field to the struct:

```rust
pub struct PublishRecord {
    pub branch: String,
    /// The branch tip before the publish. A checkout still holding exactly
    /// this content is provably stale rather than edited.
    pub from_sha: Option<String>,
    pub to_sha: String,
    /// Checkout paths that had the branch attached when the publish started.
    pub checkouts: Vec<String>,
    /// Whether this publish still owes a push to origin. Set when the publish
    /// intends to push; cleared by `mark_push_done` once it has. A record found
    /// later with this still set means the process died between moving the ref
    /// and telling the remote.
    #[serde(default)]
    pub push_owed: bool,
}
```

Widen `list` to read both namespaces:

```rust
pub fn list(git: &GitOperations) -> Result<Vec<(String, PublishRecord)>> {
    let mut found = Vec::new();

    for prefix in [REF_PREFIX, LEGACY_REF_PREFIX] {
        for refname in git.list_refs_under(prefix)? {
            let Ok(payload) = git.cat_file_blob(&refname) else {
                continue;
            };
            match serde_json::from_slice::<PublishRecord>(&payload) {
                Ok(record) => found.push((refname, record)),
                // A record we can't parse is not something to act on, but it is
                // also not something to delete silently — leave it for `doctor`.
                Err(_) => continue,
            }
        }
    }

    Ok(found)
}
```

Add the push-completion marker:

```rust
/// Clear the push obligation on `branch`'s record, leaving the resync
/// obligation intact.
///
/// The two obligations are cleared separately because they complete at
/// different times: the resync happens locally and immediately, the push
/// happens afterwards and can be declined or fail.
pub fn mark_push_done(context: &GlobalContext, branch: &str) -> Result<()> {
    let refname = ref_name(branch);
    let Ok(payload) = context.git().cat_file_blob(&refname) else {
        return Ok(());
    };
    let Ok(mut record) = serde_json::from_slice::<PublishRecord>(&payload) else {
        return Ok(());
    };
    if !record.push_owed {
        return Ok(());
    }
    record.push_owed = false;
    let updated = serde_json::to_vec_pretty(&record)?;
    let blob = context.git().hash_object_bytes(&updated)?;
    context.git().update_ref(&refname, &blob)
}
```

Extend `recover` so an owed push is reported rather than silently performed — pushing on someone else's behalf during a startup recovery pass is a network side effect nobody asked for. After the existing per-checkout repair loop and before the record is deleted:

```rust
        if record.push_owed {
            context.log_warning(&format!(
                "A previous '{}' publish moved the branch but was interrupted before it \
                 could push. The local branch is ahead of origin. To finish it:\n  \
                 hitch push {} -f",
                record.branch, record.branch
            ));
        }
```

- [ ] **Step 5: Set and clear the obligation in `publish_environment_build`**

In `src/utils/prelude.rs`, in the `record_blob` call inside `publish_environment_build`, set the new field:

```rust
    let (resync_ref, resync_blob) = crate::utils::publish_journal::record_blob(
        context,
        &crate::utils::publish_journal::PublishRecord {
            branch: env_name.to_string(),
            from_sha: old_env_sha.clone(),
            to_sha: new_sha.to_string(),
            checkouts: checkout_paths(&checkouts),
            push_owed: context.should_push(),
        },
    )?;
```

Then change the resync/clear sequence so the record survives until the push resolves. Replace:

```rust
    resync_checkouts(context, env_name, new_sha, &checkouts);
    crate::utils::publish_journal::clear(context, env_name);
```

with:

```rust
    resync_checkouts(context, env_name, new_sha, &checkouts);
    if !context.should_push() {
        // Nothing else is owed — drop the record now.
        crate::utils::publish_journal::clear(context, env_name);
    }
```

And in the push block, after the successful-push `log_success` call, add:

```rust
                    let _ = crate::utils::publish_journal::mark_push_done(context, env_name);
                    crate::utils::publish_journal::clear(context, env_name);
```

In the declined-push branch (the `else` after `context.confirm`), also clear — the user chose not to push, so nothing is owed:

```rust
            crate::utils::publish_journal::clear(context, env_name);
```

Leave the record in place on push *failure*: that is precisely the state the journal exists to remember.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p hitch publish_journal_records_push_obligation -- --nocapture`
Expected: PASS.

Run: `just test`
Expected: PASS.

- [ ] **Step 7: Update AGENTS.md**

Replace the `src/utils/pending_resync.rs` entry in the architecture map with:

```markdown
- `src/utils/publish_journal.rs` — the record of what a publish still owes,
  for the effects git cannot make atomic. The publish itself is one ref
  transaction (see `publish_environment_build`); what remains outside it is the
  checkout resync and the push to origin. Both obligations are written to
  `refs/hitch/publish/<branch>` before the ref moves, inside the same
  transaction, and cleared as each completes; `recover` runs from `main.rs` for
  mutating commands only, so it is always under the repo lock. It repairs a
  checkout only when that tree is *provably* exactly the old tip — never on the
  dead process's say-so — so an edited tree is reported, not reset, and an owed
  push is reported rather than performed. Legacy
  `refs/hitch/pending-resync/<branch>` records are still read so an upgrade
  mid-publish recovers.
```

- [ ] **Step 8: Commit**

```bash
just format && just format-check && just lint && just test
```

```bash
git add src/utils/publish_journal.rs src/utils/prelude.rs AGENTS.md
git commit -m "feat: record the owed push in the publish journal"
```

---

### Task 12: Crash-fuzz the recovery path

Recovery code is the least-exercised code in the system, and there is currently no test that interrupts a publish at an arbitrary point and asserts convergence. This is the analogue of `test_merge_tree_compose_matches_real_merge_across_scenarios` for the crash path: a differential test, not a scenario test.

**Files:**
- Create: `tests/integration/crash_recovery_tests.rs`
- Modify: `tests/integration/mod.rs`
- Modify: `src/utils/publish_journal.rs` (a test-only interruption point)

**Interfaces:**
- Consumes: `publish_journal` (Task 11), `publish_environment_build` (Task 9).
- Produces: environment variable `HITCH_TEST_ABORT_AFTER` — read only in test builds, names the publish step after which the process aborts. Values: `journal-written`, `ref-moved`, `resync-done`.

- [ ] **Step 1: Write the failing test**

Create `tests/integration/crash_recovery_tests.rs`:

```rust
//! Crash-fuzz: interrupt a publish at each step and assert that the next
//! hitch invocation converges to the same state an uninterrupted run reaches.
//!
//! This is a differential test in the same spirit as the merge-tree/real-merge
//! comparison: the uninterrupted run is the oracle, and every interruption
//! point must agree with it.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Build the same scenario every time: one environment, one promoted
    /// branch, ready to rebuild.
    fn setup(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.git.run(&["checkout", "-b", "feature-1"])?.assert_success();
        env.fs.write_file("1.txt", "one")?;
        env.git.run(&["add", "."])?.assert_success();
        env.git.run(&["commit", "-m", "feature 1"])?.assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();

        env.hitch
            .run()
            .args(&["promote", "feature-1", "dev"])
            .execute()?
            .assert_success();
        Ok(())
    }

    /// A rebuild aborted after each step, then re-run, must land on the same
    /// dev tip as a rebuild that was never interrupted, and must leave no
    /// journal record behind.
    #[test]
    fn test_publish_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        // The oracle: an uninterrupted run.
        let expected_tip = {
            let framework = HitchTestFramework::new()?;
            let mut tip = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;
                env.hitch
                    .run()
                    .args(&["rebuild", "dev", "--no-push", "--yes"])
                    .execute()?
                    .assert_success();
                tip = env
                    .git
                    .run(&["rev-parse", "refs/heads/dev"])?
                    .stdout()
                    .trim()
                    .to_string();
                Ok(())
            });
            tip
        };

        assert!(!expected_tip.is_empty(), "the oracle run produced no tip");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;

            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;

                // Interrupted run: expected to die, not to succeed.
                let _ = env
                    .hitch
                    .run()
                    .args(&["rebuild", "dev", "--no-push", "--yes"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute();

                // Recovery runs at the start of any mutating command.
                env.hitch
                    .run()
                    .args(&["rebuild", "dev", "--no-push", "--yes"])
                    .execute()?
                    .assert_success();

                let tip = env
                    .git
                    .run(&["rev-parse", "refs/heads/dev"])?
                    .stdout()
                    .trim()
                    .to_string();
                assert_eq!(
                    tip, expected_tip,
                    "aborting after '{}' did not converge to the uninterrupted tip",
                    abort_after
                );

                let leftovers = env
                    .git
                    .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                    .stdout();
                assert!(
                    leftovers.trim().is_empty(),
                    "aborting after '{}' left a journal record behind:\n{}",
                    abort_after,
                    leftovers
                );

                Ok(())
            });
        }

        Ok(())
    }
}
```

Register in `tests/integration/mod.rs`:

```rust
pub mod conflicts_tests;
pub mod crash_recovery_tests;
pub mod diff_tests;
```

- [ ] **Step 2: Add `.env()` to the hitch command builder if it is missing**

Run: `grep -n 'pub fn env' tests/test_framework/command_runners.rs`

If `HitchCommandBuilder` has no `env` method, add one next to `args`:

```rust
    /// Set an environment variable for this invocation. Used by the crash-fuzz
    /// tests to name an interruption point.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_string(), value.to_string()));
        self
    }
```

with a matching `envs: Vec<(String, String)>` field initialised to `Vec::new()` in the constructor, and an `cmd.envs(self.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())));` line in `execute()` before the spawn.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p hitch publish_converges_after_abort -- --nocapture`
Expected: FAIL — the interrupted run does not actually abort (the env var is ignored), so the assertion about journal leftovers passes vacuously while the abort points go untested. Confirm by adding a temporary `eprintln!` — or simply proceed to Step 4 and observe the test genuinely exercising the abort afterwards.

- [ ] **Step 4: Add the interruption point**

In `src/utils/publish_journal.rs`, add:

```rust
/// Abort the process at a named point in the publish sequence, if the
/// environment asks for it.
///
/// This exists solely so the crash-fuzz tests can interrupt a publish at each
/// step and assert that recovery converges. `std::process::abort` rather than a
/// normal exit, because the whole point is to skip every destructor and
/// cleanup path exactly as a `kill -9` would.
pub fn maybe_abort_for_test(point: &str) {
    if std::env::var("HITCH_TEST_ABORT_AFTER").as_deref() == Ok(point) {
        eprintln!("hitch: aborting after '{}' (HITCH_TEST_ABORT_AFTER)", point);
        std::process::abort();
    }
}
```

In `src/utils/prelude.rs`'s `publish_environment_build`, call it at the three points:

- immediately after the `ref_transaction` call returns `Ok`, before the resync:
  ```rust
    crate::utils::publish_journal::maybe_abort_for_test("ref-moved");
  ```
- immediately after `record_blob` returns, before the transaction is built:
  ```rust
    crate::utils::publish_journal::maybe_abort_for_test("journal-written");
  ```
  (this one aborts before anything observable changed, so recovery must find nothing to do)
- immediately after `resync_checkouts(...)` returns:
  ```rust
    crate::utils::publish_journal::maybe_abort_for_test("resync-done");
  ```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p hitch publish_converges_after_abort -- --nocapture`
Expected: PASS for all three abort points. If `ref-moved` fails to converge, that is a genuine recovery bug this test was written to find — fix the recovery path, not the test.

Run: `just test`
Expected: PASS.

- [ ] **Step 6: Update AGENTS.md**

Append to the "Concrete gotchas" section:

```markdown
**Recovery is tested by interruption, not by inspection.**
`tests/integration/crash_recovery_tests.rs` runs the publish sequence with
`HITCH_TEST_ABORT_AFTER` set to each named step, `std::process::abort`s there,
then re-runs the command and asserts the resulting tip equals the tip an
uninterrupted run produces and that no journal record is left behind. It is a
differential test against an oracle run, in the same spirit as
`test_merge_tree_compose_matches_real_merge_across_scenarios`. If you add a step
to a publish, add an abort point for it — a step with no abort point is a
recovery path with no test.
```

- [ ] **Step 7: Commit**

```bash
just format && just format-check && just lint && just test
```

```bash
git add tests/integration/crash_recovery_tests.rs tests/integration/mod.rs tests/test_framework/command_runners.rs src/utils/publish_journal.rs src/utils/prelude.rs AGENTS.md
git commit -m "test: crash-fuzz the publish sequence against an uninterrupted oracle"
```

---

### Task 13: Delete the dead code

`src/utils/git_error.rs`, `src/utils/hooks.rs` and `src/core/workspace.rs`'s `build_workspace_model` are documented in AGENTS.md as dead. Dead code in a production-grade codebase is a liability: it invites someone to build on it and it dilutes the signal from `-D warnings`. Phase 1 gave the codebase its typed-boundary story (validated config, hardened subprocess tier) without reviving `GitError`, so the enum has no future caller.

**Files:**
- Delete: `src/utils/git_error.rs`, `src/utils/hooks.rs`
- Modify: `src/utils/mod.rs`
- Modify: `src/core/workspace.rs`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing. This is subtraction.

- [ ] **Step 1: Confirm they are actually unreferenced**

```bash
grep -rn "git_error\|GitError" src/ tests/ crates/ | grep -v "^src/utils/git_error.rs"
```
Expected: no output.

```bash
grep -rn "hooks::" src/ tests/ crates/ | grep -v "^src/utils/hooks.rs"
```
Expected: no output.

```bash
grep -rn "build_workspace_model\|WorkspaceModel" src/ tests/ crates/ | grep -v "^src/core/workspace.rs"
```
Expected: no output.

If any command produces output, that item is not dead — stop, and remove only the ones that are.

- [ ] **Step 2: Delete**

```bash
git rm src/utils/git_error.rs src/utils/hooks.rs
```

Remove `pub mod git_error;` and `pub mod hooks;` from `src/utils/mod.rs`.

In `src/core/workspace.rs`, delete `build_workspace_model`, the `WorkspaceModel` type, and any `#[allow(dead_code)]` that existed only for them. If the file is left empty, `git rm` it and remove its `pub mod workspace;` line from `src/core/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `just format && just format-check && just lint && just test`
Expected: PASS with zero warnings. A clippy dead-code warning here means something else was only reachable through what you deleted — delete that too, or restore if it turns out to be live.

- [ ] **Step 4: Update AGENTS.md**

Delete the entire "Known dead/aspirational code — don't build on these without reviving them" section, including its heading.

- [ ] **Step 5: Commit**

```bash
git add -A src/ AGENTS.md
git commit -m "refactor: delete the unreferenced git_error, hooks and workspace-model code"
```

---

## Self-Review

**Spec coverage.** Every recommendation from the analysis maps to a task: refname firewall → Task 1; hitch.json as untrusted input → Task 2; end-of-options → Task 3; config quarantine → Task 4; choke-point enforcement → Task 5; resolution attestation → Task 6; documented merge-driver gap → Task 7; ref transactions → Task 8; atomic publish with `prev` archive → Task 9; bounded archive refs → Task 10; journal generalized to the push → Task 11; crash-fuzz harness → Task 12; dead-code removal → Task 13. Two ideas from the analysis are deliberately **not** planned: the full waybill planner (`Slot` indirection, `Op` enum, dry-run) because with an atomic publish there are two remaining effects and a plan representation for two steps is abstraction ahead of need; and the gitoxide in-process merge kernel, because it would discard the differential-test guarantee that is the codebase's strongest existing correctness property.

**Type consistency.** `PendingResync` is renamed to `PublishRecord` in Task 11 only, and every task before it uses the old name; tasks 9 and 11 use `record_blob` with the signature declared in Task 9's Interfaces block. `RefEdit`'s three variants are used identically in Tasks 8 and 9. `parse_untrusted_config` is defined in Task 2 and used nowhere else. `validate_name`'s signature is unchanged throughout.

**Ordering constraint.** Task 4 must land before Task 5 (Task 5 folds call sites onto `git_command`, which Task 4 introduces). Task 8 must land before Task 9. Task 9 must land before Tasks 10 and 11. Task 12 must land after Task 11. Task 13 is independent and can land at any point after Task 5.
