//! Integration tests for hitch add/remove commands

#[cfg(test)]
mod tests {
    use crate::test_framework::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_add_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add a basic environment (defaults to main branch)
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("declare environment 'dev' on base main");

            // Verify environment was created by reading the configuration
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "main");
            assert!(dev_env.branches.is_empty());
            assert!(!dev_env.is_locked());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_with_source_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;

            // Add environment with custom source branch
            let result = env
                .hitch
                .run()
                .args(&["add", "qa", "--base", "develop"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("declare environment 'qa' on base develop");

            // Verify environment configuration - read from hitch-metadata branch
            let config = env.read_hitch_config()?;
            let qa_env = config.environments.get("qa").unwrap();
            assert_eq!(qa_env.base, "develop");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_invalid_names() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Test that empty name fails (truly invalid case)
            let result = env.hitch.run().args(&["add", ""]).execute();
            result
                .expect("Empty environment name should fail")
                .assert_failure();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_duplicate_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // Try to add same environment again (dev environment was created by setup)
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("already exists");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_nonexistent_source_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Try to add environment with nonexistent source branch
            let result = env
                .hitch
                .run()
                .args(&["add", "dev", "--base", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to add environment without initializing hitch
            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // Remove the environment (dev environment was created by setup)
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("remove environment 'dev' from the declaration");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_with_branches_requires_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            // The dev environment was created by setup, now promote a branch to
            // it. This test used to *create* the branch and never promote it,
            // so `declared.branches` was empty and the refusal it was named for
            // never fired — it documented the absence of the behaviour and its
            // own comment said so. The branch is promoted now, so the
            // question is real.
            promote_a_feature_branch(env)?;

            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("remove environment 'dev' from the declaration");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_with_branches_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            promote_a_feature_branch(env)?;

            // Remove environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["remove", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("remove environment 'dev' from the declaration");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A real feature branch, promoted into `dev` so the declaration has one.
    fn promote_a_feature_branch(env: &TestEnvironment) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", "feature-1"])?;
        env.fs.write_file("test.txt", "content")?;
        env.git.run(&["add", "."])?;
        env.git.run(&["commit", "-m", "Add test file"])?;
        env.git.run(&["checkout", "main"])?;
        env.hitch
            .run()
            .args(&["promote", "feature-1", "dev"])
            .execute()?
            .assert_success();
        Ok(())
    }

    #[test]
    fn test_hitch_remove_locked_environment_requires_force() -> anyhow::Result<()> {
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

            // Try to remove locked environment without force. This used to be a
            // bare `Err` naming `--force`; it is a refusal *in a plan* now, so
            // the reader sees the removal they asked for and why it is held
            // rather than reconstructing either.
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            let out = result.stdout();
            let err = result.stderr();
            result
                .assert_failure()
                .assert_stderr_contains("is currently locked")
                .assert_stderr_contains("--force");
            assert!(
                out.contains("Why this cannot apply"),
                "and it is a plan, not a bare error: {out}"
            );
            assert!(
                out.contains("Current") && !out.contains("Will change"),
                "showing what `dev` currently is, and proposing nothing it will \
                 not do: {out}"
            );
            assert!(
                err.contains("hitch unlock dev"),
                "and naming the move that unblocks it, rather than the command \
                 that just failed: {err}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_locked_environment_force() -> anyhow::Result<()> {
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

            // Remove locked environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["remove", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("remove environment 'dev' from the declaration");

            // Verify environment was removed
            let config = env.read_hitch_config()?;
            assert!(!config.environments.contains_key("dev"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch first
            // Hitch is already initialized by framework

            // Try to remove nonexistent environment
            let result = env.hitch.run().args(&["remove", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_remove_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to remove environment without initializing hitch
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // ── plan → apply → receipt ──────────────────────────────────────────
    //
    // `add`/`remove` are the two halves of one operation, and the interesting
    // cases are the ones where a refusal and a question are the same fact seen
    // from different sides. `remove --force` is the sharpest: it used to exit 1
    // on a branch-bearing environment, which made `--force` a flag that could
    // only turn one failure into another.

    #[test]
    fn a_remove_with_promoted_branches_asks_and_force_answers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            promote_a_feature_branch(env)?;
            let armed = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            // No `--yes`, and nothing to answer the prompt with. That is a
            // *refusal* of the gate, not a decline of the plan — exit 1, so a
            // pipeline cannot read "asked nothing, wrote nothing" as success.
            // The promoted branch is a question rather than a refusal, though:
            // the reader has not been told anything they can agree to disagree
            // with, so the plan says what is at stake and then asks.
            let asked = env
                .hitch
                .run()
                .with_yes(false)
                .args(&["remove", "dev"])
                .execute()?;
            let out = asked.stdout();
            asked
                .assert_failure()
                .assert_stderr_contains("no interactive terminal")
                .assert_stderr_contains("--yes");
            assert!(
                out.contains("still has 1 promoted branch"),
                "and the plan says what is at stake before it asks: {out}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                armed,
                "an unanswered question writes nothing at all"
            );

            // `--force` is the pre-given answer, and the *plan* says so: no
            // question is asked, so the rendered document must not claim one is
            // required. A plan whose `confirmation` is set while the command
            // never asks is a document that lies about its own gate.
            let forced = env
                .hitch
                .run()
                .with_yes(false)
                .args(&["remove", "dev", "--force"])
                .execute()?;
            let out = forced.stdout();
            forced
                .assert_success()
                .assert_stdout_contains("remove environment 'dev' from the declaration");
            assert!(
                !out.contains("Apply this plan?"),
                "…and it does not ask: {out}"
            );
            assert!(
                !env.read_hitch_config()?.environments.contains_key("dev"),
                "so `--force` removes instead of erroring — deviation 3, and the \
                 only exit-code change P8 makes to an existing command"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn adding_an_environment_that_exists_is_refused_in_a_plan() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
            let armed = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            let result = env.hitch.run().args(&["add", "dev"]).execute()?;
            let out = result.stdout();
            let err = result.stderr();
            result
                .assert_failure()
                .assert_stderr_contains("already exists");
            // The plan shows what `dev` *is*, not what the refused `add` would
            // have made it. A `Proposed / dev = main` above a refusal that
            // `dev` is already exactly that is a plan describing an outcome it
            // will not reach.
            assert!(
                out.contains("Current") && !out.contains("Proposed"),
                "a refused add proposes nothing: {out}"
            );
            assert!(
                !out.contains("Will change"),
                "and names no ref to write: {out}"
            );
            assert!(
                err.contains("nothing to declare"),
                "and the remedy is derived from the *difference* — a reader who \
                 named the base it already has has nothing to do, and telling \
                 them to run a `set` would send them to change nothing: {err}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                armed,
                "a refused add must not spend a metadata commit"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn a_dry_run_previews_an_add_and_writes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            let result = env
                .hitch
                .run()
                .args(&["add", "qa", "--base", "main", "--dry-run"])
                .execute()?;
            let out = result.stdout();
            result
                .assert_success()
                .assert_stdout_contains("declare environment 'qa' on base main");
            assert!(
                !out.contains("Applied"),
                "a preview has no receipt half — there was no apply to record: {out}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                before
            );
            assert!(
                !env.read_hitch_config()?.environments.contains_key("qa"),
                "and it did not declare the environment either"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_add_remove_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add multiple environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["add", env_name]).execute()?;
                result.assert_success();
                // Verify environment was created
                let config = env.read_hitch_config()?;
                assert!(config.environments.contains_key(env_name));
            }

            // Remove one environment
            let result = env.hitch.run().args(&["remove", "qa"]).execute()?;
            result.assert_success();

            // Verify remaining environments exist
            let config = env.read_hitch_config()?;
            assert!(config.environments.contains_key("dev"));
            assert!(!config.environments.contains_key("qa"));
            assert!(config.environments.contains_key("staging"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
