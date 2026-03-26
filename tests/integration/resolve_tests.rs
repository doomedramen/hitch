//! Integration tests for `hitch resolve` command
//!
//! These tests exercise the conflict-resolution flow: when a rebuild squash-merge
//! conflicts, hitch saves state to `.git/hitch-resolve-state.json` and leaves the
//! user on the temporary rebuild branch.  The tests verify that:
//!
//!  * `hitch resolve`           — shows conflict status
//!  * `hitch resolve --abort`   — cleans up and returns to original branch
//!  * `hitch resolve --continue`— commits the resolution and finishes the rebuild

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Helper: inject two conflicting branches into hitch.json on hitch-metadata
    /// without going through `hitch promote` (which would block them via the
    /// pre-promote conflict check).
    fn inject_branches_into_metadata(
        env: &TestEnvironment,
        env_name: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        // Switch to the metadata branch, edit, commit, and switch back
        env.git.run(&["checkout", "hitch-metadata"])?;

        let config_str = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&config_str)?;

        let branch_array = serde_json::Value::Array(
            branches
                .iter()
                .map(|b| serde_json::Value::String(b.to_string()))
                .collect(),
        );
        config["environments"][env_name]["branches"] = branch_array;

        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: inject conflicting branches"])?;
        env.git.run(&["checkout", "main"])?;

        Ok(())
    }

    /// Set up two branches that produce a conflict during rebuild.
    ///
    /// Returns the test environment ready for `hitch rebuild dev`.
    fn setup_conflicting_rebuild(env: &TestEnvironment) -> anyhow::Result<()> {
        // Add dev environment
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        // Create shared.txt on main (common ancestor)
        env.fs.write_file("shared.txt", "base content\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "Add shared.txt"])?;

        // branch-a: replaces shared.txt with its own content
        env.git.run(&["checkout", "-b", "branch-a"])?;
        env.fs.write_file("shared.txt", "from branch-a\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git
            .run(&["commit", "-m", "branch-a: update shared.txt"])?;
        env.git.run(&["checkout", "main"])?;

        // branch-b: replaces shared.txt with incompatible content
        env.git.run(&["checkout", "-b", "branch-b"])?;
        env.fs.write_file("shared.txt", "from branch-b\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git
            .run(&["commit", "-m", "branch-b: update shared.txt"])?;
        env.git.run(&["checkout", "main"])?;

        // Inject both branches into hitch metadata, bypassing the pre-promote check
        inject_branches_into_metadata(env, "dev", &["branch-a", "branch-b"])?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // hitch resolve (no flags) — status display
    // -------------------------------------------------------------------------

    #[test]
    fn test_resolve_status_shows_conflict_info() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflicting_rebuild(env)?;

            // Trigger rebuild — should fail due to conflict
            let rebuild_result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            rebuild_result.assert_failure();

            // .git/hitch-resolve-state.json must exist
            let resolve_state = env.temp_dir.join(".git/hitch-resolve-state.json");
            assert!(
                resolve_state.exists(),
                "resolve state file should exist after a conflicting rebuild"
            );

            // `hitch resolve` (no flags) should succeed and show useful info
            let status_result = env
                .hitch
                .run()
                .args(&["--no-push", "resolve"])
                .execute()?;
            status_result
                .assert_success()
                .assert_stdout_contains("paused due to a merge conflict");

            // Cleanup so the test doesn't leave a dangling temp branch
            env.hitch
                .run()
                .args(&["--no-push", "resolve", "--abort"])
                .execute()?
                .assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // hitch resolve --abort
    // -------------------------------------------------------------------------

    #[test]
    fn test_resolve_abort_cleans_up() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflicting_rebuild(env)?;

            // Trigger rebuild — should fail due to conflict
            let rebuild_result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            rebuild_result
                .assert_failure()
                .assert_stderr_contains("hitch resolve");

            // Verify we are on the temp branch
            let branch_out = env.git.run(&["branch", "--show-current"])?;
            let current_branch = branch_out.stdout().trim().to_string();
            assert!(
                current_branch.starts_with("hitch-tmp-"),
                "expected to be on a hitch-tmp-* branch, got '{}'",
                current_branch
            );

            // Abort
            env.hitch
                .run()
                .args(&["--no-push", "resolve", "--abort"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("aborted");

            // Should be back on main
            let branch_out = env.git.run(&["branch", "--show-current"])?;
            assert_eq!(
                branch_out.stdout().trim(),
                "main",
                "expected to be back on main after abort"
            );

            // Resolve state file must be gone
            let resolve_state = env.temp_dir.join(".git/hitch-resolve-state.json");
            assert!(
                !resolve_state.exists(),
                "resolve state file should be removed after --abort"
            );

            // Temp branch must be gone
            let branches = env.git.run(&["branch", "--list", "hitch-tmp-*"])?;
            assert!(
                branches.stdout().trim().is_empty(),
                "hitch-tmp-* branch should be deleted after --abort"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // hitch resolve --continue
    // -------------------------------------------------------------------------

    #[test]
    fn test_resolve_continue_finishes_rebuild() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflicting_rebuild(env)?;

            // Trigger rebuild — should fail due to conflict
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_failure();

            // We are now on the hitch-tmp-* branch with conflict markers in shared.txt.
            // Resolve by writing the final content over the file.
            env.fs.write_file("shared.txt", "resolved content\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;

            // Continue
            let continue_result = env
                .hitch
                .run()
                .args(&["--no-push", "resolve", "--continue"])
                .execute()?;
            continue_result
                .assert_success()
                .assert_stdout_contains("rebuilt successfully");

            // Should be back on main
            let branch_out = env.git.run(&["branch", "--show-current"])?;
            assert_eq!(
                branch_out.stdout().trim(),
                "main",
                "expected to be back on main after --continue"
            );

            // Resolve state file must be gone
            let resolve_state = env.temp_dir.join(".git/hitch-resolve-state.json");
            assert!(
                !resolve_state.exists(),
                "resolve state file should be removed after --continue"
            );

            // dev branch must exist (rebuild completed)
            let branches = env.git.run(&["branch", "--list", "dev"])?;
            assert!(
                !branches.stdout().trim().is_empty(),
                "dev branch should exist after successful --continue"
            );

            // Temp branch must be gone
            let branches = env.git.run(&["branch", "--list", "hitch-tmp-*"])?;
            assert!(
                branches.stdout().trim().is_empty(),
                "hitch-tmp-* branch should be deleted after --continue"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Error cases
    // -------------------------------------------------------------------------

    #[test]
    fn test_resolve_fails_when_no_state_file() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // No rebuild in progress — resolve should report no conflict
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "resolve"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("No conflict in progress");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_resolve_continue_fails_with_unresolved_markers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflicting_rebuild(env)?;

            // Trigger rebuild — should fail
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_failure();

            // Do NOT resolve the file — attempt --continue with conflict markers still present
            // Stage the file WITH conflict markers still in it
            env.git.run(&["add", "-f", "shared.txt"])?;

            let continue_result = env
                .hitch
                .run()
                .args(&["--no-push", "resolve", "--continue"])
                .execute()?;
            continue_result
                .assert_failure()
                .assert_stderr_contains("conflict markers");

            // Cleanup
            env.hitch
                .run()
                .args(&["--no-push", "resolve", "--abort"])
                .execute()?
                .assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
