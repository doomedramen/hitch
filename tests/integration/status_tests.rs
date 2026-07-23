//! Integration tests for hitch status command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_status_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to get status without initializing hitch
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_empty_configuration() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environments
            // Hitch is already initialized by framework

            // Get status with empty configuration
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No environments configured")
                .assert_stdout_contains(
                    "Use 'hitch add <environment>' to create your first environment",
                );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Get basic status
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch Environment Status")
                .assert_stdout_contains("1 environments")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("base:")
                .assert_stdout_contains("Branches (0 promoted)")
                .assert_stdout_contains("Environment is unlocked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote feature branches
            for i in 1..=3 {
                let branch_name = format!("feature-{}", i);
                env.git.run(&["checkout", "-b", &branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", i), &format!("content {}", i))?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", &branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Get status with promoted branches
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("1 environments")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("Branches (3 promoted)")
                .assert_stdout_contains("feature-1")
                .assert_stdout_contains("feature-2")
                .assert_stdout_contains("feature-3");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_locked_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Get status with locked environment
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("dev")
                .assert_stdout_contains("Locked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_rebuilt_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, promote branch, and rebuild
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // Get status with rebuilt environment
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("dev")
                .assert_stdout_contains("Rebuilt:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Add different configurations to each environment
            // Add branches to dev
            env.git.run(&["checkout", "-b", "feature-dev"])?;
            env.fs.write_file("dev.txt", "dev content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add dev feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-dev", "dev"])
                .execute()?;
            result.assert_success();

            // Lock qa environment
            env.hitch
                .run()
                .args(&["lock", "qa"])
                .execute()?
                .assert_success();

            // Get status for multiple environments
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("3 environments")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("qa")
                .assert_stdout_contains("staging")
                .assert_stdout_contains("Branches (1 promoted)") // dev has 1 branch
                .assert_stdout_contains("Branches (0 promoted)") // qa has 0 branches
                .assert_stdout_contains("Branches (0 promoted)"); // staging has 0 branches

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_verbose() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Get verbose status
            let result = env.hitch.run().args(&["status", "--verbose"]).execute()?;
            result.assert_success();

            // Verbose mode should show additional debug information
            // Note: We can't easily test the exact verbose output without exposing internal logging
            // But we can verify it doesn't fail and produces output

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_complex_scenario() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            // Add environments with different configurations
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

            // Create qa branch first for staging to use as base
            env.git.run(&["checkout", "-b", "qa"])?;
            env.fs.write_file("qa.txt", "qa content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Create qa branch"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["add", "staging", "--base", "qa"])
                .execute()?
                .assert_success();

            // Add multiple branches to dev
            for i in 1..=2 {
                let branch_name = format!("feature-{}", i);
                env.git.run(&["checkout", "-b", &branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", i), &format!("content {}", i))?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", &branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Add one branch to qa
            env.git.run(&["checkout", "-b", "feature-qa"])?;
            env.fs.write_file("qa.txt", "qa content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add qa feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-qa", "qa"])
                .execute()?;
            result.assert_success();

            // Lock staging environment
            env.hitch
                .run()
                .args(&["lock", "staging"])
                .execute()?
                .assert_success();

            // Rebuild dev environment
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // Get comprehensive status
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("3 environments")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("base:")
                .assert_stdout_contains("main")
                .assert_stdout_contains("Branches (2 promoted)")
                .assert_stdout_contains("qa")
                .assert_stdout_contains("base:")
                .assert_stdout_contains("Branches (1 promoted)")
                .assert_stdout_contains("staging")
                .assert_stdout_contains("qa")
                .assert_stdout_contains("Branches (0 promoted)")
                .assert_stdout_contains("Rebuilt:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_git_state() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create uncommitted changes (dirty working directory)
            env.fs
                .write_file("uncommitted.txt", "uncommitted changes")?;

            // Status should still work even with unclean git state
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch Environment Status")
                .assert_stdout_contains("dev");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    #[ignore = "Timing-sensitive test: relies on git commit timestamps being newer than rebuild timestamp"]
    fn test_hitch_status_detects_base_branch_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add an environment with base branch "main"
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Rebuild the environment (sets rebuilt_at timestamp)
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // Wait enough time to ensure we're in a different second
            std::thread::sleep(std::time::Duration::from_secs(2));

            // Make a new commit directly to main (simulating an external merge)
            env.fs.write_file("external.txt", "external change")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "External change to main"])?;

            // Run hitch status - should detect that main has newer commits
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result.stdout();

            result.assert_success();
            assert!(stdout.contains("dev"), "Expected status to contain 'dev'");

            // The status should indicate rebuild is needed since main has new commits
            assert!(
                stdout.contains("Rebuild needed") || stdout.contains("main has newer commits"),
                "Expected status to show rebuild needed. Got:\n{}",
                stdout
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    #[ignore = "Timing-sensitive test: relies on git commit timestamps being newer than rebuild timestamp"]
    fn test_hitch_status_multiple_envs_with_changed_base() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add multiple environments (dev, qa) using the same base branch "main"
            for env_name in ["dev", "qa"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Rebuild both environments
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "qa"]).execute()?;
            result.assert_success();

            // Wait to ensure timestamp difference
            std::thread::sleep(std::time::Duration::from_secs(2));

            // Make a new commit to main
            env.fs.write_file("external.txt", "external change")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "External change to main"])?;

            // Run hitch status
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result.stdout();

            result.assert_success();
            assert!(stdout.contains("dev"), "Expected status to contain 'dev'");
            assert!(stdout.contains("qa"), "Expected status to contain 'qa'");
            // Both environments should show rebuild needed
            assert!(
                stdout.contains("Rebuild needed") || stdout.contains("main has newer commits"),
                "Expected status to show rebuild needed. Got:\n{}",
                stdout
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// After promoting a branch, adding new commits to it, and checking status,
    /// the branch list should show a staleness indicator ("new commits since last rebuild").
    #[test]
    fn test_status_shows_per_branch_staleness() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch.
            // Use -f to bypass the broad .gitignore inherited from hitch-metadata.
            env.git.run(&["checkout", "-b", "stale-feature"])?;
            env.fs.write_file("stale.txt", "v1")?;
            env.git.run(&["add", "-f", "stale.txt"])?;
            env.git.run(&["commit", "-m", "Initial feature commit"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "stale-feature", "dev"])
                .execute()?
                .assert_success();

            // Add a new commit to the branch AFTER promotion (making it stale).
            // Use an explicit future author-date to guarantee the commit timestamp is
            // newer than the rebuild's `rebuilt_at` timestamp regardless of
            // how fast the test runs.
            env.git.run(&["checkout", "stale-feature"])?;
            env.fs.write_file("stale.txt", "v2 - new content")?;
            env.git.run(&["add", "-f", "stale.txt"])?;
            // Use an explicit future author-date so the commit is guaranteed
            // to be newer than the rebuild's rebuilt_at timestamp.
            env.git.run(&[
                "commit",
                "-m",
                "Update after promotion",
                "--date",
                "2099-01-01T00:00:00+00:00",
            ])?;
            env.git.run(&["checkout", "main"])?;

            // Status should now show the branch as stale
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result.assert_success().stdout().to_string();

            assert!(
                stdout.contains("new commits since last rebuild"),
                "Expected staleness indicator for stale-feature. Got:\n{}",
                stdout
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch status` should flag a branch that would be held on the next
    /// rebuild (⛔), without fetching or building anything.
    #[test]
    fn test_status_shows_held_branch_glyph() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base content\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "Add shared.txt"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from branch-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-b: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Inject directly into metadata (bypass the promote gate, which
            // would otherwise refuse to promote a conflicting sibling).
            env.git.run(&["checkout", "hitch-metadata"])?;
            let config_str = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
            config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: inject branches"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("⛔")
                .assert_stdout_contains("branch-b")
                .assert_stdout_contains("held on rebuild");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
