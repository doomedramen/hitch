//! Integration tests for hitch rebuild command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// A path next to the test repository rather than inside it. Worktrees
    /// created inside the repo appear as untracked content in its own
    /// `git status`, which is not how anyone actually uses them.
    fn sibling_path(env: &TestEnvironment, name: &str) -> std::path::PathBuf {
        let repo_name = env
            .temp_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());
        env.temp_dir
            .parent()
            .expect("test repo has no parent directory")
            .join(format!("{}-{}", repo_name, name))
    }

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

    /// Set up two branches promoted to `dev` where the second conflicts with
    /// the first: both modify `shared.txt` incompatibly, after diverging from
    /// a common `main`. Shared by the eject-default and halt-override tests
    /// below.
    fn setup_two_conflicting_branches(env: &TestEnvironment) -> anyhow::Result<()> {
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

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_ejects_conflicting_branch_by_default() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_conflicting_branches(env)?;

            // Default policy is eject: the rebuild succeeds, excluding only
            // the conflicting branch, instead of blocking on it.
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            // Exit code 2: succeeded, but held a conflicting branch — not a
            // plain 0 success, and not a failure either.
            result
                .assert_exit_code(2)
                .assert_stdout_contains("held")
                // branch-a composes cleanly first, so branch-b's conflict is
                // attributed to branch-a (the branch it actually collides
                // with), not to main.
                .assert_stdout_contains("branch-b conflicts with branch-a")
                .assert_stdout_contains("shared.txt");

            // dev was built from branch-a alone
            let dev_content = env.git.run(&["show", "dev:shared.txt"])?;
            assert_eq!(dev_content.stdout().trim(), "from branch-a");

            // No hitch-tmp-* branch leaked
            let branches = env.git.run(&["branch", "--list", "hitch-tmp-*"])?;
            assert!(
                branches.stdout().trim().is_empty(),
                "expected no hitch-tmp-* branches, got '{}'",
                branches.stdout().trim()
            );

            // No worktree leaked
            let worktrees = env.git.run(&["worktree", "list"])?;
            assert_eq!(
                worktrees.stdout().lines().count(),
                1,
                "expected only the main worktree, got:\n{}",
                worktrees.stdout()
            );

            // User remains on main
            let branch_out = env.git.run(&["branch", "--show-current"])?;
            assert_eq!(branch_out.stdout().trim(), "main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_dry_run_reports_held_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_conflicting_branches(env)?;

            let result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev", "--dry-run"])
                .execute()?;
            result
                .assert_exit_code(2)
                .assert_stdout_contains("branch-b conflicts with branch-a")
                .assert_stdout_contains("would rebuild with 1 of 2 branches (1 held)");

            // Dry run must not build or publish anything
            let dev_exists = env
                .git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/dev"])?
                .success();
            assert!(!dev_exists, "dry-run must not create the 'dev' branch");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_rebuild_on_conflict_halt_flag_restores_all_or_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_conflicting_branches(env)?;

            // --on-conflict halt overrides the eject default: the rebuild
            // refuses entirely, before creating any temp branch or worktree,
            // exactly like the original all-or-nothing behavior.
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev", "--on-conflict", "halt"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Cannot rebuild 'dev' — compatibility check failed")
                .assert_stderr_contains("branch-b conflicts with branch-a")
                .assert_stderr_contains("shared.txt");

            // No hitch-tmp-* branch created
            let branches = env.git.run(&["branch", "--list", "hitch-tmp-*"])?;
            assert!(
                branches.stdout().trim().is_empty(),
                "expected no hitch-tmp-* branches, got '{}'",
                branches.stdout().trim()
            );

            // dev was never built
            let dev_exists = env
                .git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/dev"])?
                .success();
            assert!(
                !dev_exists,
                "halted rebuild must not create the 'dev' branch"
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

    /// Rebuilding while standing on the environment branch itself must leave
    /// the checkout matching the rebuilt ref, not showing the whole rebuild as
    /// uncommitted reverse changes.
    #[test]
    fn test_hitch_rebuild_resyncs_checked_out_environment_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature.txt"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // Stand on the environment branch, then rebuild it.
            env.git.run(&["checkout", "dev"])?.assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let status = env.git.run(&["status", "--porcelain"])?;
            assert!(
                status.stdout().trim().is_empty(),
                "rebuild left the checked-out environment branch desynchronized: '{}'",
                status.stdout().trim()
            );
            assert!(
                env.fs.file_exists("feature.txt"),
                "promoted file is missing from the working tree after rebuild"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The environment branch can be attached in a *linked worktree* rather
    /// than the main checkout. `get_current_branch()` cannot see that, so this
    /// desynchronization used to be permanent and invisible.
    #[test]
    fn test_hitch_rebuild_resyncs_linked_worktree_on_environment_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature.txt"])?;
            env.git.run(&["checkout", "-b", "feature-2"])?;
            env.fs.write_file("feature2.txt", "feature 2 content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature2.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // First promotion creates the 'dev' branch...
            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // ...which the *user* then checks out in their own linked worktree.
            // Sibling of the repo, not inside it — a nested worktree would
            // show up as untracked content in the repo's own `git status`.
            let wt_path = sibling_path(env, "user-worktree");
            let wt_path_str = wt_path.to_string_lossy().to_string();
            env.git
                .run(&["worktree", "add", &wt_path_str, "dev"])?
                .assert_success();

            // The second promotion rebuilds 'dev' underneath that worktree.
            env.hitch
                .run()
                .args(&["promote", "feature-2", "dev"])
                .execute()?
                .assert_success();

            assert!(
                wt_path.join("feature2.txt").exists(),
                "linked worktree on 'dev' was not updated to the rebuilt branch"
            );

            let wt_git = GitCommandRunner::new(&wt_path)?;
            let status = wt_git.run(&["status", "--porcelain"])?;
            assert!(
                status.stdout().trim().is_empty(),
                "linked worktree left desynchronized after rebuild: '{}'",
                status.stdout().trim()
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
