//! Integration tests for hitch resolve command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Mode A: a branch conflicting with base gets a guided rebase — run in a
    /// disposable detached worktree, never the user's own checkout. Git pauses
    /// it on conflict there; the user resolves with plain Git and hitch lands
    /// the result.
    #[test]
    fn test_resolve_mode_a_rebases_without_touching_the_users_checkout() -> anyhow::Result<()> {
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

            // The user has unrelated work in progress. It must survive.
            env.fs.write_file("scratch.txt", "my work")?;

            let result = env.hitch.run().args(&["resolve", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("durable fix is rebasing")
                .assert_stdout_contains("Rebase paused with conflicts");

            // The user's checkout is exactly where they left it: same branch,
            // no rebase in progress, uncommitted work intact.
            assert_eq!(
                env.git.run(&["branch", "--show-current"])?.stdout().trim(),
                "main",
                "resolve moved the user off their own branch"
            );
            assert!(
                !env.temp_dir.join(".git/rebase-merge").exists()
                    && !env.temp_dir.join(".git/rebase-apply").exists(),
                "resolve left a rebase in progress in the user's own checkout"
            );
            assert_eq!(env.fs.read_file("scratch.txt")?, "my work");
            assert_eq!(
                env.fs.read_file("shared.txt")?,
                "from-main-later\n",
                "resolve modified the user's working tree"
            );

            // The rebase is paused in a worktree of its own.
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
            assert!(session.exists(), "no resolve worktree at {}", session_path);

            let session_git = GitCommandRunner::new(&session)?;
            assert!(
                session.join(".git").exists(),
                "resolve worktree is not a git checkout"
            );

            // Resolve it there with plain git, exactly as hitch instructed.
            std::fs::write(session.join("shared.txt"), "resolved\n")?;
            session_git.run(&["add", "shared.txt"])?.assert_success();
            // -c core.editor=true is belt-and-suspenders: a single-commit
            // --continue already reuses the original message without opening
            // an editor, but this guarantees it can't block on one.
            session_git
                .run(&["-c", "core.editor=true", "rebase", "--continue"])?
                .assert_success();

            // hitch lands it.
            env.hitch
                .run()
                .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("rebased onto");

            assert!(
                !session.exists(),
                "resolve worktree survived a successful --continue"
            );
            let worktrees = env.git.run(&["worktree", "list"])?;
            assert_eq!(
                worktrees.stdout().lines().count(),
                1,
                "resolve left a worktree registered: {}",
                worktrees.stdout()
            );

            assert_eq!(
                env.git
                    .run(&["show", "branch-a:shared.txt"])?
                    .stdout()
                    .trim(),
                "resolved",
                "the rebased result was not landed on the branch"
            );
            assert_eq!(env.fs.read_file("scratch.txt")?, "my work");
            env.fs.remove("scratch.txt")?; // rebuild rightly requires a clean tree

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

    /// The branch being rebased may be the one the user is standing on. That
    /// used to be impossible to handle (git refuses to check out a branch
    /// twice); working detached means it just resyncs like any other publish.
    #[test]
    fn test_resolve_mode_a_resyncs_a_checkout_standing_on_the_rebased_branch() -> anyhow::Result<()>
    {
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
            env.git.run(&["commit", "-m", "branch-a"])?;

            env.git.run(&["checkout", "main"])?;
            env.fs.write_file("shared.txt", "from-main-later\n")?;
            env.fs.write_file("only-on-main.txt", "later")?;
            env.git.run(&["add", "-f", "."])?;
            env.git.run(&["commit", "-m", "main moves on"])?;

            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            // The user is standing on the very branch about to be rebased.
            env.git.run(&["checkout", "branch-a"])?.assert_success();
            assert!(!env.fs.file_exists("only-on-main.txt"));

            env.hitch
                .run()
                .args(&["resolve", "dev", "--branch", "branch-a"])
                .execute()?
                .assert_success();

            // Still on their branch, nothing rebasing under them.
            assert_eq!(
                env.git.run(&["branch", "--show-current"])?.stdout().trim(),
                "branch-a",
                "resolve moved the user off their branch"
            );
            assert!(
                !env.temp_dir.join(".git/rebase-merge").exists()
                    && !env.temp_dir.join(".git/rebase-apply").exists(),
                "resolve started a rebase in the user's own checkout"
            );

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

            env.hitch
                .run()
                .args(&["resolve", "dev", "--branch", "branch-a", "--continue"])
                .execute()?
                .assert_success();

            // The branch moved under a checkout standing on it — which must
            // therefore have been brought along.
            let status = env.git.run(&["status", "--porcelain"])?;
            assert!(
                status.stdout().trim().is_empty(),
                "the rebased branch left its own checkout desynchronized: '{}'",
                status.stdout().trim()
            );
            assert!(
                env.fs.file_exists("only-on-main.txt"),
                "the checkout was not brought up to the rebased tip"
            );
            assert_eq!(env.fs.read_file("shared.txt")?, "resolved\n");

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
                // Test-only: simulates a user editing files and staging them
                // by hand in the resolve worktree, so it deliberately spawns
                // plain git rather than going through GitOperations.
                #[allow(clippy::disallowed_methods)]
                let mut cmd = std::process::Command::new("git");
                cmd.args(args)
                    .current_dir(&worktree_path)
                    .stdin(std::process::Stdio::null())
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

    /// Read the build record at `refs/hitch/state/<env>`, or `None`. Reads it
    /// back through plain git rather than hitch's own reader, because the
    /// record's storage is part of what is under test.
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

    /// A replayed resolution is recorded as *what it actually was*: a branch
    /// that composed successfully, plus the content-addressed key of the
    /// recording that made it possible.
    ///
    /// Sits next to
    /// `test_dry_run_agrees_with_real_build_about_replayed_resolutions` and
    /// reuses `setup_and_record` because that fixture already builds a genuine
    /// recorded resolution; duplicating it into a second module would have
    /// meant two places to keep a recording actually recorded.
    #[test]
    fn test_replayed_branch_is_recorded_as_included_with_its_resolution_key() -> anyhow::Result<()>
    {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_and_record(env)?;

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

            let record = read_state_record(env, "dev")?
                .expect("a replaying rebuild published, so it must record");

            // A replayed branch composed. It is therefore *included*, and
            // categorically not held — a replay is precisely the case where a
            // branch that would have conflicted did not.
            let included: Vec<&str> = record["included_branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["branch"].as_str().unwrap())
                .collect();
            assert_eq!(included, vec!["branch-a", "branch-b"]);
            assert!(
                record["held"].as_array().unwrap().is_empty(),
                "nothing conflicted, because the conflict was resolved: {:?}",
                record["held"]
            );

            let replayed = record["replayed_resolutions"].as_array().unwrap();
            assert_eq!(replayed.len(), 1, "expected one replay: {replayed:?}");
            assert_eq!(replayed[0]["branch"], "branch-b");

            // The key is the point. It is content-addressed, so the same branch
            // replayed against a *later* conflict resolves under a different
            // one — a branch name alone cannot say which human-authored fix
            // produced a build. It must also name a resolution that actually
            // exists: a key nothing resolves to would make the record a claim
            // with nothing behind it.
            let key = replayed[0]["resolution_key"].as_str().unwrap();
            assert!(!key.is_empty(), "a resolution key must never be blank");
            env.git
                .run(&[
                    "rev-parse",
                    "--verify",
                    &format!("refs/hitch/resolutions/{key}"),
                ])?
                .assert_success();
            // And the recording must be the one that was replayed, i.e. it must
            // be in the list `hitch resolutions` would print.
            let listed = env
                .hitch
                .run()
                .args(&["--no-push", "resolutions", "list"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();
            assert!(
                listed.contains(&key[..12.min(key.len())]),
                "the recorded key must be one hitch actually holds a resolution \
                 for; `hitch resolutions` printed:\n{listed}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Mode B publishes an environment branch without writing a record, and
    /// that absence is deliberate.
    ///
    /// It is a real gap, pinned as a test so it stays visible instead of
    /// surfacing later as a mysterious `LegacyUnknown`. A Mode B build *does*
    /// produce an environment branch and so arguably should record one; the
    /// missing input is the included-branch list, which hitch does not track
    /// for a hand-resolved worktree — the whole point of Mode B is that a
    /// human wrote that result. Writing a record with a guessed
    /// `included_branches` would be a fabricated Actual in a new costume,
    /// which is the specific failure this record type exists to prevent. The
    /// follow-up is to have Mode B track what it composed and then record it.
    ///
    /// Sits next to `test_resolve_mode_b_worktree_continue_publishes_and_cleans_up`
    /// and reuses its whole flow, since that test already drives a Mode B
    /// session all the way to a real publish.
    #[test]
    fn test_resolve_mode_b_publishes_without_a_record_and_leaves_no_usable_actual(
    ) -> anyhow::Result<()> {
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

            env.hitch
                .run()
                .args(&["--no-push", "resolve", "dev"])
                .execute()?
                .assert_success();

            let worktree_path = env.temp_dir.parent().unwrap().join(format!(
                ".hitch-resolve-{}-dev-branch-b",
                env.temp_dir.file_name().unwrap().to_string_lossy()
            ));
            std::fs::write(worktree_path.join("shared.txt"), "resolved-both\n")?;
            // Test-only: simulates a user staging their edited file by hand in
            // the resolve worktree, so it deliberately spawns plain git rather
            // than going through GitOperations.
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new("git")
                .args(["add", "shared.txt"])
                .current_dir(&worktree_path)
                .stdin(std::process::Stdio::null())
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
                ])
                .execute()?
                .assert_success()
                .assert_stdout_contains("Published 'dev'");

            // It really did publish — otherwise "no record" would be
            // vacuously true of a no-op.
            assert_eq!(
                env.git.run(&["show", "dev:shared.txt"])?.stdout().trim(),
                "resolved-both"
            );

            assert!(
                read_state_record(env, "dev")?.is_none(),
                "Mode B has no composition result to record honestly, so it must \
                 write no record rather than an invented one"
            );

            // The absence is per-publish, not a one-way door: the next ordinary
            // rebuild records normally, even though the branch this one
            // published is still there.
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_exit_code(2)
                .assert_stdout_contains("held");
            assert!(
                read_state_record(env, "dev")?.is_some(),
                "a plain rebuild after a recordless publish must still record — \
                 otherwise the check above would pass for the wrong reason"
            );

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
        // Test-only: simulates a user staging their edited file by hand in
        // the resolve worktree, so it deliberately spawns plain git rather
        // than going through GitOperations.
        #[allow(clippy::disallowed_methods)]
        std::process::Command::new("git")
            .args(["add", "shared.txt"])
            .current_dir(&worktree_path)
            .stdin(std::process::Stdio::null())
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

    /// The regression test for the divergence phase P1 removed, and the
    /// sharpest statement of the phase's invariant.
    ///
    /// `rebuild --dry-run --replay-resolutions` used to report `branch-b` as
    /// **held** while `rebuild --replay-resolutions` actually **composed** it
    /// from the recording. Not a cosmetic mismatch: the dry-run exited 2 ("would
    /// hold") for a build that was going to succeed cleanly, so anyone gating a
    /// pipeline on the preview got a false alarm, and anyone reading it got a
    /// confidently wrong answer about what the build would do.
    ///
    /// The cause was structural, not a bug in either path. The dry-run asked
    /// `preflight_compatibility_report`, a *tree-based* loop over
    /// `merge-tree --write-tree-name-only`, which has no notion of a recorded
    /// resolution and therefore could not see one; the build asked
    /// `merge_tree_compose`, a *commit-based* loop that tries replay first.
    /// Two implementations of the same question.
    ///
    /// Both now call `compose_environment`, so the preview is the build. This
    /// asserts agreement in both directions of the flag — replay on (neither
    /// holds) and replay off (both hold) — because a test that only checked the
    /// fixed case would also pass if the dry-run had simply stopped previewing
    /// holds altogether.
    #[test]
    fn test_dry_run_agrees_with_real_build_about_replayed_resolutions() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_and_record(env)?;

            // Replay OFF: the conflict is still unresolved, so both paths hold.
            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev", "--dry-run"])
                .execute()?
                .assert_exit_code(2)
                .assert_stdout_contains("branch-b held — conflicts with branch-a");

            env.hitch
                .run()
                .args(&["--no-push", "rebuild", "dev"])
                .execute()?
                .assert_exit_code(2)
                .assert_stdout_contains("branch-b held — conflicts with branch-a");

            // Replay ON: the dry-run must now see the recording too. This is
            // the assertion that used to fail — it exited 2 and said "1 held".
            let dev_tip_before_dry_run = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "rebuild",
                    "dev",
                    "--dry-run",
                    "--replay-resolutions",
                ])
                .execute()?
                .assert_success()
                // Composed from the recording, not held: that is the whole point of
                // the preview seeing the resolution, and the plan names which
                // branch and which resolution.
                .assert_stdout_contains("from recorded resolution")
                .assert_stdout_contains("branch-b")
                // Paired with the assertion above, because either alone is a
                // claim the other contradicts: a preview that reported the
                // replay *and* a hold is not agreeing with the build.
                .assert_stdout_not_contains("held");

            // A preview that agrees with the build is still not a build: it
            // must not have moved the environment branch.
            assert_eq!(
                env.git
                    .run(&["rev-parse", "refs/heads/dev"])?
                    .stdout()
                    .trim(),
                dev_tip_before_dry_run,
                "--dry-run must not publish, even when it agrees with what the build will do"
            );

            // And the real build composes it, agreeing.
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

    /// Lineage staleness, distinct from key staleness: `git commit --amend`
    /// on the recorded branch rewrites the commit (new SHA, same tree, same
    /// parent) without changing any blob content, so the conflict's stage
    /// OIDs — and therefore the resolution key — still match exactly. But
    /// the amended tip is a *sibling* of the commit the resolution was
    /// recorded against, not a descendant of it, so the lineage check must
    /// be what holds this branch, not the stage-OID key check that
    /// `test_replay_is_a_miss_after_branch_moves` above already covers.
    /// Assert on the lineage-specific warning text so a regression that
    /// widens the key check (making it also miss here) can't masquerade as
    /// this test still passing for the right reason.
    #[test]
    fn test_replay_is_a_miss_after_branch_head_is_amended() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_and_record(env)?;

            // Amend branch-b's tip without touching content: same tree, same
            // parent, new commit SHA — the stage OIDs the resolution key is
            // built from are untouched, but branch-b's tip commit is no
            // longer the (nor a descendant of the) commit the resolution's
            // source_branch_head names.
            env.git.run(&["checkout", "branch-b"])?;
            env.git
                .run(&["commit", "--amend", "--no-edit", "--allow-empty"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?;

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
                .assert_stdout_contains("held")
                .assert_stdout_contains("was recorded against a different point in");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
