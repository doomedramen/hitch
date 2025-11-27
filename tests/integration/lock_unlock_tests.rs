//! Integration tests for hitch lock/unlock commands

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::types::HitchConfig;

    #[test]
    fn test_hitch_lock_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Lock the environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully locked environment 'dev'");

            // Verify environment is locked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            assert!(dev_env.locked_by.is_some());
            assert!(dev_env.locked_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
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

            // Verify it's locked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully unlocked environment 'dev'");

            // Verify environment is unlocked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());
            assert!(dev_env.locked_by.is_none());
            assert!(dev_env.locked_at.is_none());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to lock without initializing hitch
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Failed to read hitch.json");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to unlock without initializing hitch
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Failed to read hitch.json");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Try to lock nonexistent environment
            let result = env.hitch.run().args(&["lock", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Try to unlock nonexistent environment
            let result = env.hitch.run().args(&["unlock", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_already_locked() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
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

            // Try to lock already locked environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("already locked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_already_unlocked() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment (but don't lock it)
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to unlock already unlocked environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result.assert_failure().assert_stderr_contains("not locked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Lock all environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["lock", env_name]).execute()?;
                result.assert_success().assert_stdout_contains(&format!(
                    "Successfully locked environment '{}'",
                    env_name
                ));
            }

            // Verify all environments are locked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(env.is_locked());
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environments, and lock them
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
                env.hitch
                    .run()
                    .args(&["lock", env_name])
                    .execute()?
                    .assert_success();
            }

            // Verify all are locked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(env.is_locked());
            }

            // Unlock all environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["unlock", env_name]).execute()?;
                result.assert_success().assert_stdout_contains(&format!(
                    "Successfully unlocked environment '{}'",
                    env_name
                ));
            }

            // Verify all environments are unlocked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(!env.is_locked());
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_with_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote feature branches
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

            // Lock environment with promoted branches
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully locked environment 'dev'");

            // Verify environment is locked and branches are preserved
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            assert_eq!(dev_env.branches.len(), 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch.run().args(&["init"]).execute()?.assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Initial state: unlocked
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());

            // Lock the environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result.assert_success();

            // Verify locked state
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            let lock_time = dev_env.locked_at.unwrap();

            // Wait a moment to ensure different timestamp
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result.assert_success();

            // Verify unlocked state
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());
            assert!(dev_env.locked_by.is_none());
            assert!(dev_env.locked_at.is_none());

            // Lock again to ensure it can be re-locked
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result.assert_success();

            // Verify new lock time
            let config: HitchConfig = env.fs.read_json("hitch.json")?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            let new_lock_time = dev_env.locked_at.unwrap();
            assert!(new_lock_time > lock_time);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_prevents_operations() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
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

            // Try to promote branch to locked environment (should fail)
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
            result.assert_failure(); // Promotion should auto-unlock and succeed

            // Try to rebuild locked environment (should fail)
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("is locked")
                .assert_stderr_contains("--force");

            // Try to remove locked environment (should fail)
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
    fn test_hitch_unlock_allows_operations() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
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

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result.assert_success();

            // Now operations should succeed
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
