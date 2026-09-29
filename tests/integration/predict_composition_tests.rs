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
    /// `(old_promote_refuses, new_promote_refuses, old_dependent_skips, new_dependent_skips)`.
    fn verdicts(
        build: impl Fn(&TestEnvironment) -> anyhow::Result<Scenario>,
    ) -> anyhow::Result<(bool, bool, bool, bool)> {
        let framework = HitchTestFramework::new()?;
        let mut out = None;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            let (existing, new) = build(env)?;
            let ctx = context_for(env)?;
            let existing_owned: Vec<String> = existing.iter().map(|s| s.to_string()).collect();

            let old_refuses = hitch::utils::prelude::pre_promote_conflict_reason(
                &ctx,
                new,
                &existing_owned,
                "main",
                "dev",
            )?
            .is_some();
            let mut all = existing.clone();
            all.push(new);
            let old_skips = hitch::utils::prelude::preflight_compatibility_merge_tree(
                &ctx,
                "main",
                &all.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )?;

            let proposed = predict_composition(&ctx, &environment("main", &all), "dev")?;
            let new_refuses = proposed.held.iter().any(|h| h.branch == new);
            let new_skips = !proposed.held.is_empty();

            if let Some(failure) = &old_skips {
                if let Some(h) = proposed
                    .held
                    .iter()
                    .find(|h| h.branch == failure.blocking_branch)
                {
                    let mut old_files = failure.conflicted_files.clone();
                    let mut new_files = h.conflicted_files.clone();
                    old_files.sort();
                    new_files.sort();
                    assert_eq!(old_files, new_files, "same conflicted files");
                }
            }
            out = Some((old_refuses, new_refuses, old_skips.is_some(), new_skips));
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(out.expect("scenario ran"))
    }

    #[test]
    fn differential_clean_siblings_agree() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "a.txt", "a\n")?;
            branch_off_main(env, "feat-b", "b.txt", "b\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (false, false, false, false));
        Ok(())
    }

    #[test]
    fn differential_non_overlapping_edits_to_one_file_agree() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "ONE\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "one\ntwo\nthree\nfour\nFIVE\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (false, false, false, false));
        Ok(())
    }

    #[test]
    fn differential_peer_conflict_agrees() -> anyhow::Result<()> {
        let v = verdicts(|env| {
            seed_shared(env)?;
            branch_off_main(env, "feat-a", "shared.txt", "A\ntwo\nthree\nfour\nfive\n")?;
            branch_off_main(env, "feat-b", "shared.txt", "B\ntwo\nthree\nfour\nfive\n")?;
            Ok((vec!["feat-a"], "feat-b"))
        })?;
        assert_eq!(v, (true, true, true, true));
        Ok(())
    }

    /// AGENTS.md "Wrong merge-base": the new branch conflicts with a base that
    /// moved after it diverged.
    #[test]
    fn differential_new_branch_conflicts_with_moved_base_agrees() -> anyhow::Result<()> {
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
        assert_eq!(v, (true, true, true, true));
        Ok(())
    }

    /// Recorded disagreement. An already-promoted branch that the base has
    /// since moved out from under is *held* by a build, and a build carries on
    /// with everything else. The old promote check refused the unrelated new
    /// branch anyway ("environment already contains incompatible promoted
    /// branches"); the composition, which is what the rebuild does, accepts it.
    /// The dependent-skip verdict is unchanged: the environment does hold
    /// something, so a release still declines to rebuild it.
    #[test]
    fn differential_unrelated_new_branch_beside_an_already_held_one() -> anyhow::Result<()> {
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
        assert_eq!(v, (true, false, true, true));
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
}
