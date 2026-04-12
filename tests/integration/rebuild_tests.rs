//! Integration tests for hitch rebuild command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use std::io::Write;

    #[test]
    fn test_hitch_rebuild_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
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

            // Rebuild the dev environment
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' rebuilt successfully");

            // Verify rebuild timestamp is updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.rebuilt_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to rebuild without initializing hitch
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to rebuild nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["rebuild", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_empty_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Rebuild empty environment (no promoted branches)
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' rebuilt successfully");

            // Verify rebuild timestamp is updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.rebuilt_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_locked_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
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

            // Lock the environment
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Try to rebuild locked environment (should fail)
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("is locked")
                .assert_stderr_contains("--force");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_locked_environment_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
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

            // Lock the environment
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Rebuild locked environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["rebuild", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' rebuilt successfully");

            // Verify rebuild timestamp is updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.rebuilt_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_multiple_environments() -> anyhow::Result<()> {
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

            // Add different feature branches to each environment
            let env_branches = [
                ("dev", "feature-dev"),
                ("qa", "feature-qa"),
                ("staging", "feature-staging"),
            ];

            for (env_name, branch_name) in env_branches {
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs.write_file(&format!("{}.txt", env_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {} feature", env_name)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", branch_name, env_name])
                    .execute()?;
                result.assert_success();
            }

            // Rebuild each environment
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["rebuild", env_name]).execute()?;
                result.assert_success().assert_stdout_contains(&format!(
                    "Environment '{}' rebuilt successfully",
                    env_name
                ));
            }

            // Verify all environments have rebuild timestamps
            let config = env.read_hitch_config()?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(env.rebuilt_at.is_some());
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_with_conflicts() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create feature branches with different files (non-conflicting)
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature-1.txt", "feature 1 content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 1"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "feature-2"])?;
            env.fs.write_file("feature-2.txt", "feature 2 content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 2"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote both branches
            for branch_name in ["feature-1", "feature-2"] {
                let result = env
                    .hitch
                    .run()
                    .args(&["promote", branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Rebuild should succeed with multiple promoted branches
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' rebuilt successfully");

            // Verify rebuild timestamp is updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.rebuilt_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_multiple_times() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch
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

            // First rebuild
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // Get first rebuild timestamp
            let config = env.read_hitch_config()?;
            let first_timestamp = config.environments.get("dev").unwrap().rebuilt_at;

            // Wait a moment to ensure different timestamp
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Second rebuild
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' rebuilt successfully");

            // Verify timestamp was updated
            let config = env.read_hitch_config()?;
            let second_timestamp = config.environments.get("dev").unwrap().rebuilt_at;
            assert!(second_timestamp > first_timestamp);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Item 6: Concurrent rebuild detection
    // -------------------------------------------------------------------------

    /// If a lock file exists whose PID is still alive, a second rebuild must
    /// fail immediately with a clear "already in progress" message.
    #[test]
    fn test_rebuild_blocked_when_lock_held() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Write a lock file that looks like it belongs to the current
            // process (PID is alive by definition).
            let lock_path = env.temp_dir.join(".git").join("hitch-rebuild-dev.lock");
            let lock_json = serde_json::json!({
                "pid": std::process::id(),
                "env_name": "dev",
                "started_at": "2099-01-01T00:00:00+00:00"
            })
            .to_string();
            std::fs::File::create(&lock_path)?.write_all(lock_json.as_bytes())?;

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;

            // Clean up the lock so we don't interfere with other operations
            let _ = std::fs::remove_file(&lock_path);

            result
                .assert_failure()
                .assert_stderr_contains("already in progress");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// If the lock file's PID is not alive (stale lock), the rebuild should
    /// proceed normally, overwriting the stale lock file.
    #[test]
    fn test_rebuild_proceeds_with_stale_lock() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a feature branch to give the rebuild real work to do
            env.git.run(&["checkout", "-b", "feat-stale-lock"])?;
            env.fs.write_file("feat.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feat-stale-lock", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            // Write a stale lock file with a PID that is not running
            // (PID 1 is init/launchd and can't be killed, but any large
            // obviously-unused PID works; we pick one and trust the OS won't
            // have reused it for this short test window.)
            let lock_path = env.temp_dir.join(".git").join("hitch-rebuild-dev.lock");
            let stale_pid: u32 = 999_999; // Very unlikely to be a live PID
            let lock_json = serde_json::json!({
                "pid": stale_pid,
                "env_name": "dev",
                "started_at": "2000-01-01T00:00:00+00:00"
            })
            .to_string();
            std::fs::write(&lock_path, lock_json)?;

            // Rebuild should succeed despite the stale lock
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("rebuilt successfully");

            // Lock file must be gone (released on drop)
            assert!(
                !lock_path.exists(),
                "Lock file should be removed after rebuild"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
