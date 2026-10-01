//! `predict_composition` against the tree-based oracles it replaced, and on its
//! own terms afterwards.
//!
//! The differential half exists because the two engines could disagree: the old
//! preflights ran a hand-built `--merge-base` loop over trees, while a build
//! runs ORT over commits. Where they disagree the composition is right, so each
//! disagreement is a named scenario with the verdict spelled out rather than a
//! silently loosened assertion.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::commands::global_context::{GlobalContext, GlobalFlags};
    use hitch::types::Environment;
    use hitch::utils::logging::Logger;
    use hitch::utils::prelude::predict_composition;
    use std::sync::Arc;

    fn context_for(env: &TestEnvironment) -> anyhow::Result<GlobalContext> {
        GlobalContext::new_at_path(
            env.temp_dir.to_str().expect("utf-8 temp dir"),
            GlobalFlags {
                verbose: false,
                no_push: true,
                assume_yes: true,
                json: false,
            },
            Arc::new(Logger::new()),
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))
    }

    fn commit_file(
        env: &TestEnvironment,
        file: &str,
        content: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        env.fs.write_file(file, content)?;
        env.git.run(&["add", "-f", file])?;
        env.git.run(&["commit", "-m", message])?;
        Ok(())
    }

    /// A branch off `main`'s current tip that replaces `file` wholesale.
    fn branch_off_main(
        env: &TestEnvironment,
        name: &str,
        file: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", name, "main"])?;
        commit_file(env, file, content, &format!("{name} edits {file}"))?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    fn seed_shared(env: &TestEnvironment) -> anyhow::Result<()> {
        commit_file(env, "shared.txt", "one\ntwo\nthree\nfour\nfive\n", "shared")
    }

    fn environment(base: &str, branches: &[&str]) -> Environment {
        let mut e = Environment::new(base.to_string());
        e.branches = branches.iter().map(|b| b.to_string()).collect();
        e
    }

    /// `(existing, new)` promoted branches for a scenario built on `main`.
    type Scenario = (Vec<&'static str>, &'static str);

    /// Runs one scenario and returns
    /// `(promote_of_new_is_refused, environment_has_any_held_branch)`, the two
    /// questions the promote and release planners ask of the prediction. The
    /// expected values were established against the tree-based oracles this
    /// replaced (run side by side in the previous commit) and are the old
    /// verdicts, except where a scenario says otherwise.
    fn verdicts(
        build: impl Fn(&TestEnvironment) -> anyhow::Result<Scenario>,
    ) -> anyhow::Result<(bool, bool)> {
        let framework = HitchTestFramework::new()?;
        let mut out = None;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            let (existing, new) = build(env)?;
            let ctx = context_for(env)?;
            let mut all = existing.clone();
            all.push(new);
            let proposed = predict_composition(&ctx, &environment("main", &all), "dev")?;
            out = Some((
                proposed.held.iter().any(|h| h.branch == new),
                !proposed.held.is_empty(),
            ));
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(out.expect("scenario ran"))
    }

    #[test]
    fn predicts_clean_siblings_agree() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            branch_off_main(env, "feat-b", "b.txt", "b\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (false, false));
        Ok(())
    }

    #[test]
    fn predicts_non_overlapping_edits_to_one_file_agree() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "ONE\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "one\ntwo\nthree\nfour\nFIVE\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (false, false));
        Ok(())
    }

    #[test]
    fn predicts_peer_conflict_agrees() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "B\ntwo\nthree\nfour\nfive\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (true, true));
        Ok(())
    }

    /// AGENTS.md "Wrong merge-base": the new branch conflicts with a base that
    /// moved after it diverged.
    #[test]
    fn predicts_new_branch_conflicts_with_moved_base_agrees() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "B\ntwo\nthree\nfour\nfive\n")?;
            commit_file(
                env,
                "shared.txt",
                "MAIN\ntwo\nthree\nfour\nfive\n",
                "main moves",
            )?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (true, true));
        Ok(())
    }

    /// Recorded disagreement. An already-promoted branch that the base has
    /// since moved out from under is *held* by a build, and a build carries on
    /// with everything else. The old promote check refused the unrelated new
    /// branch anyway ("environment already contains incompatible promoted
    /// branches"); the composition, which is what the rebuild does, accepts it.
    /// The first flag is therefore `false` where the old oracle said `true`. The
    /// dependent-skip verdict is unchanged: the environment does hold
    /// something, so a release still declines to rebuild it.
    #[test]
    fn predicts_unrelated_new_branch_beside_an_already_held_one() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-c", "c.txt", "c\n")?;
            commit_file(
                env,
                "shared.txt",
                "MAIN\ntwo\nthree\nfour\nfive\n",
                "main moves",
            )?;
            Ok((vec!["feat-a"], "feat-c"))
        })?;
        assert_eq!(v, (false, true));
        Ok(())
    }

    /// A prediction reads refs and nothing else. The base exists only as a
    /// cached `origin/*` ref and origin points nowhere, so any `ls-remote` or
    /// `fetch` would fail (and `pin_environment_inputs` would refuse the base
    /// outright). `GIT_TRACE` is not used: it is a process-global variable and
    /// the other tests in this binary run git in parallel, so their fetches
    /// would land in the trace. A fetch cannot avoid touching `FETCH_HEAD`, so its
    /// mtime is checked instead.
    #[test]
    fn a_prediction_is_offline_and_reads_a_base_that_exists_only_as_a_cached_remote_ref(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            let main_sha = env.git.run(&["rev-parse", "main"])?.stdout();
            let main_sha = main_sha.trim();
            env.git
                .run(&["update-ref", "refs/remotes/origin/cached-base", main_sha])?;
            env.git.run(&[
                "remote",
                "set-url",
                "origin",
                "/nonexistent/hitch-offline-check",
            ])?;

            let ctx = context_for(env)?;
            let fetch_head = env.temp_dir.join(".git/FETCH_HEAD");
            let fetched_at = std::fs::metadata(&fetch_head)
                .and_then(|m| m.modified())
                .ok();
            let before = env.git.run(&["for-each-ref", "refs/"])?.stdout();
            let result =
                predict_composition(&ctx, &environment("cached-base", &["feat-a"]), "dev")?;
            assert_eq!(result.included, vec!["feat-a".to_string()]);
            assert!(result.held.is_empty());
            assert_eq!(
                before,
                env.git.run(&["for-each-ref", "refs/"])?.stdout(),
                "no ref moved"
            );
            assert_eq!(
                fetched_at,
                std::fs::metadata(&fetch_head)
                    .and_then(|m| m.modified())
                    .ok(),
                "a fetch ran"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    use hitch::utils::prelude::CompatibilityConflict;

    fn triples(mut c: Vec<CompatibilityConflict>) -> Vec<(String, String, Vec<String>)> {
        c.sort_by(|a, b| a.branch.cmp(&b.branch));
        c.into_iter()
            .map(|mut c| {
                c.conflicted_files.sort();
                (c.branch, c.conflicts_with, c.conflicted_files)
            })
            .collect()
    }

    /// What `hitch conflicts`, `status` and `tree` display (the held triples)
    /// and what the approval snapshot records (`merge_conflicts`), for one
    /// scenario. The old tree-based oracles agreed with these on every
    /// scenario below except the one named in
    /// `a_peer_only_conflict_is_recorded_on_the_approval_snapshot`.
    #[allow(clippy::type_complexity)]
    fn display_verdicts(
        build: impl Fn(&TestEnvironment) -> anyhow::Result<Scenario>,
    ) -> anyhow::Result<(Vec<(String, String, Vec<String>)>, bool)> {
        let framework = HitchTestFramework::new()?;
        let mut out = None;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            let (existing, new) = build(env)?;
            let ctx = context_for(env)?;
            let snap = hitch::utils::snapshot::capture_rebuild_snapshot(
                &ctx,
                &environment("main", &existing),
                "dev",
                new,
            )?;
            let mut all = existing.clone();
            all.push(new);
            let held = predict_composition(&ctx, &environment("main", &all), "dev")?.held;
            out = Some((triples(held), snap.merge_conflicts));
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(out.expect("scenario ran"))
    }

    fn peer_conflict(env: &TestEnvironment) -> anyhow::Result<Scenario> {
        seed_shared(env)?;
        branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
        branch_off_main(env, "feat-b", "shared.txt", "B\ntwo\nthree\nfour\nfive\n")?;
        Ok((vec!["feat-a"], "feat-b"))
    }

    #[test]
    fn display_verdicts_clean_and_moved_base() -> anyhow::Result<()> {
        let (held, snap) = display_verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            branch_off_main(env, "feat-b", "b.txt", "b\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert!(held.is_empty() && !snap);

        let (held, snap) = display_verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "B\ntwo\nthree\nfour\nfive\n")?;
            commit_file(
                env,
                "shared.txt",
                "MAIN\ntwo\nthree\nfour\nfive\n",
                "main moves",
            )?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(
            held,
            vec![(
                "feat-b".to_string(),
                // The last-composed branch is named, not the base the conflict
                // is really with: shared quirk of every oracle, see AGENTS.md.
                "feat-a".to_string(),
                vec!["shared.txt".to_string()]
            )]
        );
        assert!(snap);
        Ok(())
    }

    #[test]
    fn display_verdicts_name_the_peer_for_a_peer_conflict() -> anyhow::Result<()> {
        let (held, _) = display_verdicts(peer_conflict)?;
        assert_eq!(
            held,
            vec![(
                "feat-b".to_string(),
                "feat-a".to_string(),
                vec!["shared.txt".to_string()]
            )]
        );
        Ok(())
    }

    /// Recorded disagreement with the deleted oracle. The approval snapshot
    /// compared each branch to the base alone, so two branches that only collide
    /// with each other read as "no merge conflicts". The composition holds
    /// feat-b, which is what the rebuild does, so the snapshot now says so.
    #[test]
    fn a_peer_only_conflict_is_recorded_on_the_approval_snapshot() -> anyhow::Result<()> {
        let (_, snap) = display_verdicts(peer_conflict)?;
        assert!(snap);
        Ok(())
    }

    #[test]
    fn display_verdicts_beside_an_already_held_branch() -> anyhow::Result<()> {
        let (held, snap) = display_verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-c", "c.txt", "c\n")?;
            commit_file(
                env,
                "shared.txt",
                "MAIN\ntwo\nthree\nfour\nfive\n",
                "main moves",
            )?;
            Ok((vec!["feat-a"], "feat-c"))
        })?;
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].0, "feat-a");
        assert!(snap);
        Ok(())
    }

    /// The in-process offline test above cannot see a network call whose error
    /// is swallowed. Reading the metadata branch legitimately fetches it, so a
    /// fetch of exactly `origin hitch-metadata` is let through. This one puts a `git` first on PATH for the child `hitch`
    /// that records every `fetch`/`ls-remote` to a log file (and fails it), then
    /// runs the commands that predict. The log, not the exit code, is the
    /// assertion, so a swallowed error still fails the test.
    #[cfg(unix)]
    #[allow(clippy::disallowed_methods)] // test harness spawning a fake git to prove the control
    #[test]
    fn status_and_tree_predict_without_fetch_or_ls_remote() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
            env.hitch.exec(&["add", "dev"])?.assert_success();
            env.hitch.exec(&["promote", "feat-a", "dev"])?.assert_success();
            commit_file(env, "shared.txt", "MAIN\ntwo\nthree\nfour\nfive\n", "main moves")?;

            let real_git = std::process::Command::new("sh")
                .args(["-c", "command -v git"])
                .output()?;
            let real_git = String::from_utf8(real_git.stdout)?.trim().to_string();
            let bin = env.temp_dir.join("fake-bin");
            std::fs::create_dir_all(&bin)?;
            let log = env.temp_dir.join("network-calls.log");
            let script = format!(
                "#!/bin/sh\ncase \"$*\" in *\" fetch origin hitch-metadata\") exec '{}' \"$@\";; esac\nfor a in \"$@\"; do\n  case \"$a\" in\n    fetch|ls-remote|pull|push)\n      echo \"$@\" >> '{}'\n      echo NETWORK-CALL-MARKER >&2\n      exit 1;;\n  esac\ndone\nexec '{}' \"$@\"\n",
                real_git,
                log.display(),
                real_git
            );
            let wrapper = bin.join("git");
            std::fs::write(&wrapper, script)?;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;

            // Control: the wrapper does trap a fetch, so an empty log means
            // something.
            let control = std::process::Command::new(&wrapper)
                .arg("fetch")
                .output()?;
            assert!(!control.status.success());
            assert!(log.exists());
            std::fs::remove_file(&log)?;

            let path = format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            );
            for args in [vec!["status"], vec!["status", "--environments"], vec!["tree"]] {
                let out = env.hitch.run().args(&args).env("PATH", &path).execute()?;
                let stderr = out.stderr();
                let stdout = out.stdout();
                assert!(
                    !log.exists(),
                    "`hitch {args:?}` hit the network: {}",
                    std::fs::read_to_string(&log).unwrap_or_default()
                );
                assert!(!stderr.contains("NETWORK-CALL-MARKER"), "{stderr}");
                assert!(out.success(), "`hitch {args:?}` failed: {stderr}");
                assert!(stdout.contains("feat-a"), "{stdout}");
                // `status --environments` is the path that predicts; the
                // plain matrix never reaches it.
                if args.contains(&"--environments") {
                    assert!(
                        stdout.contains("would be held on the next rebuild"),
                        "the prediction did not run: {stdout}"
                    );
                }
            }
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A branch the last build included and that has since been merged into the
    /// base (no rebuild) is still offered for demotion. The build record
    /// outranks the live base check for membership, so this is a separate
    /// snapshot fact (`contained_in_base`), not `AlreadyInBase`.
    #[test]
    fn cleanup_hint_names_an_included_branch_since_merged_into_the_base() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            env.hitch.exec(&["add", "dev"])?.assert_success();
            env.hitch
                .exec(&["promote", "feat-a", "dev"])?
                .assert_success();
            // A commit after the build makes feat-a stale; merging it into main
            // must then read as "already in base", not as new commits.
            env.git.run(&["checkout", "feat-a"])?;
            commit_file(env, "a2.txt", "a2\n", "feat-a moves on")?;
            env.git.run(&["checkout", "main"])?;
            env.git
                .run(&["merge", "--no-ff", "-m", "land feat-a", "feat-a"])?;

            let out = env.hitch.exec(&["status", "--environments"])?;
            let stdout = out.stdout();
            assert!(out.success(), "{stdout}");
            assert!(stdout.contains("Branches already in source"), "{stdout}");
            assert!(stdout.contains("(already in"), "{stdout}");
            assert!(
                !stdout.contains("new commits since last rebuild"),
                "{stdout}"
            );
            assert!(stdout.contains("hitch demote feat-a dev"), "{stdout}");
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}
