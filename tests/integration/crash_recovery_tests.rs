//! Crash-fuzz: interrupt a publish at each step and assert that the next
//! hitch invocation converges to the same state an uninterrupted run reaches.
//!
//! This is a differential test in the same spirit as the merge-tree/real-merge
//! comparison: the uninterrupted run is the oracle, and every interruption
//! point must agree with it.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Build the same scenario every time: one environment, one promoted
    /// branch, ready to rebuild.
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
        Ok(())
    }

    /// The tree a branch points at, not the commit SHA. Every rebuild
    /// recomposes from the declaration and creates a fresh `commit-tree` with
    /// the ambient wall-clock time as its author/committer date (hitch sets
    /// no `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`, and the harness doesn't
    /// either) — so two rebuilds of *identical* inputs, run a second apart,
    /// legitimately produce different commit OIDs. That's expected,
    /// content-addressed-tree behaviour, not a recovery bug, and comparing
    /// commit SHAs across separate process invocations (let alone separate
    /// temp repos, as the oracle comparison below does) would make this test
    /// flaky/failing for a reason that has nothing to do with crash recovery.
    /// The tree, by contrast, is pure content and timestamp-independent, so
    /// it is the right notion of "converged to the same state".
    fn tree_oid(env: &TestEnvironment, branch: &str) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&["rev-parse", &format!("refs/heads/{}^{{tree}}", branch)])?
            .stdout()
            .trim()
            .to_string())
    }

    /// A rebuild aborted after each step, then re-run, must land on the same
    /// dev content as a rebuild that was never interrupted, and must leave no
    /// journal record behind.
    #[test]
    fn test_publish_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        // The oracle: an uninterrupted run.
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;
                env.hitch
                    .run()
                    .args(&["rebuild", "dev"])
                    .execute()?
                    .assert_success();
                tree = tree_oid(env, "dev")?;
                Ok::<(), anyhow::Error>(())
            });
            tree
        };

        assert!(!expected_tree.is_empty(), "the oracle run produced no tree");

        for abort_after in ["journal-written", "ref-moved", "resync-done"] {
            let framework = HitchTestFramework::new()?;

            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup(env)?;

                // Interrupted run: expected to die, not to succeed.
                let _ = env
                    .hitch
                    .run()
                    .args(&["rebuild", "dev"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute();

                // Recovery runs at the start of any mutating command. `--force`
                // is needed here for a reason that has nothing to do with the
                // publish journal: `rebuild` locks the environment (a
                // separate, human-facing "don't touch this env" mechanism —
                // see AGENTS.md) before it ever reaches a `maybe_abort_for_test`
                // point, and aborting skips `with_locked_env`'s unlock-on-exit
                // entirely, so every one of these abort points also leaves the
                // environment locked. That's expected given how `--force`
                // bypasses locking rather than clearing it (see the module doc
                // comment on this file for the follow-up this surfaced).
                env.hitch
                    .run()
                    .args(&["rebuild", "dev", "--force"])
                    .execute()?
                    .assert_success();

                let tree = tree_oid(env, "dev")?;
                assert_eq!(
                    tree, expected_tree,
                    "aborting after '{}' did not converge to the uninterrupted content",
                    abort_after
                );

                let leftovers = env
                    .git
                    .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                    .stdout();
                assert!(
                    leftovers.trim().is_empty(),
                    "aborting after '{}' left a journal record behind:\n{}",
                    abort_after,
                    leftovers
                );

                Ok::<(), anyhow::Error>(())
            });
        }

        Ok(())
    }

    /// A path next to the test repository rather than inside it, mirroring
    /// `trust_boundary_tests.rs::sibling_path` — a bare "remote" living inside
    /// `env.temp_dir` would show up as untracked content in the repo's own
    /// `git status` and trip hitch's working-tree-clean guard.
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

    /// The fourth abort point, `push-succeeded`, is a genuinely new crash
    /// window: Task 11 taught the publish journal about an owed push, which
    /// means there is now a window where the push has actually landed on the
    /// remote but the journal hasn't been told yet — between
    /// `force_push_with_deploy_key_if_configured` returning `Ok(())` and
    /// `mark_push_done`/`clear` running. None of the other three abort points
    /// exercise this because none of them involve a real push succeeding.
    ///
    /// To exercise it for real (not just simulate it), this pushes to a local
    /// bare repository standing in for `origin` — the same technique
    /// `trust_boundary_tests.rs` uses for its deploy-key push tests. No
    /// `hitch setup` deploy key is configured, so
    /// `force_push_with_deploy_key_if_configured` takes its fallback branch: a
    /// plain `git push --force-with-lease origin <branch>`, which is a real,
    /// verifiable push against the bare repo below.
    #[test]
    fn test_publish_converges_after_abort_at_push_succeeded() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup(env)?;

            let bare_path = sibling_path(env, "bare-origin.git");
            std::fs::create_dir_all(&bare_path)?;
            // Test-only: sets up a scratch bare remote to stand in for origin,
            // so it deliberately spawns plain git rather than going through
            // GitOperations (which has no repo at this path).
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

            // `run_raw()` does not inject `--no-push`, so this is a genuine
            // push attempt against the bare repo above — expected to die
            // right after that push lands, but before the journal is told.
            let _ = env
                .hitch
                .run_raw()
                .args(&["rebuild", "dev"])
                .env("HITCH_TEST_ABORT_AFTER", "push-succeeded")
                .execute();

            // Confirm the push actually reached the remote before the
            // process died — otherwise this test would not be exercising the
            // crash window it claims to.
            let remote_tip = env
                .git
                .run(&[
                    format!("--git-dir={}", bare_path.to_string_lossy()).as_str(),
                    "rev-parse",
                    "refs/heads/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            let local_tip_after_abort = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            assert!(
                !remote_tip.is_empty(),
                "the bare remote has no 'dev' branch — the push never landed, so this test \
                 isn't exercising the crash window it claims to"
            );
            assert_eq!(
                remote_tip, local_tip_after_abort,
                "the remote tip must match the local tip after a successful push"
            );

            // The journal record must still be here, with push_owed still
            // true — that is exactly the state this abort point should leave
            // behind (the push landed, but the journal wasn't told yet).
            let leftovers_before_recovery = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                .stdout();
            assert!(
                !leftovers_before_recovery.trim().is_empty(),
                "aborting after 'push-succeeded' should leave a journal record behind, found none"
            );

            // Recovery runs at the start of any mutating command. `--force` is
            // needed for the same reason documented on the loop test above:
            // aborting mid-publish always skips `with_locked_env`'s
            // unlock-on-exit, regardless of which step it happened at.
            //
            // Capture its output: does it report the "ahead of origin"
            // warning even though the push already landed? `recover()`
            // unconditionally warns whenever `push_owed` is still set on the
            // record it finds, with no way to know the push actually
            // succeeded — this is the finding this test exists to surface,
            // not something to paper over.
            let recovery = env
                .hitch
                .run()
                .args(&["rebuild", "dev", "--force"])
                .execute()?;

            let recovery_stdout = recovery.stdout();
            let recovery_stderr = recovery.stderr();
            let misleading_warning_present = recovery_stdout.contains("is ahead of origin")
                || recovery_stderr.contains("is ahead of origin");
            eprintln!(
                "push-succeeded recovery stdout:\n{}\npush-succeeded recovery stderr:\n{}\n\
                 misleading 'ahead of origin' warning present: {}",
                recovery_stdout, recovery_stderr, misleading_warning_present
            );
            recovery.assert_success();

            // Convergence: since this "next invocation" is itself a full
            // `hitch rebuild`, it recomposes and republishes regardless of
            // recovery (see `tree_oid`'s doc comment on why that means a
            // fresh commit, not necessarily the same commit SHA, even though
            // nothing about the declared inputs changed). What must hold is
            // that the resulting content is unchanged, and that no journal
            // record is left behind (recovery unconditionally deletes any
            // record it processes, regardless of `push_owed`'s value).
            let tree_after_recovery = tree_oid(env, "dev")?;
            let tree_after_abort = env
                .git
                .run(&["rev-parse", &format!("{}^{{tree}}", local_tip_after_abort)])?
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                tree_after_recovery, tree_after_abort,
                "recovery must not have changed dev's content after 'push-succeeded'"
            );

            let leftovers_after = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/publish"])?
                .stdout();
            assert!(
                leftovers_after.trim().is_empty(),
                "aborting after 'push-succeeded' left a journal record behind after recovery:\n{}",
                leftovers_after
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
