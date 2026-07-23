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

            // Release the dev environment to main (--yes is injected by the test runner)
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
    fn test_hitch_release_without_yes_refuses_to_prompt() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // No TTY and no --yes: the command must fail fast instead of
            // blocking forever on stdin.
            let result = env
                .hitch
                .run()
                .with_yes(false)
                .args(&["release", "dev", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("no interactive terminal")
                .assert_stderr_contains("--yes");

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

    /// Regression: post-release pruning must not mutate an environment that is locked by
    /// another operation. Its rebuild is skipped while locked, so pruning its metadata would
    /// leave the metadata and the built branch inconsistent (and violates the lock).
    #[test]
    fn test_hitch_release_does_not_prune_from_locked_environments() -> anyhow::Result<()> {
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

            // Branch X promoted to both dev and qa.
            env.git.run(&["checkout", "-b", "feature-x"])?;
            env.fs.write_file("x.txt", "x")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feature x"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-x", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature-x", "qa"])
                .execute()?
                .assert_success();

            // qa is locked by another operation while dev is released.
            env.hitch
                .run()
                .args(&["lock", "qa"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?
                .assert_success();

            let cfg = env.read_hitch_config()?;
            let dev = cfg.environments.get("dev").expect("dev missing");
            let qa = cfg.environments.get("qa").expect("qa missing");

            assert!(
                !dev.branches.contains(&"feature-x".to_string()),
                "released 'dev' should have feature-x pruned"
            );
            assert!(
                qa.branches.contains(&"feature-x".to_string()),
                "locked 'qa' must NOT be pruned (its branch was not rebuilt)"
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

    /// Regression: the environment being released must have its own branch rebuilt
    /// after the release, even though that environment is locked for the duration of
    /// the release. Previously the post-release rebuild skipped it ("Skipping rebuild
    /// of '<env>' because it is locked"), leaving its metadata pruned but its branch
    /// stale against the new base.
    #[test]
    fn test_hitch_release_rebuilds_the_released_environment_even_when_locked() -> anyhow::Result<()>
    {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();

            // Build qa from a promoted feature branch: qa == base + f1.
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("f1.txt", "feature 1")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature 1"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-1", "qa"])
                .execute()?
                .assert_success();

            // Advance main independently so a *stale* qa (base + f1) is distinguishable
            // from a correctly rebuilt qa (base + extra + f1).
            env.fs.write_file("extra.txt", "landed on main directly")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add extra.txt on main"])?;

            // Lock qa to reproduce the state the release itself puts it in.
            env.hitch
                .run()
                .args(&["lock", "qa"])
                .execute()?
                .assert_success();

            // Release qa -> main. feature-1 is pruned from qa, and qa must be rebuilt
            // from the new main (which now also contains extra.txt).
            env.hitch
                .run()
                .args(&["release", "qa", "main", "--force"])
                .execute()?
                .assert_success();

            // The released environment's branch must exist and be fully rebuilt.
            env.git
                .run(&["show-ref", "--verify", "--quiet", "refs/heads/qa"])?
                .assert_success();

            // The independently-landed commit on main must be present in the rebuilt qa
            // branch. A stale (skipped) qa would be missing extra.txt.
            env.git
                .run(&["cat-file", "-e", "qa:extra.txt"])?
                .assert_success();

            // With all promoted branches pruned, qa should be an exact rebuild of main.
            let main_tree = env
                .git
                .run(&["rev-parse", "main^{tree}"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();
            let qa_tree = env
                .git
                .run(&["rev-parse", "qa^{tree}"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                main_tree, qa_tree,
                "released 'qa' branch was not rebuilt to match 'main' (left stale)"
            );

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

    /// Regression: a release that fails partway (a later branch conflicts with a target
    /// that moved after promotion) must be atomic — the target branch must not be left
    /// with the earlier branches' merges committed on it.
    #[test]
    fn test_hitch_release_is_atomic_when_a_later_branch_conflicts() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // A shared file on main that a later branch and main will both edit.
            env.fs.write_file("shared.txt", "line1\nline2\nline3\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add shared.txt"])?;

            // feat-a: clean, unrelated file. Promoted first.
            env.git.run(&["checkout", "-b", "feat-a"])?;
            env.fs.write_file("a.txt", "a")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feat a"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feat-a", "dev"])
                .execute()?
                .assert_success();

            // feat-b: edits shared.txt line2. Compatible with feat-a, so it promotes.
            env.git.run(&["checkout", "-b", "feat-b"])?;
            env.fs
                .write_file("shared.txt", "line1\nB-CHANGE\nline3\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feat b"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feat-b", "dev"])
                .execute()?
                .assert_success();

            // main advances independently, editing the same line so feat-b will conflict
            // at release time (but only after feat-a has already merged cleanly).
            env.fs
                .write_file("shared.txt", "line1\nMAIN-MOVED\nline3\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "main advances"])?;

            let main_before = env
                .git
                .run(&["rev-parse", "main"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();

            // The release must fail on feat-b.
            env.hitch
                .run()
                .args(&["release", "dev", "main", "--force"])
                .execute()?
                .assert_failure();

            // ...and main must be exactly where it was — feat-a must NOT be left merged.
            let main_after = env
                .git
                .run(&["rev-parse", "main"])?
                .assert_success()
                .stdout()
                .trim()
                .to_string();
            assert_eq!(
                main_before, main_after,
                "failed release left 'main' partially merged (not atomic)"
            );
            env.git
                .run(&["cat-file", "-e", "main:a.txt"])?
                .assert_failure(); // feat-a's file must not be on main

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
