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
                .assert_stderr_contains("Failed to read hitch.json");

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
                .args(&["add", "staging", "--source", "qa"])
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
}
