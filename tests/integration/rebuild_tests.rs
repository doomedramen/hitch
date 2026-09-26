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

    /// Test-only: sets up a scratch bare remote to stand in for origin and
    /// wires the test repo's `origin` at it, mirroring
    /// `push_tests.rs::init_bare_origin`. The bare repo goes beside the test
    /// repo, not inside it, for the same reason `sibling_path` exists.
    /// Deliberately spawns plain git — `GitOperations` has no repository
    /// handle at this path yet.
    fn init_bare_origin(env: &TestEnvironment, name: &str) -> anyhow::Result<std::path::PathBuf> {
        let bare_path = sibling_path(env, name);
        std::fs::create_dir_all(&bare_path)?;
        #[allow(clippy::disallowed_methods)]
        let init = std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&bare_path)
            .stdin(std::process::Stdio::null())
            .output()?;
        assert!(init.status.success(), "failed to init bare origin repo");

        env.git
            .run(&["remote", "add", "origin", &bare_path.to_string_lossy()])?
            .assert_success();

        Ok(bare_path)
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
                .assert_stdout_has_line("✓ dev   realised");

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
                .assert_stdout_has_line("✓ dev   realised");

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
                .assert_stdout_has_line("✓ dev   realised");

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
                result
                    .assert_success()
                    .assert_stdout_has_line(&format!("✓ {env_name}   realised"));
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
                // branch-a composes cleanly first, so branch-b's conflict is
                // attributed to branch-a (the branch it actually collides
                // with), not to main.
                .assert_stdout_contains("branch-b held — conflicts with branch-a")
                .assert_stdout_contains("shared.txt")
                // The remedy is the part that has to survive a reword of the
                // prose around it: a held branch with no way to fix it is a dead
                // end the user has to go find `git rebase` documentation for.
                .assert_stdout_contains("fix: git checkout branch-b && git rebase branch-a");

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

    /// The load-bearing invariant of phase P1: `--dry-run` is a *preview of the
    /// real build*, not a second approximation of it.
    ///
    /// It used to be the latter. The dry-run asked
    /// `preflight_compatibility_report` — a tree-based loop over
    /// `merge-tree --write-tree-name-only` with a hand-passed `--merge-base` —
    /// while the build asked `merge_tree_compose`, a commit-based loop over the
    /// ORT merge with no explicit merge-base. Two different doors into the
    /// merge engine, so two different verdicts were possible, and the dry-run
    /// was structurally blind to recorded resolutions. Both paths now run the
    /// same `compose_environment` over the same pinned SHAs.
    ///
    /// This checks the **verdict**, not the prose: for each path, which branch
    /// was held and against which neighbour, plus how many. Deliberately
    /// tolerant of wording, because the two paths legitimately render the same
    /// event differently (the real build additionally emits compose's
    /// per-branch `⛔ Held …` warning, which a preview that never composed
    /// would not have). A test that compared rendered lines would fail on
    /// cosmetics while staying silent about an actual verdict divergence — the
    /// bug this phase exists to prevent. The sharper case, where the two paths
    /// disagreed about *whether* replay resolves a conflict, is covered by
    /// `resolve_tests::test_dry_run_agrees_with_real_build_about_replayed_resolutions`.
    #[test]
    fn test_dry_run_and_real_build_agree_on_held_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_conflicting_branches(env)?;

            /// Which branch each output reports as held, against which
            /// neighbour. Scans every conflict-ish line rather than the first,
            /// so a path that prints both a live warning and a summary report
            /// still yields a single unambiguous verdict.
            fn held_verdict(output: &str) -> Vec<(String, String)> {
                let mut found = Vec::new();
                for line in output.lines() {
                    if !(line.contains("conflicts with") || line.contains("Held '")) {
                        continue;
                    }
                    let names: Vec<&str> = line
                        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                        .filter(|t| t.starts_with("branch-"))
                        .collect();
                    if names.len() >= 2 {
                        let pair = (names[0].to_string(), names[1].to_string());
                        if !found.contains(&pair) {
                            found.push(pair);
                        }
                    }
                }
                found
            }

            let dry = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev", "--dry-run"])
                .execute()?;
            let dry_stdout = dry.stdout();
            // Exit 2 = "would hold", per the CI contract.
            dry.assert_exit_code(2)
                .assert_stdout_contains("branch-b held — conflicts with branch-a");

            let real = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            let real_stdout = real.stdout();
            real.assert_exit_code(2)
                .assert_stdout_contains("branch-b held — conflicts with branch-a");

            let dry_verdict = held_verdict(&dry_stdout);
            let real_verdict = held_verdict(&real_stdout);
            assert_eq!(
                dry_verdict,
                vec![("branch-b".to_string(), "branch-a".to_string())],
                "the dry-run must report branch-b held against branch-a"
            );
            assert_eq!(
                dry_verdict, real_verdict,
                "the dry-run and the real build must reach the same verdict about which \
                 branch is held and against which neighbour"
            );

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
                .assert_stdout_contains("branch-b held — conflicts with branch-a");

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
                .assert_stdout_has_line("✓ dev   realised");

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
                .assert_stdout_has_line("✓ dev   realised");

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
                .assert_stdout_has_line("✓ dev   realised");

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

    /// Simulate a publish that died between moving the branch ref and updating
    /// the checkout standing on it: move the ref by hand and leave a
    /// pending-resync record behind. The next mutating hitch command must
    /// finish the job.
    fn stage_interrupted_publish(
        env: &TestEnvironment,
        branch: &str,
        from_sha: &str,
        to_sha: &str,
    ) -> anyhow::Result<()> {
        let record = serde_json::json!({
            "branch": branch,
            "from_sha": from_sha,
            "to_sha": to_sha,
            "checkouts": [env.temp_dir.to_string_lossy()],
        });
        let record_path = env.temp_dir.join("pending-record.json");
        std::fs::write(&record_path, serde_json::to_vec_pretty(&record)?)?;

        let blob = env
            .git
            .run(&["hash-object", "-w", &record_path.to_string_lossy()])?
            .assert_success()
            .stdout()
            .trim()
            .to_string();
        std::fs::remove_file(&record_path)?;

        env.git
            .run(&[
                "update-ref",
                &format!("refs/hitch/pending-resync/{}", branch),
                &blob,
            ])?
            .assert_success();

        // The ref move that the dying process had already completed. Done with
        // update-ref precisely so the working tree is left behind, which is the
        // state being reproduced.
        env.git
            .run(&["update-ref", &format!("refs/heads/{}", branch), to_sha])?
            .assert_success();
        Ok(())
    }

    #[test]
    fn test_interrupted_publish_is_finished_by_the_next_command() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("late.txt", "landed late")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add late.txt"])?;
            let advanced = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();

            env.git.run(&["checkout", "main"])?;
            let original = env
                .git
                .run(&["rev-parse", "main"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();

            stage_interrupted_publish(env, "main", &original, &advanced)?;

            // Exactly the reported symptom: HEAD moved, the tree did not.
            assert!(!env.fs.file_exists("late.txt"));
            let before = env.git.run(&["status", "--porcelain"])?;
            assert!(
                !before.stdout().trim().is_empty(),
                "expected the staged interruption to leave a desynchronized tree"
            );

            // Any mutating command picks the obligation back up.
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            let after = env.git.run(&["status", "--porcelain"])?;
            assert!(
                after.stdout().trim().is_empty(),
                "interrupted publish was not finished: '{}'",
                after.stdout().trim()
            );
            assert!(
                env.fs.file_exists("late.txt"),
                "recovery did not bring the working tree up to the published tip"
            );

            let leftover = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/hitch/pending-resync",
                ])?
                .stdout();
            assert!(
                leftover.trim().is_empty(),
                "pending-resync record survived recovery: '{}'",
                leftover.trim()
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Recovery must prove a tree is stale before touching it. A tree that has
    /// been edited since the interruption is left exactly as the user left it.
    #[test]
    fn test_interrupted_publish_recovery_refuses_to_touch_an_edited_tree() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("late.txt", "landed late")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add late.txt"])?;
            let advanced = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();

            env.git.run(&["checkout", "main"])?;
            let original = env
                .git
                .run(&["rev-parse", "main"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();

            stage_interrupted_publish(env, "main", &original, &advanced)?;

            // The user got back to their desk and started working.
            env.fs.write_file("my-work.txt", "do not eat this")?;

            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            let result = result.assert_success();

            assert_eq!(
                env.fs.read_file("my-work.txt")?,
                "do not eat this",
                "recovery destroyed work that appeared after the interruption"
            );
            assert!(
                result.stdout().contains("interrupted") || result.stderr().contains("interrupted"),
                "recovery skipped an edited tree without telling the user"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Every rebuild must leave behind the tip it replaced, under
    /// refs/hitch/prev/<env>/<timestamp> — that ref is what makes rollback a
    /// one-ref flip instead of an archaeology exercise in the reflog.
    #[test]
    fn test_rebuild_archives_previous_tip_under_prev_ref() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git
                .run(&["checkout", "-b", "feature-1"])?
                .assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature 1"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let first_tip = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();

            // A second rebuild replaces that tip, so it must be archived.
            env.git
                .run(&["checkout", "-b", "feature-2"])?
                .assert_success();
            env.fs.write_file("2.txt", "two")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature 2"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-2", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let prev_refs = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/hitch/prev/dev",
                ])?
                .stdout();

            assert!(
                prev_refs.lines().any(|line| line.trim() == first_tip),
                "expected the replaced tip {} under refs/hitch/prev/dev, got:\n{}",
                first_tip,
                prev_refs
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A publish that has not pushed yet owes a push, and that obligation must
    /// be written down — otherwise a process killed between the ref move and
    /// the push leaves the local branch ahead of origin with nothing recording
    /// why.
    #[test]
    fn test_publish_journal_records_push_obligation() -> anyhow::Result<()> {
        use hitch::utils::publish_journal::PublishRecord;

        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git
                .run(&["checkout", "-b", "feature-1"])?
                .assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature 1"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // `run()` already defaults to `--no-push` (and `--yes`), so the
            // obligation is deliberately not incurred here, and a completed
            // rebuild must leave no journal record at all.
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let leftovers = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                .stdout();
            assert!(
                leftovers.trim().is_empty(),
                "a completed publish left a journal record behind:\n{}",
                leftovers
            );

            // The record type must be able to express the obligation.
            let record = PublishRecord {
                branch: "dev".to_string(),
                from_sha: None,
                to_sha: "0".repeat(40),
                checkouts: vec![],
                push_owed: true,
                ..Default::default()
            };
            assert!(record.push_owed);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch rebuild <env> --no-push` still publishes locally and still owes
    /// the remote nothing. Two pieces of *hitch's own* bookkeeping are
    /// load-bearing on exactly that path, and neither is asserted anywhere in
    /// the suite:
    ///
    ///  1. `record_pushed_tip` (`prelude.rs:865`) advances
    ///     `refs/remotes/origin/<branch>` by hand, because hitch's deploy-key
    ///     pushes go to an explicit SSH URL and therefore bypass git's own
    ///     remote-tracking update. Both of its call sites are gated on the
    ///     push succeeding (`prelude.rs:851`, `:893`), so a `--no-push`
    ///     rebuild must leave the tracking ref at the last commit that was
    ///     *actually* pushed. If it advanced anyway, `git status` would
    ///     report a never-pushed branch as in sync with origin.
    ///  2. The transient `refs/hitch/build/<env>/<ts>` anchor, which exists
    ///     only to keep the composed commit reachable until the publish CAS
    ///     lands, is dropped again. `refs/hitch/build` has **zero** assertions
    ///     in `tests/`, despite AGENTS.md making that ordering load-bearing —
    ///     a leak would accumulate an unreachable commit on every rebuild.
    ///
    /// Two neighbouring claims are already covered and deliberately not
    /// re-tested here: that the journal record is cleared
    /// (`test_publish_journal_records_push_obligation`, above — the harness
    /// injects `--no-push` by default, which is exactly the path it checks),
    /// and that the remote branch itself is untouched
    /// (`push_tests::test_hitch_push_force_succeeds_against_existing_remote_branch`
    /// stages this same sequence and then force-pushes over it).
    #[test]
    fn test_rebuild_with_no_push_leaves_hitch_push_bookkeeping_alone() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            init_bare_origin(env, "bare-origin.git")?;

            env.hitch
                .run_raw()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // A real push, so the remote-tracking ref has something truthful
            // to be stale against. `run_raw` is required here: `run()` would
            // inject --no-push and there would be no push to observe.
            env.hitch
                .run_raw()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let pushed_tip = env
                .git
                .run(&["rev-parse", "--verify", "refs/remotes/origin/dev"])?
                .stdout()
                .trim()
                .to_string();
            let local_tip_at_push = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                pushed_tip, local_tip_at_push,
                "the first rebuild pushed, so both refs should name the same commit"
            );

            // Promote without rebuilding, so the only rebuild in this test is
            // the `--no-push` one under test.
            env.git
                .run(&["checkout", "-b", "feature-1"])?
                .assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature 1"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            env.hitch
                .run_raw()
                .args(&["rebuild", "dev", "--no-push"])
                .execute()?
                .assert_success();

            // The local branch really did move. Without this, both assertions
            // below would pass for the wrong reason — a rebuild that did
            // nothing would also leave the tracking ref and the anchor alone.
            let local_tip_after = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            assert_ne!(
                local_tip_after, pushed_tip,
                "the --no-push rebuild must still publish locally"
            );

            // 1. hitch's own remote-tracking bookkeeping did not run.
            let tracking_after = env
                .git
                .run(&["rev-parse", "--verify", "refs/remotes/origin/dev"])?
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                tracking_after, pushed_tip,
                "--no-push must not advance refs/remotes/origin/dev to a commit \
                 that was never pushed"
            );

            // 2. The transient build anchor is gone. Matched over the whole
            // namespace rather than a literal path, because the ref name is
            // `refs/hitch/build/<env>/<timestamp>` (`prelude.rs:795`) — a
            // literal `refs/hitch/build/dev` would never match and the
            // assertion would pass vacuously.
            //
            // Verified non-vacuous by hand on 2026-09-25: a clean rebuild
            // leaves the namespace empty, while aborting mid-publish
            // (HITCH_TEST_ABORT_AFTER=journal-written, which fires inside the
            // window between create at `prelude.rs:796` and drop at `:814`)
            // leaves `refs/hitch/build/dev/20260925160909` behind. So this
            // namespace is genuinely populated during a build, and its
            // emptiness afterwards is a real assertion.
            let build_refs = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/build"])?
                .stdout();
            assert!(
                build_refs.trim().is_empty(),
                "rebuild left a build anchor behind:\n{}",
                build_refs
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Desired and Actual must disagree, and the record must say so.
    ///
    /// A held branch is the case the Desired/Actual split exists for: it *was*
    /// promoted, it is *not* in what you are deploying, and the difference
    /// needs to be legible rather than inferred. The fixture is
    /// `setup_two_conflicting_branches` — the same one
    /// `test_hitch_rebuild_ejects_conflicting_branch_by_default` uses,
    /// deliberately, so this assertion keeps running against a real eject
    /// rather than a purpose-built approximation of one.
    #[test]
    fn test_rebuild_records_held_branches_separately_from_included() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_conflicting_branches(env)?;

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);

            let record = read_state_record(env, "dev")?
                .expect("a rebuild that ejects a branch still published, and must record it");

            let desired: Vec<&str> = record["desired_branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["branch"].as_str().unwrap())
                .collect();
            assert_eq!(
                desired,
                vec!["branch-a", "branch-b"],
                "both branches were promoted, so both are desired — a record that \
                 omitted the held one would erase the reason for the hold"
            );

            let included: Vec<&str> = record["included_branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["branch"].as_str().unwrap())
                .collect();
            assert_eq!(
                included,
                vec!["branch-a"],
                "the ejected branch must not appear in what was actually built"
            );

            let held = record["held"].as_array().unwrap();
            assert_eq!(held.len(), 1, "expected exactly one held branch: {held:?}");
            assert_eq!(held[0]["branch"], "branch-b");
            assert_eq!(
                held[0]["conflicts_with"], "branch-a",
                "branch-b conflicts with the branch it actually collided with, \
                 not with the base"
            );
            assert_eq!(
                held[0]["conflicted_files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap())
                    .collect::<Vec<_>>(),
                vec!["shared.txt"]
            );

            // A hold is a *successful* publish, so the record must be
            // describing the tip that actually exists — not an aborted or
            // half-applied state.
            assert_eq!(
                record["result_sha"].as_str().unwrap(),
                rev_parse(env, "dev")?.as_str(),
                "a held branch must not make the record stale: the branch moved \
                 and the record describes where it moved to"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The record is a snapshot of a build, not a live view of intent.
    ///
    /// After a promotion with `--no-rebuild`, `dev` still holds the old build
    /// and the record still correctly describes it — while the *declaration*
    /// now names a branch the build never saw. That gap is the whole subject
    /// of P3. P2's obligation is narrower and this test pins it: the record
    /// must not silently update itself to describe a build that did not
    /// happen, and must not lie about the one that did.
    #[test]
    fn test_a_promotion_after_a_rebuild_leaves_the_record_describing_the_old_build(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            for name in ["feature-a", "feature-b"] {
                env.git.run(&["checkout", "-b", name])?.assert_success();
                env.fs.write_file(&format!("{name}.txt"), name)?;
                env.git.run(&["add", "."])?.assert_success();
                env.git.run(&["commit", "-m", name])?.assert_success();
                env.git.run(&["checkout", "main"])?.assert_success();
            }

            inject_branches_into_metadata(env, "dev", &["feature-a"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let tip_at_build = rev_parse(env, "dev")?;
            let before = read_state_record(env, "dev")?.expect("the rebuild recorded");
            assert_eq!(
                before["result_sha"].as_str().unwrap(),
                tip_at_build.as_str()
            );

            // Widen the declaration without rebuilding.
            inject_branches_into_metadata(env, "dev", &["feature-a", "feature-b"])?;

            let after = read_state_record(env, "dev")?
                .expect("a promotion with no rebuild must not remove the record");
            let names: Vec<&str> = after["desired_branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["branch"].as_str().unwrap())
                .collect();
            assert_eq!(
                names,
                vec!["feature-a"],
                "the record describes the build that ran, not the declaration as \
                 it stands now — otherwise it would claim feature-b was built"
            );
            assert_eq!(
                after["result_sha"].as_str().unwrap(),
                rev_parse(env, "dev")?.as_str(),
                "and it must still match the branch, because nothing rebuilt"
            );
            assert_eq!(after["result_sha"].as_str().unwrap(), tip_at_build.as_str());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Read the build record at `refs/hitch/state/<env>`, or `None` if there
    /// is no such ref. Goes through plain git rather than hitch's own reader
    /// on purpose: the record's *storage* is part of what is under test, and a
    /// helper that read it back the way `build_record::read_state` does would
    /// happily agree with a writer that put the wrong thing there.
    fn read_state_record(
        env: &TestEnvironment,
        env_name: &str,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let listed = env
            .git
            .run(&[
                "for-each-ref",
                "--format=%(objectname)",
                &format!("refs/hitch/state/{env_name}"),
            ])?
            .stdout();
        let oids: Vec<&str> = listed
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        match oids.as_slice() {
            [] => Ok(None),
            [one] => {
                let blob = env.git.run(&["cat-file", "-p", one])?.stdout();
                Ok(Some(serde_json::from_str(&blob)?))
            }
            many => anyhow::bail!(
                "refs/hitch/state/{env_name} must name exactly one live record, \
                 but {} refs matched: {many:?}",
                many.len()
            ),
        }
    }

    fn rev_parse(env: &TestEnvironment, rev: &str) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&["rev-parse", rev])?
            .stdout()
            .trim()
            .to_string())
    }

    /// Two clean, non-overlapping feature branches declared in a known
    /// promotion order, so the record's `desired_branches` has a *sequence* to
    /// get right rather than just a set.
    fn setup_two_clean_branches(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        for (name, body) in [("feature-a", "a"), ("feature-b", "b")] {
            env.git.run(&["checkout", "-b", name])?.assert_success();
            env.fs.write_file(&format!("{name}.txt"), body)?;
            env.git.run(&["add", "."])?.assert_success();
            env.git.run(&["commit", "-m", name])?.assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();
        }

        inject_branches_into_metadata(env, "dev", &["feature-a", "feature-b"])
    }

    /// The record of what hitch actually built, as a fact rather than a
    /// guess.
    ///
    /// Before this existed, `hitch status` inferred an environment branch's
    /// contents by comparing *commit timestamps* against a *wall-clock* rebuild
    /// time — wrong for every rebased or cherry-picked branch, for any
    /// skewed-clock commit, and silently so: it just reports "up to date" when
    /// it is not. The record replaces that inference with a written fact.
    #[test]
    fn test_rebuild_writes_a_build_record_matching_the_published_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_clean_branches(env)?;

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let dev_tip = rev_parse(env, "dev")?;
            let record = read_state_record(env, "dev")?
                .expect("a rebuild must leave a build record at refs/hitch/state/dev");

            assert_eq!(record["schema_version"], 1);
            assert_eq!(record["environment"], "dev");
            assert_eq!(record["base_name"], "main");
            assert_eq!(
                record["base_sha"].as_str().unwrap(),
                rev_parse(env, "main")?.as_str(),
                "base_sha must name the exact commit composed from, not a name"
            );

            // `metadata_sha` is deliberately *not* asserted equal to the
            // `hitch-metadata` tip. It cannot be: a rebuild brackets the
            // commit it records on both sides with its own metadata writes.
            // `with_locked_env` commits the lock before the declaration is
            // read, and publishing commits the `rebuilt_at` stamp and the
            // unlock after — so the recorded SHA is a commit that only ever
            // existed as a transient branch tip, observable from nowhere
            // outside the command. All that is assertable is that it is a
            // commit on that branch's history and is not the final tip. (It
            // is also the reason this field is not the staleness signal: see
            // `build_record::EnvironmentBuildRecord::metadata_sha`.)
            let metadata_sha = record["metadata_sha"].as_str().unwrap();
            assert_eq!(
                metadata_sha.len(),
                40,
                "expected a full SHA, got {metadata_sha:?}"
            );
            env.git
                .run(&["cat-file", "-e", &format!("{metadata_sha}^{{commit}}")])?
                .assert_success();
            let metadata_tip = rev_parse(env, "hitch-metadata")?;
            assert_ne!(
                metadata_sha, metadata_tip,
                "the recorded SHA is the pre-stamp commit, so it cannot be the tip"
            );
            env.git
                .run(&[
                    "merge-base",
                    "--is-ancestor",
                    metadata_sha,
                    "refs/heads/hitch-metadata",
                ])?
                .assert_success();
            assert!(
                record["hitch_version"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty()),
                "hitch_version must say which hitch wrote this"
            );
            assert_eq!(
                record["result_sha"].as_str().unwrap(),
                dev_tip.as_str(),
                "result_sha must be the commit the branch actually has"
            );
            assert!(
                record["held"].as_array().unwrap().is_empty(),
                "two non-overlapping branches cannot conflict: {:?}",
                record["held"]
            );
            assert!(record["replayed_resolutions"]
                .as_array()
                .unwrap()
                .is_empty());

            // Asserted as a *sequence*: composition walks declared branches in
            // order, so the record's order is semantic. A set comparison would
            // pass on a record that had sorted them.
            let desired = record["desired_branches"].as_array().unwrap();
            assert_eq!(
                desired
                    .iter()
                    .map(|b| (b["branch"].as_str().unwrap(), b["sha"].as_str().unwrap()))
                    .collect::<Vec<_>>(),
                vec![
                    ("feature-a", rev_parse(env, "feature-a")?.as_str()),
                    ("feature-b", rev_parse(env, "feature-b")?.as_str()),
                ],
                "desired_branches must preserve promotion order and record each \
                 branch's exact consumed commit"
            );
            assert_eq!(
                record["included_branches"], record["desired_branches"],
                "with nothing held, included is exactly desired"
            );

            // Parsed as a real timestamp rather than pattern-matched, so this
            // survives a switch in serialization.
            chrono::DateTime::parse_from_rfc3339(
                record["built_at"]
                    .as_str()
                    .expect("built_at must be a string"),
            )
            .expect("built_at must be an RFC 3339 timestamp");

            // --- Step 4: the record tracks a *second* rebuild. ---
            //
            // This is the assertion that makes the record live rather than
            // write-once. It also exercises the ref edit's
            // `expected_old: Some(String::new())`: `Create` semantics would
            // have failed this batch outright, because refs/hitch/state/dev
            // already exists from the first build.
            env.git.run(&["checkout", "feature-a"])?.assert_success();
            env.fs.write_file("feature-a.txt", "a-advanced")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature-a: advance"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let advanced_a = rev_parse(env, "feature-a")?;
            let record2 = read_state_record(env, "dev")?
                .expect("the live record must still be readable after a second build");

            assert_eq!(
                record2["result_sha"].as_str().unwrap(),
                rev_parse(env, "dev")?.as_str(),
                "the second build's record must describe the new tip"
            );
            assert_eq!(
                record2["desired_branches"][0]["sha"].as_str().unwrap(),
                advanced_a.as_str(),
                "a record left stale from the first build would still name the \
                 old feature-a commit"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The dry-run's non-mutation guarantee, extended to the new ref.
    ///
    /// P1 established that `--dry-run` composes through the same primitive as
    /// the real build while mutating nothing. A record written from the
    /// dry-run path would break that in a way no other test would notice: the
    /// preview would describe a build that had not happened, and the *next*
    /// real rebuild would find a record already there.
    #[test]
    fn test_dry_run_does_not_write_a_build_record() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_two_clean_branches(env)?;

            env.hitch
                .run()
                .args(&["rebuild", "dev", "--dry-run"])
                .execute()?
                .assert_success();

            assert!(
                read_state_record(env, "dev")?.is_none(),
                "--dry-run must not write a record: a preview cannot leave \
                 behind a description of a build that never ran"
            );

            // Non-vacuity: the same command without --dry-run does write one,
            // so the assertion above is about the flag and not about a
            // namespace that is always empty.
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();
            assert!(
                read_state_record(env, "dev")?.is_some(),
                "the real build must write a record — otherwise the check above \
                 would pass for the wrong reason"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `promote` runs its own rebuild, through the same core — so the record
    /// must be current afterwards.
    ///
    /// This is the assertion that catches someone building the record in
    /// `src/commands/rebuild.rs` instead of in `rebuild_environment_opts`, where
    /// `promote`/`demote`/`approve`/post-release-rebuild cannot reach it. That
    /// mistake would look perfectly correct in every rebuild test and be
    /// silently wrong in production: `promote` would move `dev` and leave the
    /// record describing the *previous* build.
    #[test]
    fn test_promote_refreshes_the_build_record() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git
                .run(&["checkout", "-b", "feature-a"])?
                .assert_success();
            env.fs.write_file("feature-a.txt", "a")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature-a"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            // promote rebuilds by default, so this both promotes and builds.
            env.hitch
                .run()
                .args(&["promote", "feature-a", "dev"])
                .execute()?
                .assert_success();

            let after_first =
                read_state_record(env, "dev")?.expect("promote's rebuild must write a record too");
            assert_eq!(after_first["desired_branches"].as_array().unwrap().len(), 1);

            env.git
                .run(&["checkout", "-b", "feature-b"])?
                .assert_success();
            env.fs.write_file("feature-b.txt", "b")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature-b"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature-b", "dev"])
                .execute()?
                .assert_success();

            let after_second = read_state_record(env, "dev")?
                .expect("the live record must survive a second promote");
            let names: Vec<&str> = after_second["desired_branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["branch"].as_str().unwrap())
                .collect();
            assert_eq!(
                names,
                vec!["feature-a", "feature-b"],
                "the record must reflect the declaration promote just wrote"
            );
            assert_eq!(
                after_second["result_sha"].as_str().unwrap(),
                rev_parse(env, "dev")?.as_str(),
                "and must describe the tip promote's rebuild just published, not \
                 the previous one"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
