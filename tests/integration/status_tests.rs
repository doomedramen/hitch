//! Integration tests for the `hitch status` command.
//!
//! # Two views, and saying which one a test is reading
//!
//! `hitch status` has two views. The default is the **matrix** — a
//! feature × environment grid plus one summary line per environment — and the
//! per-environment, per-branch detail that predates it moved behind
//! `--environments` with its wording untouched.
//!
//! So every assertion below says which of the two it reads, and a test that
//! wants the detail view puts `--environments` on the command line. Getting
//! that wrong fails loudly rather than silently (an assertion on
//! `Branches (3 promoted)` is simply not present in the matrix output), but
//! there is one class that does *not*: a summary count. `desired 3 · actual 3`
//! reads the same in both, so an assertion on it passes against either view and
//! tests neither. That is why the matrix's own claims are asserted against the
//! matrix, and the detail view's claims against the detail view, in separate
//! tests.
//!
//! # Why the summary assertions collapse whitespace
//!
//! `render_environment_summaries` sizes its name column to the widest
//! environment name rather than to a constant, so `DEV  desired 3` and
//! `DEV      desired 0` are both correct — for different sets of environments.
//! Hardcoding the padding would mean every test that adds an environment
//! silently starts asserting a layout bug, and would be asserting the constant
//! rather than the content. [`assert_summary`] normalises runs of spaces for
//! that reason, and says so where the next reader will see it.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Collapse runs of horizontal whitespace to a single space and trim, so an
    /// assertion about a summary line is about its *content* — the name, the
    /// two counts, the lock marker — and not about the name column's width,
    /// which is a layout decision the matrix is free to make from the data.
    fn normalise(line: &str) -> String {
        line.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Assert that `name`'s summary line is an equation `name = base + ...` with
    /// `branches` terms, optionally with the lock marker.
    ///
    /// Whole-output on failure, because a wrong line is nearly always a
    /// misread view: a detail-view line and a matrix line differ in everything
    /// except the names, so the surrounding output tells the two apart.
    fn assert_summary(stdout: &str, name: &str, branches: usize, locked: bool) {
        let found = stdout.lines().map(normalise).find(|line| {
            line.starts_with(&format!("{name} = "))
                && line.matches(" + ").count() == branches
                && line.ends_with("🔒") == locked
        });
        assert!(
            found.is_some(),
            "expected a summary line for {name} with {branches} branches (locked: {locked}).\n--- full output ---\n{stdout}"
        );
    }

    #[test]
    fn test_hitch_status_on_a_clean_repo_is_plain_words() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let stdout = env
                .hitch
                .run()
                .args(&["status"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();

            assert!(!stdout.contains("DEV"), "names as typed:\n{stdout}");
            assert!(!stdout.contains("desired"), "no model internals:\n{stdout}");
            assert!(!stdout.contains("Feature"), "no empty table:\n{stdout}");
            assert!(
                stdout.contains(
                    "Nothing is promoted yet. Promote a branch with: hitch promote <branch> dev"
                ),
                "{stdout}"
            );
            assert!(
                stdout
                    .lines()
                    .any(|l| l.starts_with("dev = main  ·  up to date  ·  rebuilt ")),
                "{stdout}"
            );
            assert!(!stdout.contains("git branch -a"), "{stdout}");
            assert!(!stdout.contains("Quick commands"), "{stdout}");
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn test_hitch_status_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to get status without initializing hitch
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_empty_configuration() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environments
            // Hitch is already initialized by framework

            // Get status with empty configuration
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No environments configured")
                .assert_stdout_contains(
                    "Use 'hitch add <environment>' to create your first environment",
                );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The default view is a grid, and an environment is a **column**.
    ///
    /// This is the assertion the deleted `📊 1 environments: 1 total, …` rollup
    /// used to make, restated: one column named `DEV`, one summary row, and a
    /// verdict. The rollup's *absence* is asserted too, because a line coming
    /// back would be a second set of totals derived by a second pass — the P3
    /// drift the summary rows exist to prevent.
    #[test]
    fn test_hitch_status_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("Hitch Environment Status")
                // Nothing is promoted, so there is no grid to draw: one
                // sentence names the next step, with the real environment name.
                .assert_stdout_contains("Nothing is promoted yet")
                .assert_stdout_contains("hitch promote <branch> dev")
                .stdout()
                .to_string();

            assert_summary(&stdout, "dev", 0, false);
            // `hitch add` declares an environment without building it, so the
            // environment branch itself does not exist yet. The verdict says
            // that rather than reporting a stale "up to date".
            assert!(
                stdout.contains("branch missing"),
                "a declared-but-unbuilt environment has no branch. Got:\n{stdout}"
            );
            assert!(
                !stdout.contains("environments:"),
                "the rollup line should be gone; the summary rows replace it. Got:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The detail view keeps its own wording, unchanged.
    ///
    /// Deliberately a separate test from `test_hitch_status_basic`: the two
    /// views are allowed to drift apart from here on, and a single test checking
    /// both would pass on either one's output alone.
    #[test]
    fn test_hitch_status_environments_detail_view_is_unchanged() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("base:")
                .assert_stdout_contains("Branches (0 promoted)")
                .assert_stdout_contains("Environment is unlocked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--environments <NAME>` narrows the detail view to one environment, and a
    /// name hitch does not have is an error that lists the ones it does.
    ///
    /// `--environments [NAME]` takes an *optional* value because clap has no
    /// optional-value form for `Option<T>`, so `hitch status --environments qa`
    /// is the only way a name can arrive, and the test that would catch a
    /// mis-parse is this one.
    #[test]
    fn test_hitch_status_environments_scopes_to_one_name() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            for name in ["dev", "qa"] {
                env.hitch
                    .run()
                    .args(&["add", name])
                    .execute()?
                    .assert_success();
            }

            let scoped = env
                .hitch
                .run()
                .args(&["status", "--environments", "qa"])
                .execute()?;
            let stdout = scoped
                .assert_success()
                .assert_stdout_contains("base:")
                .stdout()
                .to_string();
            assert!(
                !stdout.contains("┌─ dev"),
                "`--environments qa` must show qa only. Got:\n{stdout}"
            );

            let missing = env
                .hitch
                .run()
                .args(&["status", "--environments", "nope"])
                .execute()?;
            missing
                .assert_failure()
                .assert_stderr_contains("nope")
                .assert_stderr_contains("dev")
                .assert_stderr_contains("qa");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote feature branches
            for i in 1..=3 {
                let branch_name = format!("feature-{}", i);
                env.git.run(&["checkout", "-b", &branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", i), &format!("content {}", i))?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", &branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // The matrix: three feature rows under one environment column, each
            // included, and the summary counting them.
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("feature-1")
                .assert_stdout_contains("feature-2")
                .assert_stdout_contains("feature-3")
                .assert_stdout_contains("● included")
                .assert_stdout_contains("up to date")
                .stdout()
                .to_string();
            assert_summary(&stdout, "dev", 3, false);

            // The detail view still reports the promoted-branch count.
            let detail = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            detail
                .assert_success()
                .assert_stdout_contains("Branches (3 promoted)");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A lock is not a composition fact, so it has no cell. It rides the
    /// environment's summary row instead — on that row rather than in a
    /// repository-wide total, because a total cannot say *which* environment
    /// is locked, and a lock is the one fact here that is per-environment.
    #[test]
    fn test_hitch_status_with_locked_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Get status with locked environment
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result.assert_success().stdout().to_string();
            assert_summary(&stdout, "dev", 0, true);
            // And only that row carries it: with one environment, a lock
            // anywhere else in the block would show up in the same assertion, so
            // this pins the count rather than the presence.
            assert_eq!(
                stdout.matches('🔒').count(),
                1,
                "the lock marker belongs on dev's summary row and nowhere else. Got:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_rebuilt_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, promote branch, and rebuild
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // The matrix says "up to date" and counts the branch. The rebuild
            // *timestamp* is a detail-view fact — the matrix deliberately shows
            // no clock, because no verdict in it depends on one.
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("feature-1")
                .assert_stdout_contains("● included")
                .assert_stdout_contains("up to date")
                .stdout()
                .to_string();
            assert_summary(&stdout, "dev", 1, false);

            let detail = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            detail.assert_success().assert_stdout_contains("Rebuilt:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Three environments are three **columns**, which is the shape the whole
    /// view exists for: one promoted feature read across every place it is
    /// declared, in a single block. The old view asserted
    /// `Branches (1 promoted)`, `Branches (0 promoted)`, `Branches (0 promoted)`
    /// in sequence and let the reader infer the grid from three separate
    /// sections.
    #[test]
    fn test_hitch_status_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Add branches to dev
            env.git.run(&["checkout", "-b", "feature-dev"])?;
            env.fs.write_file("dev.txt", "dev content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add dev feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-dev", "dev"])
                .execute()?;
            result.assert_success();

            // Lock qa environment
            env.hitch
                .run()
                .args(&["lock", "qa"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("Feature")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("qa")
                .assert_stdout_contains("staging")
                .assert_stdout_contains("feature-dev")
                .stdout()
                .to_string();

            assert_summary(&stdout, "dev", 1, false);
            assert_summary(&stdout, "qa", 0, true);
            assert_summary(&stdout, "staging", 0, false);
            assert!(
                !stdout.contains("environments:"),
                "the rollup line should be gone. Got:\n{stdout}"
            );
            // The lock is on qa's row and no other row's, so exactly one
            // marker: the same count assertion as the single-environment test,
            // here with two unlocked rows to be wrong about.
            assert_eq!(
                stdout.matches('🔒').count(),
                1,
                "only qa is locked. Got:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_verbose() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Get verbose status
            let result = env.hitch.run().args(&["status", "--verbose"]).execute()?;
            result.assert_success();

            // Verbose mode should show additional debug information
            // Note: We can't easily test the exact verbose output without exposing internal logging
            // But we can verify it doesn't fail and produces output

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_complex_scenario() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            // Add environments with different configurations
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

            // Create qa branch first for staging to use as base
            env.git.run(&["checkout", "-b", "qa"])?;
            env.fs.write_file("qa.txt", "qa content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Create qa branch"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["add", "staging", "--base", "qa"])
                .execute()?
                .assert_success();

            // Add multiple branches to dev
            for i in 1..=2 {
                let branch_name = format!("feature-{}", i);
                env.git.run(&["checkout", "-b", &branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", i), &format!("content {}", i))?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", &branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Add one branch to qa
            env.git.run(&["checkout", "-b", "feature-qa"])?;
            env.fs.write_file("qa.txt", "qa content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add qa feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-qa", "qa"])
                .execute()?;
            result.assert_success();

            // Lock staging environment
            env.hitch
                .run()
                .args(&["lock", "staging"])
                .execute()?
                .assert_success();

            // Rebuild dev environment
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // The grid, and each environment's own counts on its own row.
            // `staging` is based on `qa` rather than `main`, and its base does
            // not exist as a *promoted* branch, so it is the one environment
            // that stays `branch missing` — which is why this test also pins
            // that verdict rather than only counting.
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("Feature")
                .stdout()
                .to_string();
            assert_summary(&stdout, "dev", 2, false);
            assert_summary(&stdout, "qa", 1, false);
            assert_summary(&stdout, "staging", 0, true);

            // The detail view keeps the bases and the rebuild stamp.
            let detail = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            detail
                .assert_success()
                .assert_stdout_contains("base:")
                .assert_stdout_contains("main")
                .assert_stdout_contains("Branches (2 promoted)")
                .assert_stdout_contains("Branches (1 promoted)")
                .assert_stdout_contains("Branches (0 promoted)")
                .assert_stdout_contains("Rebuilt:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_status_with_git_state() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create uncommitted changes (dirty working directory)
            env.fs
                .write_file("uncommitted.txt", "uncommitted changes")?;

            // Status should still work even with unclean git state
            let result = env.hitch.run().args(&["status"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch Environment Status")
                .assert_stdout_contains("dev = main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Was `#[ignore]`d as "timing-sensitive: relies on git commit timestamps
    /// being newer than rebuild timestamp". That dependency *was* the bug — the
    /// verdict used to come from comparing a commit's date against a wall-clock
    /// `rebuilt_at`, so a test could only pass by sleeping long enough for the
    /// clock to tick. The verdict is now a SHA comparison against the build
    /// record, so neither the assertion nor the test needs a clock, and the
    /// `sleep(2)` that bought a second is gone.
    ///
    /// Split across the two views deliberately. The matrix carries the *verdict*
    /// and the detail view carries the *name* of the input that moved; naming
    /// what moved is a fact about one environment, and the grid is a fact about
    /// the shape of the repository.
    #[test]
    fn test_hitch_status_detects_base_branch_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add an environment with base branch "main"
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Rebuild, which pins main's current SHA into the build record.
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            // Make a new commit directly to main (simulating an external merge)
            env.fs.write_file("external.txt", "external change")?;
            env.git.run(&["add", "-f", "external.txt"])?;
            env.git.run(&["commit", "-m", "External change to main"])?;

            // The matrix: the verdict, and no clock.
            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("needs rebuild")
                .stdout()
                .to_string();
            assert_summary(&stdout, "dev", 0, false);

            // The name of what moved, in the form spec §11.2 asks for: an
            // explicit before → after rather than a bare verdict. This is a
            // stronger assertion than the one it replaces — the old one accepted
            // any of two loosely-related strings and would have passed against
            // output naming the wrong branch.
            let result = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("has new commits")
                .stdout()
                .to_string();
            assert!(
                stdout.contains("main"),
                "Expected status to name main. Got:\n{stdout}"
            );
            assert_eq!(
                stdout.matches('\u{2192}').count(),
                1,
                "Exactly one input moved, so exactly one from \u{2192} to arrow. Got:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Un-ignored for the same reason as
    /// `test_hitch_status_detects_base_branch_changes`: the clock dependency
    /// was the defect, not an environmental hazard.
    ///
    /// The matrix is where the repository-wide claim lives — *two* environments
    /// are behind — and it is made from the rows rather than from a second pass,
    /// so counting the verdicts is a real check that the two columns and the two
    /// rows agree.
    #[test]
    fn test_hitch_status_multiple_envs_with_changed_base() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add multiple environments (dev, qa) using the same base branch "main"
            for env_name in ["dev", "qa"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Rebuild both environments
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "qa"]).execute()?;
            result.assert_success();

            // Make a new commit to main. No sleep, and no wait for a clock to
            // tick: both environments' records pinned main's SHA at their own
            // rebuild, and this commit moves it for both of them.
            env.fs.write_file("external.txt", "external change")?;
            env.git.run(&["add", "-f", "external.txt"])?;
            env.git.run(&["commit", "-m", "External change to main"])?;

            let result = env.hitch.run().args(&["status"]).execute()?;
            let stdout = result.assert_success().stdout().to_string();
            assert_summary(&stdout, "dev", 0, false);
            assert_summary(&stdout, "qa", 0, false);
            // Once per summary row's verdict line, and nowhere else: the
            // suggested-actions block prompts with commands (`hitch rebuild dev`)
            // rather than repeating the verdict, so this counts verdicts.
            assert_eq!(
                stdout.matches("needs rebuild").count(),
                2,
                "both dev and qa share main as a base, so both must read needs rebuild. Got:\n{stdout}"
            );
            // Assert the command hints, not the labels: the suggestion list
            // colourises the environment name with ANSI escapes, so the literal
            // text "Rebuild dev" never appears even though the row does.
            assert!(
                stdout.contains("hitch rebuild dev") && stdout.contains("hitch rebuild qa"),
                "and both environments should get a rebuild hint. Got:\n{stdout}"
            );

            // The detail view names the input, once per environment.
            let result = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("has new commits")
                .stdout()
                .to_string();
            assert!(
                stdout.contains("main"),
                "Expected status to name main. Got:\n{stdout}"
            );
            assert_eq!(
                stdout.matches('\u{2192}').count(),
                2,
                "both dev and qa share main as a base, so both must report it moved. Got:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// After promoting a branch and then committing to it, the detail view
    /// should show a staleness indicator ("new commits since last rebuild").
    ///
    /// Asserted against the **detail view**, and the reason is worth recording
    /// because the matrix assertion at the end of this test looks like it
    /// contradicts it. In the matrix this branch's cell reads `included` — it
    /// genuinely *is* in the last build — while the row beneath reads `needs
    /// rebuild`, because that build predates these commits. A cell answers "what
    /// is in the build"; a row answers "is the build current". The
    /// reconciliation is the summary row, and `hitch why <branch> <environment>`
    /// is the form that names the moving commit. Asserting the branch-level
    /// staleness wording against the matrix would assert that a cell re-derives a
    /// verdict it deliberately does not carry.
    #[test]
    fn test_status_shows_per_branch_staleness() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch.
            // Use -f to bypass the broad .gitignore inherited from hitch-metadata.
            env.git.run(&["checkout", "-b", "stale-feature"])?;
            env.fs.write_file("stale.txt", "v1")?;
            env.git.run(&["add", "-f", "stale.txt"])?;
            env.git.run(&["commit", "-m", "Initial feature commit"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "stale-feature", "dev"])
                .execute()?
                .assert_success();

            // Add a new commit to the branch AFTER promotion (making it stale).
            //
            // This used to carry an explicit future author-date
            // (`--date 2099-01-01T00:00:00+00:00`) so the commit's timestamp
            // would outrank the rebuild's `rebuilt_at` however fast the test ran.
            // That hack is exactly the bug: a staleness check that needs a faked
            // clock to notice a *content* change is not reading content. The
            // record pins the branch's SHA, so an ordinary commit is now enough
            // and the faked date is gone.
            env.git.run(&["checkout", "stale-feature"])?;
            env.fs.write_file("stale.txt", "v2 - new content")?;
            env.git.run(&["add", "-f", "stale.txt"])?;
            env.git
                .run(&["commit", "-m", "Update after promotion"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?;

            // The detail view: the branch-level staleness wording.
            let result = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            let stdout = result.assert_success().stdout().to_string();

            assert!(
                stdout.contains("new commits since last rebuild"),
                "Expected staleness indicator for stale-feature. Got:\n{stdout}"
            );

            // The matrix: the environment is behind, without naming the commit.
            // Both halves of the cell/row distinction, asserted together so the
            // pair cannot drift.
            let matrix = env.hitch.run().args(&["status"]).execute()?;
            matrix
                .assert_success()
                .assert_stdout_contains("stale-feature")
                .assert_stdout_contains("● included")
                .assert_stdout_contains("needs rebuild");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch status --environments` should flag a branch that *would* be held
    /// on the next rebuild (⛔), without fetching or building anything.
    ///
    /// This repo has never been built, so there is no record and the ⛔ comes
    /// from the local preflight — a prediction. It is worded as one: "would be
    /// held on the next rebuild". See
    /// `test_status_distinguishes_a_held_branch_from_one_that_would_be_held` in
    /// `state_model_tests.rs` for the record-backed (fact) wording, which used
    /// to be conflated with this one.
    ///
    /// The matrix deliberately has no such column, and the last assertion is the
    /// one holding that line. A prediction has no business in a grid of facts:
    /// a cell that could read "would be held" would put a `⛔` beside a `⛔`
    /// meaning "was held", with nothing to tell them apart, and it would be a
    /// third caller of `predict_composition` — the call site
    /// count that P7's Global Constraints fix at two (`status --environments`
    /// and `tree`).
    #[test]
    fn test_status_shows_held_branch_glyph() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base content\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "Add shared.txt"])?;

            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from branch-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-b: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Inject directly into metadata (bypass the promote gate, which
            // would otherwise refuse to promote a conflicting sibling).
            env.git.run(&["checkout", "hitch-metadata"])?;
            let config_str = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
            config["environments"]["dev"]["branches"] =
                serde_json::json!(["branch-a", "branch-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: inject branches"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("⛔")
                .assert_stdout_contains("branch-b")
                // A prediction, so it is worded as one. The previous phrasing,
                // "held on rebuild", read as a statement about the branch's
                // current state when it was describing the *next* build.
                .assert_stdout_contains("would be held on the next rebuild")
                .stdout()
                .to_string();
            // No build has happened, so nothing may claim a past one.
            assert!(
                !stdout.contains("held in the last build"),
                "nothing was built, so no branch can have been held in a build. Got:\n{stdout}"
            );

            // The matrix shows the two branches as *not built yet* — which is
            // the fact — and carries no prediction at all.
            let matrix = env.hitch.run().args(&["status"]).execute()?;
            let matrix_stdout = matrix
                .assert_success()
                .assert_stdout_contains("branch-a")
                .assert_stdout_contains("branch-b")
                .assert_stdout_contains("needs rebuild")
                .stdout()
                .to_string();
            assert!(
                !matrix_stdout.contains("would be held"),
                "the matrix carries facts only, so it must not predict a hold. Got:\n{matrix_stdout}"
            );
            assert!(
                !matrix_stdout.contains('⛔'),
                "and so it must not show a hold glyph for a build that has not run. Got:\n{matrix_stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--json` emits one envelope carrying the matrix and the per-environment
    /// models, and nothing else. A JSON consumer is a program, and a value it has
    /// to parse a sentence out of is a value whose wording it has to pin.
    /// Every string *value* and every object key under `value` that looks like
    /// a Rust type or variant name.
    ///
    /// Recursive on purpose. The claim being tested is about the whole envelope,
    /// so a shallow scan would pass while the one enum that actually needed the
    /// rename kept its default. Deliberately over-eager about the pattern — a
    /// false positive costs one extra rename, a false negative costs a silent
    /// break for a consumer.
    fn collect_pascal_case_tokens(value: &serde_json::Value, into: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => {
                if s.chars().next().is_some_and(char::is_uppercase)
                    && s.contains(char::is_alphabetic)
                {
                    into.push(s.clone());
                }
            }
            serde_json::Value::Object(map) => {
                for (key, inner) in map {
                    if key.chars().next().is_some_and(char::is_uppercase) {
                        into.push(key.clone());
                    }
                    collect_pascal_case_tokens(inner, into);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect_pascal_case_tokens(item, into);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn test_hitch_status_json_is_a_document_not_prose() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["status", "--json"]).execute()?;
            let document: serde_json::Value = result
                .assert_success()
                .stdout()
                .parse()
                .map_err(|e| anyhow::anyhow!("status --json was not JSON: {e}"))?;

            assert_eq!(
                document["schema_version"], 1,
                "the envelope's version is the only sanctioned way to change its shape. Got:\n{document}"
            );
            let status = &document["status"];
            assert!(
                status["matrix"]["columns"].is_array(),
                "the matrix is part of the document. Got:\n{document}"
            );
            assert!(
                status["environments"].is_array(),
                "so is the per-environment model. Got:\n{document}"
            );
            // Columns are the environments, in the snapshot's own (name-sorted)
            // order, as bare names. Asserted here because a column set that
            // reordered between runs would make the document non-reproducible,
            // and this is the only place a consumer can see the order.
            assert_eq!(
                status["matrix"]["columns"][0], "dev",
                "columns are the environments, in snapshot order. Got:\n{document}"
            );
            assert_eq!(
                status["matrix"]["columns"]
                    .as_array()
                    .expect("columns is an array")
                    .len(),
                1,
                "one environment, one column. Got:\n{document}"
            );

            // Every enum in the envelope is `snake_case`. This is a wire
            // contract, so a Rust variant name must not leak into it: a
            // consumer that matched on `"ActualUnknown"` would break silently
            // the first time the variant was renamed, and `serde`'s default
            // derives exactly that. Asserted by scanning the whole document
            // rather than by naming one field, because the point is that *no*
            // enum in here is PascalCase — including the ones that reached the
            // envelope by accident (`ActualComposition`,
            // `EnvironmentHealth`), which are the ones that would be missed.
            let mut pascal = Vec::new();
            collect_pascal_case_tokens(&document, &mut pascal);
            assert!(
                pascal.is_empty(),
                "a Rust type name leaked into the JSON contract: {pascal:?}\nGot:\n{document}"
            );

            // And the one enum a consumer will match on most, spelled out, so
            // the rename itself is pinned rather than only its absence of
            // capital letters. `missing_branch` and not `MissingBranch`,
            // because `hitch add` declares an environment without building it —
            // the same reason the matrix test above expects a missing branch.
            assert_eq!(
                status["matrix"]["summaries"][0]["health"], "missing_branch",
                "Got:\n{document}"
            );
            assert_eq!(
                status["environments"][0]["state"]["actual"], "legacy_unknown",
                "and the composition is named the same way. Got:\n{document}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--verbose` is a global flag; the command must honour it on the context
    /// even when its own copy of the flag is false (a caller that builds the
    /// context directly, as here, never goes through clap's propagation).
    #[test]
    fn a_global_verbose_context_is_honoured_by_status() -> anyhow::Result<()> {
        use clap::Parser;
        use hitch::commands::global_context::{GlobalContext, GlobalFlags};
        use hitch::utils::logging::Logger;
        use hitch::utils::output::BufferedOutputSink;
        use std::sync::Arc;

        #[derive(Parser)]
        struct Wrapper {
            #[command(flatten)]
            command: hitch::commands::status::StatusCommand,
        }

        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            let mut context = GlobalContext::new_at_path(
                env.temp_dir.to_str().expect("utf-8 temp dir"),
                GlobalFlags {
                    verbose: true,
                    no_push: true,
                    assume_yes: true,
                    json: false,
                },
                Arc::new(Logger::new()),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            let sink = BufferedOutputSink::new();
            context.output = sink.clone();

            let command = Wrapper::parse_from(["w"]).command;
            hitch::commands::status::run(command, &context)?;

            let lines: Vec<String> = sink.snapshot().into_iter().map(|l| l.message).collect();
            assert!(
                lines.iter().any(|l| l == "Starting status command..."),
                "the global verbose flag was ignored: {lines:?}"
            );
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }
}
