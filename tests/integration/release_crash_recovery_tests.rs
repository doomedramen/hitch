//! Crash-fuzz for `hitch release`, mirroring `crash_recovery_tests.rs`'s
//! differential pattern now that `release` publishes through the same
//! `publish_branch` core as `rebuild`.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// One environment with a promoted branch, ready to release to `main`.
    fn setup(env: &TestEnvironment) -> anyhow::Result<()> {
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

        // Stand on 'main' before releasing, so the resync path has real work.
        env.git.run(&["checkout", "main"])?.assert_success();
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

    /// A release aborted after each step, then re-run, must land on the same
    /// `main` content as a release that was never interrupted, and must leave
    /// no journal record behind.
    #[test]
    fn test_release_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;
                env.hitch
                    .run()
                    .args(&["release", "dev"])
                    .execute()?
                    .assert_success();
                tree = tree_oid(env, "main")?;
                Ok::<(), anyhow::Error>(())
            });
            tree
        };
        assert!(!expected_tree.is_empty(), "the oracle run produced no tree");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;

                let interrupted = env
                    .hitch
                    .run()
                    .args(&["release", "dev"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute()?;
                assert!(
                    !interrupted.success(),
                    "aborting after '{}' should have crashed hitch release, but it exited \
                     successfully.\nstdout: {}\nstderr: {}",
                    abort_after,
                    interrupted.stdout(),
                    interrupted.stderr()
                );

                // Recovery needs --force: `release` (non-force path) locks
                // the environment via `with_locked_env` before it ever
                // reaches a `maybe_abort_for_test` point, and aborting skips
                // that closure's unlock-on-exit entirely — same gotcha as
                // `rebuild`, documented in AGENTS.md. Unlike `rebuild`
                // though, `release`'s `--force` bypasses both the lock
                // *and* the "environment is locked" precondition check in
                // one flag (see `validate_preconditions` /
                // `perform_release_core` in `src/commands/release.rs`), so
                // there is no separate `unlock` step needed — a plain
                // `hitch unlock dev` followed by unforced `release` would
                // also work, but `--force` is release's own documented
                // recovery convention and is what this test exercises.
                //
                // Unlike a crashed `rebuild`, retrying a crashed `release`
                // re-runs `perform_release_core` from scratch, which creates
                // a *new* release tag stamped with the current wall-clock
                // second (`create_tag_at` is a hard `git tag -a`, which
                // fails outright if the name already exists — there is no
                // unconditional-overwrite convention here the way there is
                // for `refs/hitch/prev`/`backup`, see AGENTS.md). On a fast
                // machine the interrupted attempt and this recovery retry
                // land in the same second, so retrying immediately
                // reproducibly collides on the tag name and fails again —
                // unrelated to the publish-journal machinery this test
                // exercises. Sleep past the second boundary so recovery
                // exercises what this test actually cares about instead of
                // tripping over that unrelated, pre-existing tag-naming
                // limitation.
                std::thread::sleep(std::time::Duration::from_millis(1100));

                env.hitch
                    .run()
                    .args(&["release", "dev", "--force"])
                    .execute()?
                    .assert_success();

                let actual_tree = tree_oid(env, "main")?;
                assert_eq!(
                    actual_tree, expected_tree,
                    "release aborted after '{}' did not converge to the oracle's tree",
                    abort_after
                );

                let journal = env
                    .git
                    .run(&["for-each-ref", "refs/hitch/publish/main"])?
                    .stdout()
                    .trim()
                    .to_string();
                assert!(
                    journal.is_empty(),
                    "a publish-journal record for 'main' survived recovery after abort \
                     at '{}'",
                    abort_after
                );

                Ok::<(), anyhow::Error>(())
            });
        }

        Ok(())
    }
}
