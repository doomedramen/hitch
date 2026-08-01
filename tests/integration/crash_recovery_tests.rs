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

    /// Build on `setup`, then promote a *second* branch with `--no-rebuild`
    /// and check the repo out onto `dev` at its pre-second-promotion content.
    /// Returns that `dev` tip — the `from_sha` an interrupted rebuild below
    /// is expected to move away from.
    ///
    /// The second promotion is what makes the checkout attached to `dev`
    /// worth resyncing at all. `setup` alone leaves `dev` already fully
    /// built, so a *second*, uninterrupted `rebuild dev` recomposes
    /// byte-identical content under a fresh commit SHA (see `tree_oid`'s doc
    /// comment) — a checkout standing on it would report "clean" throughout
    /// no matter what recovery does, because the tree never actually
    /// changes. Promoting `feature-2` with `--no-rebuild` means the
    /// interrupted `rebuild dev` call genuinely changes the tree (adds
    /// `2.txt`), so a checkout on `dev` truly diverges between the ref move
    /// and the resync, and `repair_checkout`'s stale-tree `reset_hard_to`
    /// path — not just its "already consistent" no-op — is what actually
    /// gets exercised.
    fn setup_with_pending_second_promotion(env: &TestEnvironment) -> anyhow::Result<String> {
        setup(env)?;

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
            .args(&["promote", "feature-2", "dev", "--no-rebuild"])
            .execute()?
            .assert_success();

        // Stand on 'dev' before the interrupted rebuild advances it, so
        // `scan_checkouts_on_branch("dev")` finds this checkout attached and
        // `repair_checkout` has real work to prove and do.
        env.git.run(&["checkout", "dev"])?.assert_success();

        Ok(env
            .git
            .run(&["rev-parse", "dev"])?
            .stdout()
            .trim()
            .to_string())
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
    ///
    /// Beyond convergence, this also asserts the abort actually landed the
    /// observable state each step name claims *before* recovery gets a
    /// chance to run — otherwise a disabled or no-op `maybe_abort_for_test`
    /// (or a test whose checkout was never attached to `dev` in the first
    /// place, which used to be the case here) could pass this test for the
    /// wrong reason: a normal, uninterrupted rebuild converges trivially.
    #[test]
    fn test_publish_converges_after_abort_at_each_step() -> anyhow::Result<()> {
        // The oracle: an uninterrupted run.
        let expected_tree = {
            let framework = HitchTestFramework::new()?;
            let mut tree = String::new();
            let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
                setup_with_pending_second_promotion(env)?;
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
                let from_sha = setup_with_pending_second_promotion(env)?;

                // Interrupted run: expected to die, not to succeed.
                let interrupted = env
                    .hitch
                    .run()
                    .args(&["rebuild", "dev"])
                    .env("HITCH_TEST_ABORT_AFTER", abort_after)
                    .execute()?;
                assert!(
                    !interrupted.success(),
                    "aborting after '{}' should have crashed the process, but it exited \
                     successfully — the abort hook did not fire.\nstdout: {}\nstderr: {}",
                    abort_after,
                    interrupted.stdout(),
                    interrupted.stderr()
                );

                // Prove the abort actually landed the state its name claims,
                // before recovery runs and (correctly) erases the evidence.
                let dev_after_abort = env
                    .git
                    .run(&["rev-parse", "dev"])?
                    .stdout()
                    .trim()
                    .to_string();
                let journal_oid = env
                    .git
                    .run(&[
                        "for-each-ref",
                        "--format=%(objectname)",
                        "refs/hitch/publish/dev",
                    ])?
                    .stdout()
                    .trim()
                    .to_string();

                match abort_after {
                    "journal-written" => {
                        // This point fires before the atomic ref transaction
                        // that writes both the journal ref and moves 'dev' —
                        // record_blob only hashes the payload into the object
                        // database at this step, it does not point any ref at
                        // it. So neither ref should have moved yet.
                        assert!(
                            journal_oid.is_empty(),
                            "aborting after 'journal-written' should leave no journal ref \
                             yet (it is only written as part of the same atomic transaction \
                             as the branch move, which happens later), found one"
                        );
                        assert_eq!(
                            dev_after_abort, from_sha,
                            "aborting after 'journal-written' should leave 'dev' unmoved"
                        );
                    }
                    "ref-moved" | "resync-done" => {
                        assert!(
                            !journal_oid.is_empty(),
                            "aborting after '{}' should leave a journal record behind, \
                             found none",
                            abort_after
                        );
                        let payload = env.git.run(&["cat-file", "-p", &journal_oid])?.stdout();
                        let record: serde_json::Value = serde_json::from_str(&payload)?;
                        assert_eq!(record["branch"], "dev");
                        assert_eq!(record["to_sha"], dev_after_abort);
                        assert_eq!(record["push_owed"], false);
                        assert_ne!(
                            dev_after_abort, from_sha,
                            "aborting after '{}' should have already moved 'dev'",
                            abort_after
                        );

                        // The whole point of `setup_with_pending_second_promotion`:
                        // 'dev' now composes both branches, but this checkout
                        // was standing on the pre-second-promotion content, so
                        // whether it has caught up is real, observable signal
                        // for whether the resync step ran before the abort.
                        let checkout_caught_up = env.fs.file_exists("2.txt");
                        if abort_after == "ref-moved" {
                            assert!(
                                !checkout_caught_up,
                                "aborting after 'ref-moved' should leave this checkout \
                                 stale — the ref moved but resync hasn't run yet"
                            );
                        } else {
                            assert!(
                                checkout_caught_up,
                                "aborting after 'resync-done' should have already brought \
                                 this checkout up to date, since resync runs before this \
                                 abort point"
                            );
                        }
                    }
                    other => panic!("unexpected abort point '{}'", other),
                }

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
                assert!(
                    env.fs.file_exists("2.txt"),
                    "aborting after '{}': recovery must leave this checkout with the fully \
                     composed content, not just a matching branch tip",
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
            // warning even though the push already landed? `recover()` now
            // checks `refs/remotes/origin/<branch>` against the record's
            // `to_sha` before warning, so it can tell "push already landed"
            // apart from "push genuinely still owed" instead of assuming the
            // worse case unconditionally. This test's job is to prove that
            // distinction actually holds at the one abort point where the
            // push truly did land: the warning must not fire, and the
            // accurate "already completed" message must.
            let recovery = env
                .hitch
                .run()
                .args(&["rebuild", "dev", "--force"])
                .execute()?;

            let recovery_stdout = recovery.stdout();
            let recovery_stderr = recovery.stderr();
            let misleading_warning_present = recovery_stdout.contains("is ahead of origin")
                || recovery_stderr.contains("is ahead of origin");
            assert!(
                !misleading_warning_present,
                "recovery reported the branch as ahead of origin even though the push had \
                 already landed before the crash — recover() should have seen \
                 'refs/remotes/origin/dev' already matching the record's 'to_sha' and skipped \
                 the warning.\nstdout: {}\nstderr: {}",
                recovery_stdout, recovery_stderr
            );
            assert!(
                recovery_stdout.contains("Nothing to do"),
                "recovery should have reported the push as already completed (the accurate \
                 'Nothing to do' message from publish_journal::recover()), but did not.\n\
                 stdout: {}\nstderr: {}",
                recovery_stdout,
                recovery_stderr
            );
            recovery.assert_success();

            // Convergence: since this "next invocation" is itself a full
            // `hitch rebuild`, it recomposes and republishes regardless of
            // recovery (see `tree_oid`'s doc comment on why that means a
            // fresh commit, not necessarily the same commit SHA, even though
            // nothing about the declared inputs changed). What must hold is
            // that the resulting content is unchanged, and that no journal
            // record is left behind. In this scenario `recover()` already
            // drops the old record itself, in the same pass as the "already
            // completed" message above, because it proved the push had
            // landed. Even if it hadn't (the genuinely-owed case — see
            // `test_publish_journal_persists_owed_push_until_resolved`
            // below), this same `rebuild --force` would still end with no
            // record left, because it performs its own fresh publish for
            // 'dev' and clears its own record on success — so this assertion
            // alone would not distinguish "recover() cleaned up the stale
            // record" from "the new rebuild's own cycle overwrote and then
            // cleared it". That distinction is what the dedicated persists
            // test below proves instead, by triggering recovery through a
            // command that never touches 'dev's record at all.
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

    /// The other half of the push-check fix in `publish_journal::recover()`:
    /// a push that genuinely never happened must not be forgotten. Abort at
    /// `resync-done` — strictly before the push step runs — so the remote
    /// never receives the new tip, then prove two things recovery must do:
    /// warn about the owed push, and leave the record in place rather than
    /// deleting it, so the *next* mutating command's `recover()` warns again
    /// instead of the obligation silently vanishing after one report.
    ///
    /// Recovery is triggered here through `hitch add qa` rather than another
    /// `hitch rebuild dev`, deliberately: a second `rebuild dev` would launch
    /// its own fresh publish cycle for 'dev' and write (then, on success,
    /// clear) its own record at the same `refs/hitch/publish/dev` ref,
    /// masking whether `recover()` itself left the stale record alone.
    /// `hitch add qa` is a mutating command (so `recover()` still runs at
    /// its start, per `main.rs`'s `command_is_mutating`), but it never
    /// touches 'dev' or its journal record, so the record's state
    /// immediately after `add` returns is exactly what `recover()` alone
    /// left behind.
    #[test]
    fn test_publish_journal_persists_owed_push_until_resolved() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup(env)?;

            let bare_path = sibling_path(env, "bare-origin-persist.git");
            std::fs::create_dir_all(&bare_path)?;
            // Test-only: sets up a scratch bare remote to stand in for
            // origin, so it deliberately spawns plain git rather than going
            // through GitOperations (which has no repo at this path).
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

            // `run_raw()` does not inject `--no-push`, so this rebuild
            // genuinely intends to push — it just never gets there, because
            // 'resync-done' fires before the push step. `push_owed` on the
            // record this writes is therefore `true`, for a push that never
            // even started.
            let interrupted = env
                .hitch
                .run_raw()
                .args(&["rebuild", "dev"])
                .env("HITCH_TEST_ABORT_AFTER", "resync-done")
                .execute()?;
            assert!(
                !interrupted.success(),
                "aborting after 'resync-done' should have crashed the process, but it exited \
                 successfully — the abort hook did not fire.\nstdout: {}\nstderr: {}",
                interrupted.stdout(),
                interrupted.stderr()
            );

            // Confirm the remote never received the new tip — the push
            // genuinely never happened, so this test is exercising the
            // "still owed" case, not the "already landed" one.
            #[allow(clippy::disallowed_methods)]
            let remote_rev_parse = std::process::Command::new("git")
                .arg("--git-dir")
                .arg(&bare_path)
                .args(["rev-parse", "--verify", "--quiet", "refs/heads/dev"])
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(
                !remote_rev_parse.status.success(),
                "the bare remote already has a 'dev' branch — the push must not have happened \
                 yet for this test to exercise the genuinely-owed case"
            );

            let journal_oid_before = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/hitch/publish/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            assert!(
                !journal_oid_before.is_empty(),
                "aborting after 'resync-done' should leave a journal record behind, found none"
            );
            let payload_before = env
                .git
                .run(&["cat-file", "-p", &journal_oid_before])?
                .stdout();
            let record_before: serde_json::Value = serde_json::from_str(&payload_before)?;
            assert_eq!(
                record_before["push_owed"], true,
                "the record from a run_raw() rebuild aborted before the push step should still \
                 have push_owed set"
            );

            // Trigger recovery via a command that never touches 'dev's
            // record, so what's left afterward is exactly what recover()
            // itself did with it.
            let recovery = env.hitch.run().args(&["add", "qa"]).execute()?;
            let recovery_stdout = recovery.stdout();
            let recovery_stderr = recovery.stderr();
            recovery.assert_success();

            let warning_present = recovery_stdout.contains("is ahead of origin")
                || recovery_stderr.contains("is ahead of origin");
            assert!(
                warning_present,
                "recovery should have warned that 'dev' is ahead of origin — the push never \
                 happened, so the obligation is genuinely still owed.\nstdout: {}\nstderr: {}",
                recovery_stdout, recovery_stderr
            );

            let journal_oid_after = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/hitch/publish/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            assert!(
                !journal_oid_after.is_empty(),
                "recovery deleted the journal record for a push that never actually happened — \
                 a genuinely-owed push must persist so the next mutating command's recover() \
                 warns again, instead of the obligation being silently forgotten after one \
                 report"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
