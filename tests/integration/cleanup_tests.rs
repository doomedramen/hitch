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
}
