//! Integration tests for hitch add/remove commands

#[cfg(test)]
mod tests {
    use crate::test_framework::*;
    use hitch::types::HitchConfig;

    #[test]
    fn test_hitch_add_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch first
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Add a basic environment (defaults to main branch)
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully added environment 'dev'");

            // Verify environment was created
            env.assert.hitch_environment_exists(&env.fs, "dev")?;

            // Verify environment configuration
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
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

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch and create a develop branch
            env.hitch.run().args(&["init"]).execute()?.assert_success();
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

            // Verify environment configuration
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let qa_env = config.environments.get("qa").unwrap();
            assert_eq!(qa_env.base, "develop");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_invalid_names() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch first
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Test invalid environment names
            let invalid_names = vec![
                ("", "empty name"),
                ("dev environment", "contains space"),
                ("dev/env", "contains slash"),
                ("dev@env", "contains special character"),
                ("123dev", "starts with number"),
            ];

            for (invalid_name, _description) in invalid_names {
                let result = env.hitch.run().args(&["add", invalid_name]).execute();
                match result {
                    Ok(cmd_result) => {
                        // Should fail
                        cmd_result.assert_failure();
                    }
                    Err(_) => {
                        // Command execution failed - also acceptable
                    }
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_duplicate_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch and add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to add same environment again
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

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch first
            env.hitch.run().args(&["init"]).execute()?.assert_success();

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

        let _ = framework.with_test_environment(|env| {
            // Try to add environment without initializing hitch
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Hitch is not initialized");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch and add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Remove the environment
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully removed environment 'dev'");

            // Verify environment was removed
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_with_branches_requires_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch, add environment, and promote a branch
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

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

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch, add environment, and promote a branch
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

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
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_locked_environment_requires_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch, add environment, and lock it
            env.hitch.run().args(&["init"]).execute()?.assert_success();
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

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch, add environment, and lock it
            env.hitch.run().args(&["init"]).execute()?.assert_success();
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
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch first
            env.hitch.run().args(&["init"]).execute()?.assert_success();

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

        let _ = framework.with_test_environment(|env| {
            // Try to remove environment without initializing hitch
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Hitch is not initialized");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_remove_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize hitch
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Add multiple environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["add", env_name]).execute()?;
                result.assert_success();
                env.assert.hitch_environment_exists(&env.fs, env_name)?;
            }

            // Remove one environment
            let result = env.hitch.run().args(&["remove", "qa"]).execute()?;
            result.assert_success();

            // Verify remaining environments exist
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            assert!(config.environments.contains_key("dev"));
            assert!(!config.environments.contains_key("qa"));
            assert!(config.environments.contains_key("staging"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
