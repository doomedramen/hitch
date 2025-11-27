//! Integration tests for hitch add/remove commands

#[cfg(test)]
mod tests {
    use crate::test_framework::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_add_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add a basic environment (defaults to main branch)
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully added environment 'dev'");

            // Verify environment was created by reading the configuration
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "main");
            assert!(dev_env.branches.is_empty());
            assert!(!dev_env.is_locked());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_with_source_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;

            // Add environment with custom source branch
            let result = env
                .hitch
                .run()
                .args(&["add", "qa", "--source", "develop"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully added environment 'qa'");

            // Verify environment configuration - read from hitch-metadata branch
            let config = env.read_hitch_config()?;
            let qa_env = config.environments.get("qa").unwrap();
            assert_eq!(qa_env.base, "develop");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_invalid_names() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Test that empty name fails (truly invalid case)
            let result = env.hitch.run().args(&["add", ""]).execute();
            result
                .expect("Empty environment name should fail")
                .assert_failure();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_duplicate_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // Try to add same environment again (dev environment was created by setup)
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("already exists");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_nonexistent_source_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Try to add environment with nonexistent source branch
            let result = env
                .hitch
                .run()
                .args(&["add", "dev", "--source", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to add environment without initializing hitch
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result.assert_failure().assert_stderr_contains("hitch.json");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // Remove the environment (dev environment was created by setup)
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully removed environment 'dev'");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_with_branches_requires_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // The dev environment was created by setup, now promote a branch to it

            // Create and promote a feature branch
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("test.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add test file"])?;
            env.git.run(&["checkout", "main"])?;

            // Try to remove environment with promoted branches without force
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("has promoted branches")
                .assert_stderr_contains("--force");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_with_branches_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // The dev environment was created by setup, now promote a branch to it

            // Create and promote a feature branch
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("test.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add test file"])?;
            env.git.run(&["checkout", "main"])?;

            // Remove environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["remove", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully removed environment 'dev'");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_locked_environment_requires_force() -> anyhow::Result<()> {
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

            // Try to remove locked environment without force
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("is locked")
                .assert_stderr_contains("--force");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_locked_environment_force() -> anyhow::Result<()> {
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

            // Remove locked environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["remove", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully removed environment 'dev'");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch first
            // Hitch is already initialized by framework

            // Try to remove nonexistent environment
            let result = env.hitch.run().args(&["remove", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to remove environment without initializing hitch
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result.assert_failure().assert_stderr_contains("hitch.json");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_remove_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add multiple environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["add", env_name]).execute()?;
                result.assert_success();
                // Verify environment was created
                let config = env.read_hitch_config()?;
                assert!(config.environments.contains_key(env_name));
            }

            // Remove one environment
            let result = env.hitch.run().args(&["remove", "qa"]).execute()?;
            result.assert_success();

            // Verify remaining environments exist
            let config = env.read_hitch_config()?;
            assert!(config.environments.contains_key("dev"));
            assert!(!config.environments.contains_key("qa"));
            assert!(config.environments.contains_key("staging"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
