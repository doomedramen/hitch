//! Integration tests for hitch diff command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// `hitch diff <env>` should list the commits each promoted branch adds.
    #[test]
    fn test_diff_shows_commits_per_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a branch with a recognisable commit message
            env.git.run(&["checkout", "-b", "feat-diff-test"])?;
            env.fs.write_file("diff_test.txt", "hello")?;
            env.git.run(&["add", "-f", "diff_test.txt"])?;
            env.git.run(&["commit", "-m", "feat: add diff_test.txt"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feat-diff-test", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["diff", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("feat-diff-test")
                .assert_stdout_contains("feat: add diff_test.txt");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch diff <env> --branch <b>` limits output to that branch.
    #[test]
    fn test_diff_single_branch_filter() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            for branch in &["feat-alpha", "feat-beta"] {
                env.git.run(&["checkout", "-b", branch])?;
                let filename = format!("{}.txt", branch);
                env.fs.write_file(&filename, "x")?;
                env.git.run(&["add", "-f", &filename])?;
                env.git.run(&["commit", "-m", &format!("add {}", branch)])?;
                env.git.run(&["checkout", "main"])?;

                env.hitch
                    .run()
                    .args(&["promote", branch, "dev", "--no-rebuild"])
                    .execute()?
                    .assert_success();
            }

            let result = env
                .hitch
                .run()
                .args(&["diff", "dev", "--branch", "feat-alpha"])
                .execute()?;

            // feat-beta must NOT appear in the filtered output
            let stdout = result.stdout();
            result.assert_success().assert_stdout_contains("feat-alpha");

            assert!(
                !stdout.contains("feat-beta"),
                "--branch filter should exclude feat-beta"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch diff <env> --stat` shows file-level statistics.
    #[test]
    fn test_diff_stat_mode() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feat-stat"])?;
            env.fs.write_file("statfile.txt", "some content")?;
            env.git.run(&["add", "-f", "statfile.txt"])?;
            env.git.run(&["commit", "-m", "add statfile"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feat-stat", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["diff", "dev", "--stat"]).execute()?;

            result
                .assert_success()
                .assert_stdout_contains("feat-stat")
                .assert_stdout_contains("statfile.txt");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// When the env has no promoted branches, a friendly message is shown.
    #[test]
    fn test_diff_empty_env() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["diff", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("no promoted branches");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Passing a branch not in the env returns an error.
    #[test]
    fn test_diff_unknown_branch_fails() -> anyhow::Result<()> {
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
                .args(&["diff", "dev", "--branch", "nonexistent"])
                .execute()?;

            result
                .assert_failure()
                .assert_stderr_contains("not promoted to environment");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
