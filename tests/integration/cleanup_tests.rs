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
            // Merged, so `git branch -d` accepts it and the plan can propose it.
            env.git.run(&[
                "merge",
                "--no-ff",
                "feat-demoted",
                "-m",
                "Merge feat-demoted",
            ])?;

            // The candidate list is the plan's "Will change" table, and each row
            // names a verb — a bare list of names is not a statement about what
            // would happen to them.
            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("delete feat-demoted")
                .assert_stdout_contains("preview — nothing was deleted");

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
                .assert_stdout_contains("delete feat-merged");

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

    /// A promoted branch is protected, and *named* as protected.
    ///
    /// This used to assert only `!stdout.contains("feat-active")`, which is why
    /// it passed while the sweep was offering to delete the `dev` environment
    /// branch: `feat-active` was the only name it looked at, and the answer to
    /// "is it a candidate" is a claim about the *Will change* table, not about
    /// the document. The plan now has a "Will not change" section that names
    /// the promoted branches, so the bare substring test is now the wrong test
    /// twice over — it would fail for the right reason and read as a failure.
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
            env.hitch
                .run()
                .args(&["promote", "feat-active", "dev"])
                .execute()?
                .assert_success();

            // A second branch, demoted, so the sweep has *something* to find and
            // the assertions below are about a non-empty plan rather than about
            // the empty-plan short circuit.
            env.git.run(&["checkout", "-b", "feat-gone"])?;
            env.fs.write_file("gone.txt", "content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat-gone"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feat-gone", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["demote", "feat-gone", "dev"])
                .execute()?
                .assert_success();
            env.git
                .run(&["merge", "--no-ff", "feat-gone", "-m", "Merge feat-gone"])?;

            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            let stdout = result.stdout();
            result.assert_success();
            let will_change = stdout
                .split_once("Will change")
                .expect("a non-empty plan has an effect list")
                .1
                .split_once('\n')
                .map(|(_, rest)| rest)
                .unwrap_or_default()
                .split("Will not change")
                .next()
                .unwrap_or_default();
            assert!(
                will_change.contains("delete feat-gone"),
                "the demoted branch is a candidate: {stdout}"
            );
            assert!(
                !will_change.contains("feat-active"),
                "a currently-promoted branch is not: {stdout}"
            );
            // …and it is *named* as spared, because "why wasn't my branch
            // deleted?" is the first question a reader has and the plan used to
            // make them work it out from what was missing.
            assert!(
                stdout.contains("Will not change") && stdout.contains("feat-active"),
                "and the plan says which branches it spared: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// When there is nothing to clean up, the command says so in one line and
    /// prints no plan.
    ///
    /// "No *branches* to clean up" was half-wrong the moment the sweep also
    /// pruned archive refs, and now half-right: an empty plan renders as a
    /// headline and nothing else, which is true and reads as a command that did
    /// not run.
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
                .assert_stdout_contains("Nothing to clean up");

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

    // ── plan → apply → receipt ──────────────────────────────────────────
    //
    // A cleanup is the only operation whose effects are deletions, and that
    // makes two of its properties unshared: the plan cannot be re-derived (the
    // refs are gone), and the planner has to ask git's own "is it merged"
    // question so it never promises a delete `git branch -d` will refuse. Each
    // of those gets a test.

    /// A branch merged into `main`, and one that is not. The pair is what makes
    /// the tests real: `git branch -d` accepts the first and would refuse the
    /// second, so a single-branch repository can only ever test half of a
    /// cleanup.
    fn one_mergeable_and_one_unmergeable_branch(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.git.run(&["checkout", "-b", "feat-merged"])?;
        env.fs.write_file("merged.txt", "content")?;
        env.git.run(&["add", "."])?;
        env.git.run(&["commit", "-m", "Add feat-merged"])?;
        env.git.run(&["checkout", "main"])?;
        env.git
            .run(&["merge", "--no-ff", "feat-merged", "-m", "Merge feat-merged"])?;

        // Never merged into main, so `-d` refuses it. This is the ordinary case
        // for a feature branch that was promoted and then demoted: it is still
        // unmerged into the base, and `git branch -d` is right to say so.
        env.git.run(&["checkout", "-b", "feat-unmerged"])?;
        env.fs.write_file("unmerged.txt", "content")?;
        env.git.run(&["add", "."])?;
        env.git.run(&["commit", "-m", "Add feat-unmerged"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// The plan does not propose a delete git will refuse.
    ///
    /// It used to: the plan said `delete feat-unmerged`, `git branch -d` refused
    /// at apply time, and the receipt listed the refusal under `Still owed` and
    /// exited 0 — a plan claiming an effect it could have known would not happen,
    /// and an "owed" effect that nothing would ever retry. Whether `-d` accepts a
    /// branch is a question the planner can ask the same way git does (is the tip
    /// an ancestor of the upstream, or of `HEAD`), so it does, and a branch that
    /// fails the test is *kept*: named, with the reason, and never deleted by
    /// hitch. Deleting unmerged work is data loss, so there is no `-D` here.
    #[test]
    fn a_cleanup_plan_keeps_an_unmerged_branch_instead_of_promising_to_delete_it(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            one_mergeable_and_one_unmergeable_branch(env)?;

            let preview = env.hitch.run().args(&["cleanup"]).execute()?;
            let plan = preview.stdout();
            preview.assert_success();
            assert!(plan.contains("delete feat-merged"), "{plan}");
            assert!(
                !plan.contains("delete feat-unmerged"),
                "the plan must not claim a deletion git will refuse: {plan}"
            );
            assert!(
                plan.contains("feat-unmerged")
                    && plan.contains("not merged")
                    && plan.contains("git branch -D feat-unmerged"),
                "it names the kept branch, why, and how to remove it by hand: {plan}"
            );

            let result = env.hitch.run().args(&["cleanup", "--apply"]).execute()?;
            let stdout = result.stdout();
            result.assert_success();

            assert!(
                env.git
                    .run(&["branch", "--list", "feat-merged"])?
                    .stdout()
                    .trim()
                    .is_empty(),
                "the merged branch is gone: {stdout}"
            );
            assert!(
                !env.git
                    .run(&["branch", "--list", "feat-unmerged"])?
                    .stdout()
                    .trim()
                    .is_empty(),
                "the unmerged branch is untouched: {stdout}"
            );
            assert!(
                stdout.contains("delete feat-merged"),
                "the receipt records what it removed: {stdout}"
            );
            assert!(
                !stdout.contains("Still owed"),
                "nothing is owed: a kept branch is a decision, not a debt: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The same cleanup, as a document.
    ///
    /// `commands/cleanup.rs` once printed branch and ref names with `println!`
    /// to **stdout** inside its preview branch, so a `--json` run interleaved
    /// prose into the stream a consumer is trying to parse. This holds that gone
    /// with a `serde_json::from_str` over the whole of stdout, which fails on the
    /// first stray line — and that the kept branch is absent from the effects.
    #[test]
    fn a_cleanup_json_document_is_well_formed_and_excludes_the_kept_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            one_mergeable_and_one_unmergeable_branch(env)?;

            let result = env
                .hitch
                .run()
                .args(&["cleanup", "--apply", "--json"])
                .execute()?;
            let stdout = result.stdout();
            result.assert_success();

            let doc: serde_json::Value = serde_json::from_str(&stdout)
                .map_err(|e| anyhow::anyhow!("stdout is not one JSON document ({e}):\n{stdout}"))?;
            assert_eq!(doc["plan"]["kind"], "Cleanup");
            assert_eq!(doc["receipt"]["operation"], "Cleanup");
            assert_eq!(doc["receipt"]["outcome"], "Applied");
            assert_eq!(
                doc["receipt"]["warnings"]
                    .as_array()
                    .map(Vec::len)
                    .unwrap_or_default(),
                0,
                "nothing is owed: {stdout}"
            );
            assert!(
                !doc["plan"]["effects"].to_string().contains("feat-unmerged"),
                "the plan proposes no deletion of the unmerged branch: {stdout}"
            );
            assert!(
                doc["plan"]["detail"]["unmerged"]
                    .to_string()
                    .contains("feat-unmerged"),
                "and names it as kept: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A delete that fails *at apply time* is a failure, not a debt.
    ///
    /// Nothing retries an "owed" cleanup ref, so listing it under `Still owed`
    /// and exiting 0 promised a follow-up that does not exist. The receipt still
    /// shows what did apply — and the command then exits 1 naming what did not,
    /// with the one command that would try again. A stale ref lock is the cause
    /// used here because it is a refusal the planner cannot foresee.
    #[test]
    fn a_delete_that_fails_at_apply_time_is_reported_after_the_receipt_and_exits_one(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.git.run(&["branch", "feat-a"])?;
            env.git.run(&["branch", "feat-b"])?;
            env.fs.write_file(".git/refs/heads/feat-b.lock", "")?;

            let result = env.hitch.run().args(&["cleanup", "--apply"]).execute()?;
            let stdout = result.stdout();
            let stderr = result.stderr();
            result.assert_failure();
            assert!(
                stdout.contains("✓ delete feat-a"),
                "the receipt shows what applied: {stdout}"
            );
            assert!(
                !stdout.contains("✓ delete feat-b") && !stdout.contains("Still owed"),
                "and does not list the failure as applied or as owed: {stdout}"
            );
            assert!(
                stderr.contains("feat-b") && stderr.contains("hitch cleanup --apply"),
                "the error names what failed and how to try again: {stderr}"
            );
            assert!(
                env.git
                    .run(&["branch", "--list", "feat-a"])?
                    .stdout()
                    .trim()
                    .is_empty(),
                "the other delete was not abandoned"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--json` always yields exactly one document on success, including when
    /// there is nothing to clean.
    #[test]
    fn a_json_cleanup_with_nothing_to_do_still_prints_one_document() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            for args in [
                vec!["cleanup", "--apply", "--json"],
                vec!["cleanup", "--json"],
            ] {
                let result = env.hitch.run().args(&args).execute()?;
                let stdout = result.stdout();
                result.assert_success();
                let doc: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
                    anyhow::anyhow!("{args:?}: stdout is not one JSON document ({e}):\n{stdout}")
                })?;
                assert_eq!(doc["plan"]["kind"], "Cleanup", "{args:?}: {stdout}");
            }
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A cleanup receipt is about refs, so it does not list every environment's
    /// state.
    #[test]
    fn a_cleanup_receipt_has_no_environment_result_block() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.git.run(&["branch", "feat-a"])?;

            let result = env.hitch.run().args(&["cleanup", "--apply"]).execute()?;
            let stdout = result.stdout();
            result.assert_success();
            assert!(stdout.contains("delete feat-a"), "{stdout}");
            assert!(
                !stdout.contains("Result"),
                "environment state is irrelevant to deleting a branch: {stdout}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A branch checked out in a linked worktree is never a candidate.
    ///
    /// `git branch --list` prefixes such a branch with `+ `, which the branch
    /// enumeration did not strip, so the plan said "Kept: + feat" and offered
    /// `git branch -D + feat`. Beyond the parsing, a branch someone is standing on
    /// in another checkout is not stale whether or not it is merged, so it is kept
    /// on purpose and the advisory says where it is checked out.
    #[test]
    fn a_branch_checked_out_in_a_linked_worktree_is_kept_and_named_correctly() -> anyhow::Result<()>
    {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.git.run(&["branch", "feat-merged"])?;
            env.git.run(&["checkout", "-b", "feat-unmerged"])?;
            env.fs.write_file("unmerged.txt", "x")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "unmerged"])?;
            env.git.run(&["checkout", "main"])?;

            let repo_name = env
                .temp_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let parent = env.temp_dir.parent().expect("repo has a parent");
            for branch in ["feat-merged", "feat-unmerged"] {
                let wt = parent.join(format!("{repo_name}-{branch}-wt"));
                env.git
                    .run(&["worktree", "add", &wt.to_string_lossy(), branch])?
                    .assert_success();
            }

            let preview = env.hitch.run().args(&["cleanup"]).execute()?;
            let plan = preview.stdout();
            preview.assert_success();
            assert!(
                !plan.contains("+ feat"),
                "a marker leaked into a name: {plan}"
            );
            assert!(
                !plan.contains("delete feat-"),
                "a checked-out branch is not proposed for deletion: {plan}"
            );
            assert!(
                plan.contains("feat-merged")
                    && plan.contains("feat-unmerged")
                    && plan.contains("checked out"),
                "both are named, with why they are kept: {plan}"
            );
            assert!(
                plan.contains("feat-merged-wt"),
                "and where they are checked out: {plan}"
            );
            assert!(
                !plan.contains("git branch -D feat-merged"),
                "a merged branch is not kept for being unmerged: {plan}"
            );

            env.hitch
                .run()
                .args(&["cleanup", "--apply"])
                .execute()?
                .assert_success();
            for branch in ["feat-merged", "feat-unmerged"] {
                assert!(
                    !env.git
                        .run(&["branch", "--list", branch])?
                        .stdout()
                        .trim()
                        .is_empty(),
                    "{branch} survives the sweep"
                );
            }
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A preview whose only findings are kept branches has nothing to re-run
    /// with `--apply`, so it does not say to.
    #[test]
    fn a_preview_with_nothing_to_delete_does_not_suggest_apply() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.git.run(&["checkout", "-b", "feat-unmerged"])?;
            env.fs.write_file("u.txt", "x")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "u"])?;
            env.git.run(&["checkout", "main"])?;

            let out = env.hitch.run().args(&["cleanup"]).execute()?.stdout();
            assert!(out.contains("Kept: feat-unmerged"), "{out}");
            assert!(!out.contains("re-run with --apply"), "{out}");
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// An apply-time failure is in the `--json` document, typed, so a consumer
    /// need not parse stderr — while the exit code still says it failed.
    #[test]
    fn a_failed_delete_appears_in_the_json_document() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.git.run(&["branch", "feat-a"])?;
            env.git.run(&["branch", "feat-b"])?;
            env.fs.write_file(".git/refs/heads/feat-b.lock", "")?;

            let result = env
                .hitch
                .run()
                .args(&["cleanup", "--apply", "--json"])
                .execute()?;
            let stdout = result.stdout();
            result.assert_failure();
            let doc: serde_json::Value = serde_json::from_str(&stdout)
                .map_err(|e| anyhow::anyhow!("not one JSON document ({e}):\n{stdout}"))?;
            assert_eq!(doc["receipt"]["outcome"], "Applied");
            let failures = doc["failures"].as_array().expect("a failures array");
            assert_eq!(failures.len(), 1, "{stdout}");
            assert_eq!(failures[0]["refname"], "refs/heads/feat-b");
            assert!(
                !failures[0]["cause"].as_str().unwrap_or_default().is_empty(),
                "{stdout}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--env` scopes the archives, not the protections.
    ///
    /// The rule it used to carry scoped *both*: a branch promoted into `dev` was
    /// a candidate under `hitch cleanup --env qa`, because nothing was promoted
    /// into `qa`. That deletes work someone is relying on, and it leaves `dev`'s
    /// declaration naming a ref that no longer exists — the next rebuild of
    /// `dev` fails on it. A branch is a repository-wide object and a flag named
    /// after one environment is not a statement about the others, which is the
    /// same argument that keeps every environment's base branch and every
    /// environment's own branch out of the sweep regardless of scope.
    ///
    /// The archive half is untouched: `--env` still selects whose `prev`/`backup`
    /// refs are pruned, because per-environment retention is the one place an
    /// environment genuinely is the unit.
    #[test]
    fn scoping_the_sweep_does_not_expose_another_environments_branch() -> anyhow::Result<()> {
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

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feature 1"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // Over-plant `qa`'s archives so the scoped run has work, which is
            // what makes the absence of `feature-1` a decision rather than an
            // empty plan.
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
                        &format!("refs/hitch/prev/qa/2020010100{i:04}"),
                        &head,
                    ])?
                    .assert_success();
            }

            let result = env
                .hitch
                .run()
                .args(&["cleanup", "--env", "qa"])
                .execute()?;
            let stdout = result.stdout();
            result.assert_success();
            assert!(
                stdout.contains("delete prev/qa/20200101000000"),
                "qa's archives are in scope: {stdout}"
            );
            assert!(
                !stdout.contains("delete feature-1"),
                "and a branch promoted into a *different* environment is not: {stdout}"
            );
            assert!(
                stdout.contains("feature-1"),
                "it is named as spared rather than silently omitted: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A declared environment's *own* branch is never a candidate.
    ///
    /// This is a bug fix that the plan's "move the candidate collection
    /// verbatim" step would have carried forward: `dev` is not a base branch and
    /// is not promoted into anything, so the filter chain offered to delete it,
    /// and `hitch cleanup --apply` destroyed the build that
    /// `refs/hitch/state/dev` then claimed to describe. Every existing test
    /// missed it because each asserted the presence or absence of one *feature*
    /// branch and none looked at what else the sweep had found.
    ///
    /// The assertion is on the repository, not on the document: "the plan does
    /// not mention `dev`" is true of the bug too, because the bug is that the
    /// plan mentions it as a *deletion*.
    #[test]
    fn a_declared_environments_own_branch_is_never_deleted() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feature 1"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // A real `dev` branch, written by the product rather than planted:
            // this is testing that the sweep spares a live build, not that a ref
            // can be conjured.
            let dev_before = env
                .git
                .run(&["rev-parse", "dev"])?
                .stdout()
                .trim()
                .to_string();
            assert!(
                !dev_before.is_empty(),
                "the promote's rebuild must have produced a dev branch"
            );

            // Enough archive refs that the sweep has work either way, so this
            // cannot pass by a plan simply being empty.
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
                        &format!("refs/hitch/prev/dev/2020010100{i:04}"),
                        &head,
                    ])?
                    .assert_success();
            }

            env.hitch
                .run()
                .args(&["cleanup", "--apply"])
                .execute()?
                .assert_success();

            assert_eq!(
                env.git.run(&["rev-parse", "dev"])?.stdout().trim(),
                dev_before,
                "hitch cleanup deleted a declared environment's own branch — it is \
                 the build refs/hitch/state/dev describes, not an artefact"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The sweep's archive list never contains a `state/` ref, and the branch
    /// list never contains an environment branch.
    ///
    /// `test_cleanup_does_not_prune_the_build_record_ref` holds the first half
    /// from the outside, by planting refs and checking what survived. This one
    /// holds it from the inside — on the *plan*, before anything is deleted —
    /// because the two failures are different: a wrong plan that is never
    /// applied loses nothing, and a wrong plan that is applied is the only way
    /// the outer test can fail at all.
    #[test]
    fn a_cleanup_plan_never_proposes_deleting_a_live_pointer() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feature 1"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // Over-plant the state namespace itself, so the exclusion is being
            // tested against a real temptation rather than against an absence.
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
                        &format!("refs/hitch/state/ghost/2020010100{i:04}"),
                        &head,
                    ])?
                    .assert_success();
            }

            let result = env.hitch.run().args(&["cleanup"]).execute()?;
            let stdout = result.stdout();
            result.assert_success();
            assert!(
                !stdout.contains("refs/hitch/state"),
                "the plan proposes deleting a state ref: {stdout}"
            );
            assert!(
                !stdout.contains("delete dev"),
                "or a declared environment's own branch: {stdout}"
            );

            // And they are all still there, because a preview deletes nothing.
            assert_eq!(
                env.git
                    .run(&["for-each-ref", "refs/hitch/state/ghost"])?
                    .stdout()
                    .lines()
                    .count(),
                15,
                "everything planted under the state namespace survived a preview"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
