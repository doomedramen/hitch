//! Integration tests for hitch cleanup command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Dry-run (default) lists demoted branches without deleting them.
    #[test]
    fn test_cleanup_dry_run_lists_candidates() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a branch, promote it, then demote it — it should show up as a candidate
            env.git.run(&["checkout", "-b", "feat-demoted"])?;
            env.fs.write_file("demoted.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat-demoted"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feat-demoted", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["demote", "feat-demoted", "dev"])
                .execute()?
                .assert_success();

            // dry-run should list feat-demoted
            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("feat-demoted")
                .assert_stdout_contains("dry-run");

            // Branch must still exist (no deletion in dry-run)
            let branch_list = env.git.run(&["branch", "--list", "feat-demoted"])?;
            assert!(
                !branch_list.stdout().trim().is_empty(),
                "feat-demoted should still exist after dry-run"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--apply` deletes branches that are fully merged and not promoted.
    #[test]
    fn test_cleanup_force_deletes_merged_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and merge a branch into main so it qualifies for clean deletion
            env.git.run(&["checkout", "-b", "feat-merged"])?;
            env.fs.write_file("merged.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat-merged"])?;
            env.git.run(&["checkout", "main"])?;
            // Merge it so `git branch -d` will succeed
            env.git
                .run(&["merge", "--no-ff", "feat-merged", "-m", "Merge feat-merged"])?;

            // It was never promoted, so cleanup should offer it
            let result = env.hitch.run().args(&["cleanup", "--apply"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Deleted 'feat-merged'");

            // Branch should be gone
            let branch_list = env.git.run(&["branch", "--list", "feat-merged"])?;
            assert!(
                branch_list.stdout().trim().is_empty(),
                "feat-merged should be deleted after --apply, but branch still exists"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Branches that are currently promoted must never appear as candidates.
    #[test]
    fn test_cleanup_skips_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feat-active"])?;
            env.fs.write_file("active.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat-active"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote and keep promoted
            env.hitch
                .run()
                .args(&["promote", "feat-active", "dev"])
                .execute()?
                .assert_success();

            // cleanup dry-run output must NOT mention feat-active
            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            // Either nothing to clean or the active branch is not listed
            let stdout = result.stdout();
            assert!(
                !stdout.contains("feat-active"),
                "Currently-promoted branch should not appear in cleanup candidates"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// When there is nothing to clean up, the command reports "No branches to clean up."
    #[test]
    fn test_cleanup_nothing_to_do() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // No extra branches — only main exists (the current branch)
            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No branches to clean up");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// refs/hitch/backup/* and refs/hitch/prev/* both accumulate one ref per
    /// rebuild with no existing bound — `hitch cleanup --apply` must prune
    /// each namespace down to its N-most-recent-per-environment, independent
    /// of the other namespace and of other environments.
    #[test]
    fn test_cleanup_prunes_old_archive_refs_per_env_per_namespace() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();

            let head = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .stdout()
                .trim()
                .to_string();

            // Plant more refs than the retention policy keeps, in both
            // namespaces, across two environments, so the test also proves
            // pruning is scoped per-env (dev's excess doesn't affect qa's
            // count and vice versa).
            for namespace in ["backup", "prev"] {
                for env_name in ["dev", "qa"] {
                    for i in 0..15 {
                        env.git
                            .run(&[
                                "update-ref",
                                &format!(
                                    "refs/hitch/{}/{}/2020010100{:04}",
                                    namespace, env_name, i
                                ),
                                &head,
                            ])?
                            .assert_success();
                    }
                }
            }

            env.hitch
                .run()
                .args(&["cleanup", "--apply"])
                .execute()?
                .assert_success();

            for namespace in ["backup", "prev"] {
                for env_name in ["dev", "qa"] {
                    let remaining = env
                        .git
                        .run(&[
                            "for-each-ref",
                            "--format=%(refname)",
                            &format!("refs/hitch/{}/{}", namespace, env_name),
                        ])?
                        .stdout();
                    let mut surviving: Vec<String> = remaining
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    surviving.sort();

                    // The 10 planted refs with the highest timestamp suffix
                    // (i == 5..=14, since 15 were planted as i == 0..=14) are
                    // the ones retention must keep. Asserting the exact set —
                    // not just the count — catches an inverted sort/skip that
                    // would keep the OLDEST 10 and delete the newest: a count
                    // of 10 would look identical either way, but that would
                    // silently discard the most recent rollback points, the
                    // ones actually useful.
                    let expected: Vec<String> = (5..15)
                        .map(|i| {
                            format!("refs/hitch/{}/{}/2020010100{:04}", namespace, env_name, i)
                        })
                        .collect();

                    assert_eq!(
                        surviving, expected,
                        "cleanup did not keep exactly the 10 most-recent refs under \
                         refs/hitch/{}/{} — got:\n{:?}\nexpected:\n{:?}",
                        namespace, env_name, surviving, expected
                    );
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `refs/hitch/state/*` is a **live pointer**, not an archive, and cleanup
    /// must never prune it.
    ///
    /// The distinction is easy to get wrong later because every other
    /// `refs/hitch/*` family is either an archive (`prev`, `backup` — pruned
    /// down to `ARCHIVE_REF_RETENTION`) or transient (`build`, `publish`,
    /// `resolutions` — not hitch's to keep). `state` is the one ref whose
    /// whole purpose is to be the *current* answer to "what is in this
    /// environment branch", so deleting it as a stale artefact would silently
    /// degrade the tool back to guessing. `cleanup.rs`'s prunable set names
    /// `["backup", "prev"]` explicitly and this test is what holds it to that.
    ///
    /// Asserts both halves. "The state ref survived" alone would pass even if
    /// cleanup had stopped pruning anything at all, so the archive refs are
    /// planted and their post-cleanup count is checked too.
    #[test]
    fn test_cleanup_does_not_prune_the_build_record_ref() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // A real build, so the record is written by the product rather
            // than planted — this is testing that cleanup spares it, not that
            // it can be conjured.
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

            let state_oid_before = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/hitch/state/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            assert!(
                !state_oid_before.is_empty(),
                "the promote's rebuild must have left a build record"
            );

            // Plant more archive refs than retention keeps, so cleanup has real
            // work and the second assertion below is not vacuous.
            let head = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .stdout()
                .trim()
                .to_string();
            for i in 0..15 {
                env.git
                    .run(&[
                        "update-ref",
                        &format!("refs/hitch/prev/dev/2020010100{:04}", i),
                        &head,
                    ])?
                    .assert_success();
            }

            env.hitch
                .run()
                .args(&["cleanup", "--apply"])
                .execute()?
                .assert_success();

            // 1. The live record survived, unchanged and still readable.
            let state_oid_after = env
                .git
                .run(&[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/hitch/state/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                state_oid_after, state_oid_before,
                "cleanup must not touch refs/hitch/state/dev — it is the live \
                 answer to what the environment branch contains, not an archive"
            );
            let payload = env.git.run(&["cat-file", "-p", &state_oid_after])?.stdout();
            let record: serde_json::Value = serde_json::from_str(&payload)?;
            assert_eq!(
                record["result_sha"].as_str().unwrap(),
                env.git.run(&["rev-parse", "dev"])?.stdout().trim(),
                "and it must still parse and describe the live tip"
            );

            // 2. Cleanup really did prune, so assertion 1 is about the state
            // ref specifically rather than about cleanup doing nothing.
            let remaining = env
                .git
                .run(&["for-each-ref", "--format=%(refname)", "refs/hitch/prev/dev"])?
                .stdout();
            let surviving: Vec<&str> = remaining
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            assert_eq!(
                surviving.len(),
                10,
                "cleanup should have pruned the planted archive refs down to the \
                 retention limit, but left {} of them: {:?}",
                surviving.len(),
                surviving
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
