//! Integration tests for hitch guard command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_guard_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch
            // Hitch is already initialized by framework

            // Create a feature branch (not an environment branch)
            env.git.run(&["checkout", "-b", "feature-1"])?;

            // Run guard command on feature branch - should pass
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed")
                .assert_stdout_contains("feature-1")
                .assert_stdout_contains("is not an environment branch");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            // Try to run guard without initializing hitch
            env.git.run(&["checkout", "-b", "feature-1"])?;

            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Hitch is not initialized");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_with_environment_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to run guard on main branch - should pass (not an environment branch)
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_specific_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environments
            // Hitch is already initialized by framework
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

            // Create a feature branch
            env.git.run(&["checkout", "-b", "feature-1"])?;

            // Run guard command checking specific environment
            let result = env.hitch.run().args(&["guard", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_detached_head() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch
            // Hitch is already initialized by framework

            // Create a commit and checkout detached HEAD
            env.fs.write_file("test.txt", "test content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Test commit"])?;
            env.git.run(&["checkout", "HEAD~1"])?;

            // Try to run guard in detached HEAD state - should fail
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Cannot guard in detached HEAD state");

            // Go back to a branch
            env.git.run(&["checkout", "main"])?;

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_multiple_environments() -> anyhow::Result<()> {
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

            // Create various feature branches and test guard on each
            let feature_branches = ["feature-auth", "feature-ui", "feature-api"];

            for branch_name in feature_branches {
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", branch_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {}", branch_name)])?;

                let result = env.hitch.run().args(&["guard"]).execute()?;
                result
                    .assert_success()
                    .assert_stdout_contains("Guard check passed");

                env.git.run(&["checkout", "main"])?;
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_workflow_prevention() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Simulate a pre-commit hook scenario
            // Create a feature branch and try to guard before committing
            env.git.run(&["checkout", "-b", "feature-secure"])?;

            // Guard should pass on feature branch
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result.assert_success();

            // Commit some changes
            env.fs.write_file("secure.txt", "secure content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add secure feature"])?;

            // Guard should still pass
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_with_complex_branch_names() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch
            // Hitch is already initialized by framework

            // Test with various branch naming patterns
            let branch_names = [
                "feature/user-authentication",
                "bugfix/memory-leak",
                "hotfix/security-patch",
                "refactor/database-optimization",
                "experimental/ai-integration",
            ];

            for branch_name in branch_names {
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", branch_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {}", branch_name)])?;

                // Guard should pass for all non-environment branches
                let result = env.hitch.run().args(&["guard"]).execute()?;
                result
                    .assert_success()
                    .assert_stdout_contains("Guard check passed");

                env.git.run(&["checkout", "main"])?;
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_environment_edge_cases() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment with custom base
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Create environment-like branch names that should still pass guard
            let similar_names = [
                "prod-feature",
                "production-backup",
                "prod-test",
                "production-hotfix",
            ];

            for branch_name in similar_names {
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", branch_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {}", branch_name)])?;

                // Guard should pass - these aren't exact environment names
                let result = env.hitch.run().args(&["guard"]).execute()?;
                result
                    .assert_success()
                    .assert_stdout_contains("Guard check passed");

                env.git.run(&["checkout", "main"])?;
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_empty_repository() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch on empty repository
            // Hitch is already initialized by framework

            // Run guard on main branch (should pass)
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_blocks_environment_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add a dev environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Rebuild to create the dev branch
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            // Checkout the dev environment branch
            env.git.run(&["checkout", "dev"])?;

            // Guard should FAIL because we're on an environment branch
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Cannot commit directly to environment branch")
                .assert_stderr_contains("Environment branches are managed by hitch");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_guard_allows_promoted_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add a dev environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and checkout a feature branch
            env.git.run(&["checkout", "-b", "feature/my-feature"])?;
            env.fs.write_file("feature.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;

            // Promote the feature branch to dev
            env.hitch
                .run()
                .args(&["promote", "feature/my-feature", "dev"])
                .execute()?
                .assert_success();

            // Guard should PASS - we're on a feature branch, even though it's promoted
            let result = env.hitch.run().args(&["guard"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Guard check passed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
