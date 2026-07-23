//! Integration tests for hitch conflicts command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// `hitch conflicts <env>` should report a clean environment as such,
    /// without building or locking anything.
    #[test]
    fn test_conflicts_reports_clean_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
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

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["conflicts", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("no conflicts");

            // Read-only: dev must not have been built by this command.
            let dev_exists = env
                .git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/dev"])?
                .success();
            assert!(
                !dev_exists,
                "'hitch conflicts' must not build the environment"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch conflicts <env>` should name every conflicting branch and its
    /// pair, and reflect the environment's on_conflict policy in the message.
    #[test]
    fn test_conflicts_reports_held_branch_and_policy() -> anyhow::Result<()> {
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
            config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: inject branches"])?;
            env.git.run(&["checkout", "main"])?;

            // Colored output interposes ANSI codes between styled spans, so
            // assert the two branch names and the connecting phrase
            // separately rather than as one contiguous substring.
            let result = env.hitch.run().args(&["conflicts", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("branch-b")
                .assert_stdout_contains("conflicts with")
                .assert_stdout_contains("branch-a")
                .assert_stdout_contains("shared.txt")
                // default policy is eject
                .assert_stdout_contains("held");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Regression test: a branch that conflicts with `base` because base
    /// moved on independently *after* the branch diverged must be detected.
    ///
    /// The preflight's `git merge-tree --merge-base` argument must be the
    /// true common ancestor of base and the branch, not base's own current
    /// tip — passing the tip makes merge-tree treat "our" side as having no
    /// changes since the merge-base, so it silently fast-forwards to the
    /// branch's content instead of reporting the conflict. This is the most
    /// common real-world conflict shape (a long-lived feature branch vs. a
    /// base that kept moving), so a regression here would be a preflight
    /// that quietly stops catching the primary case it exists for.
    #[test]
    fn test_conflicts_detects_branch_conflicting_with_moved_base() -> anyhow::Result<()> {
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

            // branch-a diverges from this point...
            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from-branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a: update shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // ...then main moves on with an incompatible change to the same line.
            env.fs.write_file("shared.txt", "from-main-later\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "main: update shared.txt"])?;

            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["conflicts", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("branch-a")
                .assert_stdout_contains("conflicts with")
                .assert_stdout_contains("main")
                .assert_stdout_contains("shared.txt");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
