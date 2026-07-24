//! Integration tests for hitch resolve command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Mode A: a branch conflicting with base gets a guided rebase kicked
    /// off (checkout + `git rebase`), which git itself pauses on conflict —
    /// hitch hands off to plain Git rather than inventing its own flow.
    #[test]
    fn test_resolve_mode_a_starts_guided_rebase() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "v1\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "base v1"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from-branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            env.fs.write_file("shared.txt", "from-main-later\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "main: update shared.txt"])?;

            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["resolve", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("durable fix is rebasing")
                .assert_stdout_contains("git rebase --continue");

            // hitch checked out branch-a and started a real rebase, which
            // git itself paused on the conflict.
            assert_eq!(
                env.git.run(&["branch", "--show-current"])?.stdout().trim(),
                ""
            );
            assert!(env.temp_dir.join(".git/rebase-merge").exists());

            // Resolve it with plain git, exactly as hitch told us to.
            env.fs.write_file("shared.txt", "resolved\n")?;
            env.git.run(&["add", "shared.txt"])?.assert_success();
            // -c core.editor=true is belt-and-suspenders: a single-commit
            // --continue already reuses the original message without
            // opening an editor, but this guarantees it can't block on one
            // regardless of platform/config.
            env.git
                .run(&["-c", "core.editor=true", "rebase", "--continue"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            // No resolve worktree was ever created for Mode A.
            let worktrees = env.git.run(&["worktree", "list"])?;
            assert_eq!(worktrees.stdout().lines().count(), 1);

            // Now a plain rebuild picks up the durable fix.
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_success();
            assert_eq!(
                env.git.run(&["show", "dev:shared.txt"])?.stdout().trim(),
                "resolved"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Mode B: a peer-vs-peer conflict gets an isolated worktree with real
    /// conflict markers; the user's own checkout is never touched;
    /// `--continue` publishes the resolved composition; a later plain
    /// rebuild re-holds the branch since nothing was persisted.
    #[test]
    fn test_resolve_mode_b_worktree_continue_publishes_and_cleans_up() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "base"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "a"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "b"])?;
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

            // No --branch needed: exactly one branch is held.
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "resolve", "dev"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Conflicts left in")
                .assert_stdout_contains("--continue");

            // The user's own checkout is untouched.
            assert_eq!(
                env.git.run(&["branch", "--show-current"])?.stdout().trim(),
                "main"
            );
            assert_eq!(env.git.run(&["status", "--porcelain"])?.stdout(), "");

            let worktree_path = env.temp_dir.parent().unwrap().join(format!(
                ".hitch-resolve-{}-dev-branch-b",
                env.temp_dir.file_name().unwrap().to_string_lossy()
            ));
            assert!(worktree_path.exists(), "expected resolve worktree to exist");

            let markers = std::fs::read_to_string(worktree_path.join("shared.txt"))?;
            assert!(markers.contains("<<<<<<<"));

            std::fs::write(worktree_path.join("shared.txt"), "resolved-both\n")?;
            let git_in_worktree = |args: &[&str]| {
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&worktree_path)
                    .status()
            };
            git_in_worktree(&["add", "shared.txt"])?;

            let result = env
                .hitch
                .run()
                .args(&[
                    "--no-push",
                    "resolve",
                    "dev",
                    "--branch",
                    "branch-b",
                    "--continue",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Published 'dev'")
                .assert_stdout_contains("not saved anywhere");

            assert_eq!(
                env.git.run(&["show", "dev:shared.txt"])?.stdout().trim(),
                "resolved-both"
            );

            // Worktree and its temp branch are fully cleaned up.
            let worktrees = env.git.run(&["worktree", "list"])?;
            assert_eq!(worktrees.stdout().lines().count(), 1);
            let branches = env.git.run(&["branch", "--list", "hitch-resolve-*"])?;
            assert!(branches.stdout().trim().is_empty());

            // Nothing was persisted: a plain rebuild re-holds branch-b.
            let result = env
                .hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?;
            result.assert_exit_code(2).assert_stdout_contains("held");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--abort` discards an in-progress Mode B session without publishing
    /// anything or leaving a worktree/branch behind.
    #[test]
    fn test_resolve_mode_b_abort_discards_session() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "base"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "a"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "b"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "hitch-metadata"])?;
            let config_str = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
            config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: inject branches"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["--no-push", "resolve", "dev", "--branch", "branch-b"])
                .execute()?
                .assert_success();

            let result = env
                .hitch
                .run()
                .args(&["resolve", "dev", "--branch", "branch-b", "--abort"])
                .execute()?;
            result.assert_success().assert_stdout_contains("Discarded");

            let worktrees = env.git.run(&["worktree", "list"])?;
            assert_eq!(worktrees.stdout().lines().count(), 1);
            let branches = env.git.run(&["branch", "--list", "hitch-resolve-*"])?;
            assert!(branches.stdout().trim().is_empty());

            // dev was never published by the aborted session.
            let dev_exists = env
                .git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/dev"])?
                .success();
            assert!(!dev_exists);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// With more than one branch held, `hitch resolve <env>` without
    /// `--branch` must ask which one rather than guessing.
    #[test]
    fn test_resolve_requires_branch_when_ambiguous() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "base"])?;
            env.fs.write_file("other.txt", "base\n")?;
            env.git.run(&["add", "-f", "other.txt"])?;
            env.git.run(&["commit", "-m", "base2"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "a"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "b"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-c"])?;
            env.fs.write_file("other.txt", "from-c\n")?;
            env.git.run(&["add", "-f", "other.txt"])?;
            env.git.run(&["commit", "-m", "c"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-d"])?;
            env.fs.write_file("other.txt", "from-d\n")?;
            env.git.run(&["add", "-f", "other.txt"])?;
            env.git.run(&["commit", "-m", "d"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "hitch-metadata"])?;
            let config_str = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
            config["environments"]["dev"]["branches"] =
                serde_json::json!(["branch-a", "branch-b", "branch-c", "branch-d"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: inject branches"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env.hitch.run().args(&["resolve", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Specify which one with --branch");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Set up dev with branch-a and branch-b conflicting on shared.txt, then
    /// record a resolution for branch-b (Mode B --continue --record).
    /// Returns nothing; leaves the recorded ref in place.
    fn setup_and_record(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.fs.write_file("shared.txt", "base\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "base"])?;

        env.git.run(&["checkout", "-b", "branch-a"])?;
        env.fs.write_file("shared.txt", "from-a\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "a"])?;
        env.git.run(&["checkout", "main"])?;

        env.git.run(&["checkout", "-b", "branch-b"])?;
        env.fs.write_file("shared.txt", "from-b\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "b"])?;
        env.git.run(&["checkout", "main"])?;

        env.git.run(&["checkout", "hitch-metadata"])?;
        let config_str = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
        config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git.run(&["commit", "-m", "test: inject branches"])?;
        env.git.run(&["checkout", "main"])?;

        env.hitch
            .run()
            .args(&["--no-push", "resolve", "dev", "--branch", "branch-b"])
            .execute()?
            .assert_success();

        let worktree_path = env.temp_dir.parent().unwrap().join(format!(
            ".hitch-resolve-{}-dev-branch-b",
            env.temp_dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(worktree_path.join("shared.txt"), "resolved-both\n")?;
        std::process::Command::new("git")
            .args(["add", "shared.txt"])
            .current_dir(&worktree_path)
            .status()?;

        env.hitch
            .run()
            .args(&[
                "--no-push",
                "resolve",
                "dev",
                "--branch",
                "branch-b",
                "--continue",
                "--record",
            ])
            .execute()?
            .assert_success();

        Ok(())
    }

    /// A recorded resolution is replayed under `--replay-resolutions` — the
    /// conflicting branch composes instead of being held — but a plain
    /// rebuild still holds it (replay is strictly opt-in).
    #[test]
    fn test_replay_composes_recorded_resolution() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_and_record(env)?;

            // Plain rebuild: branch-b is still held.
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_exit_code(2)
                .assert_stdout_contains("held");

            // With replay (and --yes, since the test is non-interactive):
            // branch-b composes from the recording.
            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "--yes",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                ])
                .execute()?
                .assert_success()
                .assert_stdout_contains("Reused recorded resolution");

            assert_eq!(
                env.git.run(&["show", "dev:shared.txt"])?.stdout().trim(),
                "resolved-both"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Structural staleness: after the recorded branch moves (so the conflict
    /// stage OIDs differ), the exact-match key no longer hits, so replay is a
    /// miss and the branch is held — never a wrong (stale) replay. This is
    /// the red-team-critical property that motivated content-addressed keying
    /// over rerere.
    #[test]
    fn test_replay_is_a_miss_after_branch_moves() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_and_record(env)?;

            // Move branch-b so its side of the conflict has a different blob.
            env.git.run(&["checkout", "branch-b"])?;
            env.fs.write_file("shared.txt", "from-b-CHANGED\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "branch-b moved"])?;
            env.git.run(&["checkout", "main"])?;

            // Even with replay on, the recorded resolution doesn't match the
            // new conflict, so branch-b is held rather than wrongly replayed.
            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "--yes",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                ])
                .execute()?
                .assert_exit_code(2)
                .assert_stdout_contains("held");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
