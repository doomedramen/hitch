//! Integration tests for hitch rebuild command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Helper: inject branches into hitch.json on hitch-metadata without using `hitch promote`.
    fn inject_branches_into_metadata(
        env: &TestEnvironment,
        env_name: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
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
            .run(&["commit", "-m", "test: inject branches into metadata"])?;

        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

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
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a base file on main
            env.fs.write_file("shared.txt", "base content\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "Add shared.txt"])?;

            // branch-a modifies shared.txt
            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // branch-b modifies shared.txt in an incompatible way
            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from branch-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-b: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Inject conflicting branches into metadata (bypass promote gating)
            inject_branches_into_metadata(env, "dev", &["branch-a", "branch-b"])?;

            // Rebuild should refuse before creating any temp branch
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Cannot rebuild 'dev' — compatibility check failed")
                .assert_stderr_contains("branch-b conflicts with main")
                .assert_stderr_contains("shared.txt");

            // No hitch-tmp-* branch created
            let branches = env.git.run(&["branch", "--list", "hitch-tmp-*"])?;
            assert!(
                branches.stdout().trim().is_empty(),
                "expected no hitch-tmp-* branches, got '{}'",
                branches.stdout().trim()
            );

            // User remains on main
            let branch_out = env.git.run(&["branch", "--show-current"])?;
            assert_eq!(branch_out.stdout().trim(), "main");

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

    /// If another process is actively holding the per-environment rebuild lock,
    /// a second rebuild must fail immediately with a clear "already in progress"
    /// message. The lock is an advisory `flock`, so we hold a real one from this
    /// process (via the library) while running `hitch rebuild` as a subprocess.
    #[test]
    fn test_rebuild_blocked_when_lock_held() -> anyhow::Result<()> {
        use hitch::utils::rebuild_lock::RebuildLock;

        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Actually hold the rebuild lock for `dev` in this process. Because it
            // is an OS advisory lock, the separate `hitch rebuild` process below
            // will contend with it (writing a lock file would NOT — the file's
            // existence no longer enforces the lock).
            let git_dir = env.temp_dir.join(".git");
            let _held =
                RebuildLock::acquire(&git_dir, "dev").expect("test should hold the rebuild lock");

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;

            result
                .assert_failure()
                .assert_stderr_contains("already in progress");

            // `_held` releases the advisory lock when it drops at end of scope.
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A lock file left behind by a previous (now-dead) process holds no live
    /// advisory lock, so a new rebuild should acquire it and proceed normally.
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

            // Leave behind a lock file from a "previous run". No live process holds
            // an flock on it, so the rebuild must proceed.
            let lock_path = env.temp_dir.join(".git").join("hitch-rebuild-dev.lock");
            let lock_json = serde_json::json!({
                "pid": 999_999,
                "env_name": "dev",
                "started_at": "2000-01-01T00:00:00+00:00"
            })
            .to_string();
            std::fs::write(&lock_path, lock_json)?;

            // Rebuild should succeed despite the leftover lock file
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("rebuilt successfully");

            // With advisory locks the marker file is intentionally left in place
            // (its existence does not hold the lock), so we do not assert removal.
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Regression: a hitch-metadata branch without a `.gitignore` (e.g. a repo
    /// initialized by an older hitch, or one where it was removed) must not break
    /// metadata mutations. Previously `add_and_commit(["hitch.json", ".gitignore"])`
    /// hard-failed on the missing `.gitignore`, leaving hitch.json staged but
    /// uncommitted and stranding the operation on hitch-metadata — so the switch
    /// back to the user's branch aborted with "local changes to hitch.json would
    /// be overwritten by checkout".
    #[test]
    fn test_hitch_rebuild_without_gitignore_on_metadata() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Drop `.gitignore` from the hitch-metadata branch to mimic a repo
            // that never had one committed there.
            env.git.run(&["checkout", "hitch-metadata"])?;
            env.git.run(&["rm", "--quiet", ".gitignore"])?;
            env.git
                .run(&["commit", "-m", "test: drop .gitignore from metadata"])?;
            env.git.run(&["checkout", "main"])?;

            // The rebuild (which locks -> writes metadata -> unlocks) must succeed
            // and return us to `main` with a clean tree.
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("rebuilt successfully");

            let branch = env.git.run(&["branch", "--show-current"])?;
            assert_eq!(
                branch.stdout().trim(),
                "main",
                "expected to be back on main after rebuild"
            );

            let status = env.git.run(&["status", "--porcelain"])?;
            assert!(
                status.stdout().trim().is_empty(),
                "expected a clean working tree after rebuild, got '{}'",
                status.stdout().trim()
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
