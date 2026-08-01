//! Crash-fuzz for hitch resolve's Mode A publish, mirroring
//! crash_recovery_tests.rs's differential pattern now that resolve publishes
//! through the same publish_branch core as rebuild/release.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Produce a real Mode A conflict (branch-a vs main on shared.txt),
    /// promote branch-a to dev, pause the guided rebase in its worktree, and
    /// resolve the conflict there with plain git — everything up to but not
    /// including the final 'hitch resolve dev --branch branch-a --continue'
    /// that lands it. Mirrors
    /// resolve_tests.rs::test_resolve_mode_a_rebases_without_touching_the_users_checkout.
    fn setup_paused_rebase(env: &TestEnvironment) -> anyhow::Result<()> {
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

        env.hitch
            .run()
            .args(&["resolve", "dev"])
            .execute()?
            .assert_success();

        let session_path = env
            .hitch
            .run()
            .args(&["resolve", "dev", "--branch", "branch-a", "--path"])
            .execute()?
            .assert_success()
            .stdout()
            .trim()
            .to_string();
        let session = std::path::PathBuf::from(&session_path);
        let session_git = GitCommandRunner::new(&session)?;

        std::fs::write(session.join("shared.txt"), "resolved\n")?;
        session_git.run(&["add", "shared.txt"])?.assert_success();
        session_git
            .run(&["-c", "core.editor=true", "rebase", "--continue"])?
            .assert_success();

        Ok(())
    }

    fn tree_oid(env: &TestEnvironment, branch: &str) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&["rev-parse", &format!("refs/heads/{}^{{tree}}", branch)])?
            .stdout()
            .trim()
            .to_string())
    }

    /// A resolve `--continue` aborted after each publish step, then re-run,
    /// must land on the same `branch-a` content as a resolve that was never
    /// interrupted, and must leave no journal record behind.
    ///
    /// The paused rebase's worktree and its session marker file survive
    /// `std::process::abort` untouched (nothing in `continue_rebase_session`
    /// removes the worktree until `finish_mode_a` returns `Ok`), so re-running
    /// the exact same 'hitch resolve dev --branch branch-a --continue' command
    /// is the real recovery path: `git rebase --continue` already succeeded
    /// before hitch's own publish step ran, so `continue_rebase_session` finds
    /// no rebase in progress and no leftover markers, recomputes the same
    /// `new_sha` from the untouched worktree, and republishes.
    #[test]
    fn test_resolve_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup_paused_rebase(env)?;
                env.hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .execute()?
                    .assert_success();
                tree = tree_oid(env, "branch-a")?;
                Ok::<(), anyhow::Error>(())
            });
            tree
        };
        assert!(!expected_tree.is_empty(), "the oracle run produced no tree");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup_paused_rebase(env)?;

                let interrupted = env
                    .hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute()?;
                assert!(
                    !interrupted.success(),
                    "aborting after '{}' should have crashed hitch resolve --continue, but \
                     it exited successfully.\nstdout: {}\nstderr: {}",
                    abort_after,
                    interrupted.stdout(),
                    interrupted.stderr()
                );

                // Re-running --continue is the documented recovery path: the
                // worktree's rebase already finished (git rebase --continue
                // succeeded before hitch's own publish step ran), so hitch
                // only needs to redo the publish half. Confirmed by reading
                // resolve.rs's continue_rebase_session: nothing tears the
                // worktree or its session marker down until finish_mode_a
                // returns Ok, so a crash mid-publish leaves both exactly as
                // they were and this same command is what finishes the job.
                env.hitch
                    .run()
                    .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                    .execute()?
                    .assert_success();

                let actual_tree = tree_oid(env, "branch-a")?;
                assert_eq!(
                    actual_tree, expected_tree,
                    "resolve aborted after '{}' did not converge to the oracle's tree",
                    abort_after
                );

                let journal = env
                    .git
                    .run(&["for-each-ref", "refs/hitch/publish/branch-a"])?
                    .stdout()
                    .trim()
                    .to_string();
                assert!(
                    journal.is_empty(),
                    "a publish-journal record for 'branch-a' survived recovery after abort \
                     at '{}'",
                    abort_after
                );

                Ok::<(), anyhow::Error>(())
            });
        }

        Ok(())
    }
}
