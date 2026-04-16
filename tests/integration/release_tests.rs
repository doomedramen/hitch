//! Integration tests for hitch release command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_release_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote feature branches
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

            // Release the dev environment to main (use --force to skip confirmation in tests)
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_preserves_ancestry_for_stacked_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a stacked branch setup:
            // feature-2 is based on feature-1. Only feature-1 is promoted/released.
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("f1.txt", "feature 1")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 1"])?;

            env.git.run(&["checkout", "-b", "feature-2"])?;
            env.fs.write_file("f2.txt", "feature 2")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 2"])?;

            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?
                .assert_success();

            // feature-1 should now be an ancestor of main (merge commit preserves ancestry)
            env.git
                .run(&["merge-base", "--is-ancestor", "feature-1", "main"])?
                .assert_success();

            // feature-2 should only be ahead of main by its own commit; feature-1 shouldn't show up.
            let log = env
                .git
                .run(&["log", "--oneline", "main..feature-2"])?
                .stdout();
            assert!(
                log.contains("Add feature 2"),
                "expected stacked branch to contain its own commit; got:\n{}",
                log
            );
            assert!(
                !log.contains("Add feature 1"),
                "expected released base commit to be part of main ancestry; got:\n{}",
                log
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_prunes_promoted_branches_in_other_envs() -> anyhow::Result<()> {
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

            // Create a feature branch and promote it to multiple environments.
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("f1.txt", "feature 1")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 1"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature-1", "qa"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?
                .assert_success();

            let cfg = env.read_hitch_config()?;
            let dev = cfg
                .environments
                .get("dev")
                .expect("dev environment missing");
            let qa = cfg.environments.get("qa").expect("qa environment missing");

            assert!(
                !dev.branches.contains(&"feature-1".to_string()),
                "expected release to prune feature-1 from dev promotions"
            );
            assert!(
                !qa.branches.contains(&"feature-1".to_string()),
                "expected release to prune feature-1 from qa promotions"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_rebuilds_dependent_environments_in_order() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("f1.txt", "feature 1")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 1"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();

            // qa is based on dev (transitive dependency). This requires the base branch to exist,
            // so we add it after dev has been created by the promote/rebuild.
            env.hitch
                .run()
                .args(&["add", "qa", "--base", "dev"])
                .execute()?
                .assert_success();

            // qa branch should not exist yet (no promotions, no rebuild).
            env.git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/qa"])?
                .assert_failure();

            env.hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?
                .assert_success();

            // Dependent rebuild should create/update qa environment branch.
            env.git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/qa"])?
                .assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to release without initializing hitch
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to release nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["release", "nonexistent", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_nonexistent_target_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to release to nonexistent target branch
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_empty_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment (but no promoted branches)
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Release empty environment
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_with_default_target_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch
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

            // Release without specifying target branch (should use environment's base branch)
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_locked_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, promote branches, and lock it
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

            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Try to release locked environment (should fail)
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("is locked")
                .assert_stderr_contains("--force");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_locked_environment_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, promote branches, and lock it
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

            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Release locked environment with force flag
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            for env_name in ["dev", "qa"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Add branches to each environment
            let env_branches = [("dev", "feature-dev"), ("qa", "feature-qa")];

            for (env_name, branch_name) in env_branches {
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs.write_file(&format!("{}.txt", env_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {} feature", env_name)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", branch_name, env_name])
                    .execute()?;
                result.assert_success();
            }

            // Release each environment to different target branches
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            // Create release branch for qa target
            env.git.run(&["checkout", "-b", "release"])?;
            env.fs.write_file("release.txt", "release content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Create release branch"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["release", "qa", "release", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'qa' released successfully to 'release'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_custom_base_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment with custom base
            // Hitch is already initialized by framework

            // Create develop branch first
            env.git.run(&["checkout", "-b", "develop"])?;
            env.fs.write_file("develop.txt", "develop content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Create develop branch"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["add", "staging", "--base", "develop"])
                .execute()?
                .assert_success();

            // Add and promote feature to staging
            env.git.run(&["checkout", "-b", "feature-staging"])?;
            env.fs.write_file("staging.txt", "staging content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add staging feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-staging", "staging"])
                .execute()?;
            result.assert_success();

            // Release staging to develop (its base branch)
            let result = env
                .hitch
                .run()
                .args(&["release", "staging", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'staging' released successfully to 'develop'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_release_with_conflicts() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create conflicting changes in main branch
            env.fs.write_file("conflict.txt", "main branch content")?;
            env.git.run(&["add", "."])?;
            env.git
                .run(&["commit", "-m", "Add conflicting content to main"])?;

            // Create and promote feature branch with conflicting file
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs
                .write_file("conflict.txt", "feature branch content")?;
            env.fs.write_file("feature.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git
                .run(&["commit", "-m", "Add feature with conflict"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_success();

            // Release should handle conflicts gracefully
            let result = env
                .hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Environment 'dev' released successfully to 'main'");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
