//! Integration tests for hitch promote/demote commands

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use serde_json;

    #[test]
    fn test_hitch_promote_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a feature branch with commits
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote the feature branch to dev environment
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("promote feature-1 into 'dev' (now: feature-1)");

            // Verify branch was promoted in environment configuration
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.branches.contains(&"feature-1".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_demote_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote a branch
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

            // Demote the branch from dev environment
            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-1", "dev"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("demote feature-1 out of 'dev' (now: nothing)");

            // Verify branch was demoted from environment configuration
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.branches.contains(&"feature-1".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_promote_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to promote without initializing hitch
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_demote_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to demote without initializing hitch
            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-1", "dev"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_promote_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to promote to nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_demote_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to demote from nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-1", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_promote_demote_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote multiple branches
            let branches = vec!["feature-1", "feature-2", "feature-3"];

            for branch_name in &branches {
                // Create feature branch
                env.git.run(&["checkout", "-b", branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", branch_name), "content")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add {}", branch_name)])?;
                env.git.run(&["checkout", "main"])?;

                // Promote to dev environment
                let result = env
                    .hitch
                    .run()
                    .args(&["promote", branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Verify all branches are promoted
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.branches.len(), 3);

            // Demote one branch
            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-2", "dev"])
                .execute()?;
            result.assert_success();

            // Verify only one branch was demoted
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.branches.len(), 2);
            assert!(dev_env.branches.contains(&"feature-1".to_string()));
            assert!(!dev_env.branches.contains(&"feature-2".to_string()));
            assert!(dev_env.branches.contains(&"feature-3".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_successful_promote_after_rollback() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a feature branch with commits
            env.git
                .run(&["checkout", "-b", "feature-success-after-rollback"])?;
            env.fs
                .write_file("feature.txt", "feature for success test")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Test that we can successfully promote this branch
            let success_result = env
                .hitch
                .run()
                .args(&["promote", "feature-success-after-rollback", "dev"])
                .execute()?;
            success_result
                .assert_success()
                .assert_stdout_contains("promote feature-success-after-rollback into 'dev'");

            // Verify branch was promoted
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env
                .branches
                .contains(&"feature-success-after-rollback".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_successful_demote_after_rollback() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch first
            env.git.run(&["checkout", "-b", "feature-demote-success"])?;
            env.fs
                .write_file("feature.txt", "feature for successful demote test")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote the feature branch successfully
            env.hitch
                .run()
                .args(&["promote", "feature-demote-success", "dev"])
                .execute()?
                .assert_success();

            // Verify branch was promoted
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env
                .branches
                .contains(&"feature-demote-success".to_string()));

            // Now demote should succeed
            let success_result = env
                .hitch
                .run()
                .args(&["demote", "feature-demote-success", "dev"])
                .execute()?;
            success_result
                .assert_success()
                .assert_stdout_contains("demote feature-demote-success out of 'dev'");

            // Verify branch was demoted
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env
                .branches
                .contains(&"feature-demote-success".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A promote whose *rebuild* fails is not a failed promote.
    ///
    /// The declaration edit is the operation's durable effect and it has
    /// landed; what did not happen is the build it forces. Reverting the
    /// declaration would throw away what the user asked for on the grounds of a
    /// downstream failure — and it would restore a whole-config snapshot, so it
    /// would also revert anything else that wrote `hitch.json` in between
    /// (including a rebuild that failed *after* moving the environment branch,
    /// which is the case where reverting the declaration would leave the branch
    /// holding code the declaration says should not be there).
    ///
    /// So the contract is: exit 0, the declaration persists, the environment is
    /// left unbuilt, and the output names the command that finishes the job.
    #[test]
    fn test_promote_whose_rebuild_fails_keeps_the_declaration() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a feature branch with commits
            env.git
                .run(&["checkout", "-b", "feature-declared-not-built"])?;
            env.fs
                .write_file("feature.txt", "the branch survives a failed build")?;
            env.git.run(&["add", "-f", "feature.txt"])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Point dev at a base branch that does not exist anywhere, so the
            // rebuild cannot compose.
            env.git.run(&["checkout", "hitch-metadata"])?;
            let mut config = env.read_hitch_config()?;
            config.environments.get_mut("dev").unwrap().base =
                "definitely-nonexistent-base-branch-99999".to_string();
            let config_json = serde_json::to_string_pretty(&config)?;
            env.fs.write_file("hitch.json", &config_json)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&[
                "commit",
                "-m",
                "Change dev environment to use non-existent base branch",
            ])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-declared-not-built", "dev"])
                .execute()?;

            // Not a failure: the promote's own effect landed.
            result
                .assert_success()
                .assert_stdout_contains("promote feature-declared-not-built into 'dev'")
                // The unbuilt half is reported as owed work, with the command
                // that settles it.
                .assert_stdout_contains("hitch rebuild dev");

            // The declaration kept the branch. This is the assertion the old
            // test inverted, and it is the whole point.
            let after = env.read_hitch_config()?;
            assert!(
                after
                    .environments
                    .get("dev")
                    .unwrap()
                    .branches
                    .contains(&"feature-declared-not-built".to_string()),
                "the declaration must keep a promotion whose rebuild failed"
            );

            // And the environment branch was never built, so there is nothing
            // claiming otherwise: `hitch status` will call this out.
            env.assert.git_branch_not_exists(&env.git, "dev")?;

            // The recovery is exactly what the message says it is.
            env.git.run(&["checkout", "hitch-metadata"])?;
            let mut config = env.read_hitch_config()?;
            config.environments.get_mut("dev").unwrap().base = "main".to_string();
            let config_json = serde_json::to_string_pretty(&config)?;
            env.fs.write_file("hitch.json", &config_json)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git
                .run(&["commit", "-m", "Fix dev environment base branch"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            env.assert.git_branch_exists(&env.git, "dev")?;

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Item 1: Pre-promote conflict check against sibling branches
    // -------------------------------------------------------------------------

    /// When two branches both modify the same file in incompatible ways, promoting
    /// the second branch should be blocked before any state is mutated.
    #[test]
    fn test_promote_blocked_by_sibling_conflict() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a shared file on main so both branches have a common ancestor.
            // Use -f to bypass the broad .gitignore inherited from hitch-metadata.
            env.fs.write_file("shared.txt", "line one\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "Add shared.txt"])?;

            // Branch A: replaces line one with "from branch-a"
            env.git.run(&["checkout", "-b", "branch-a"])?;
            env.fs.write_file("shared.txt", "from branch-a\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-a changes shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Branch B: replaces line one with "from branch-b" (incompatible)
            env.git.run(&["checkout", "-b", "branch-b"])?;
            env.fs.write_file("shared.txt", "from branch-b\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", "branch-b changes shared.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote branch-a first – should succeed
            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev"])
                .execute()?
                .assert_success();

            // Attempt to promote branch-b – should be blocked
            let result = env
                .hitch
                .run()
                .args(&["promote", "branch-b", "dev"])
                .execute()?;

            result
                .assert_failure()
                .assert_stderr_contains("compatibility check failed")
                .assert_stderr_contains("branch-b conflicts with main")
                .assert_stderr_contains("shared.txt");

            // Metadata must be unchanged (branch-b not in the list)
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.branches.contains(&"branch-a".to_string()));
            assert!(!dev_env.branches.contains(&"branch-b".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// When two branches modify different files they should both be promotable
    /// with no conflict errors.
    #[test]
    fn test_promote_succeeds_with_non_conflicting_siblings() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Branch A touches file-a.txt only
            env.git.run(&["checkout", "-b", "feat-a"])?;
            env.fs.write_file("file-a.txt", "content a\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feat-a adds file-a.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Branch B touches file-b.txt only
            env.git.run(&["checkout", "-b", "feat-b"])?;
            env.fs.write_file("file-b.txt", "content b\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feat-b adds file-b.txt"])?;
            env.git.run(&["checkout", "main"])?;

            // Both promotes should succeed
            env.hitch
                .run()
                .args(&["promote", "feat-a", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["promote", "feat-b", "dev"])
                .execute()?
                .assert_success();

            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.branches.contains(&"feat-a".to_string()));
            assert!(dev_env.branches.contains(&"feat-b".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The demote counterpart of
    /// `test_promote_whose_rebuild_fails_keeps_the_declaration`. A failed
    /// dependent rebuild is owed work, not a failed declaration edit.
    #[test]
    fn test_demote_whose_rebuild_fails_keeps_the_declaration() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch first
            env.git
                .run(&["checkout", "-b", "feature-to-demote-functional"])?;
            env.fs
                .write_file("feature.txt", "feature for demote functional test")?;
            env.git.run(&["add", "."])?;
            env.git
                .run(&["commit", "-m", "Add feature for demote functional test"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote the feature branch successfully first
            env.hitch
                .run()
                .args(&["promote", "feature-to-demote-functional", "dev"])
                .execute()?
                .assert_success();

            // Verify branch was promoted
            let promoted_config = env.read_hitch_config()?;
            let promoted_dev_env = promoted_config.environments.get("dev").unwrap();
            assert!(promoted_dev_env
                .branches
                .contains(&"feature-to-demote-functional".to_string()));

            // Break the base branch so the rebuild cannot compose. Unlike the
            // promote counterpart, `dev` here *already exists* and already
            // contains this branch — which is exactly why reverting the
            // declaration would be the wrong repair. The state the rollback used
            // to produce is "declaration says the branch is promoted, and the
            // environment branch genuinely contains it", i.e. a state with
            // nothing wrong in it that the user would have no way to distinguish
            // from success.
            env.git.run(&["checkout", "hitch-metadata"])?;
            let mut config = env.read_hitch_config()?;
            config.environments.get_mut("dev").unwrap().base =
                "definitely-nonexistent-base-branch-88888".to_string();
            let config_json = serde_json::to_string_pretty(&config)?;
            env.fs.write_file("hitch.json", &config_json)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&[
                "commit",
                "-m",
                "Change dev environment to use non-existent base branch for demote",
            ])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-to-demote-functional", "dev"])
                .execute()?;

            result
                .assert_success()
                .assert_stdout_contains("demote feature-to-demote-functional out of 'dev'")
                .assert_stdout_contains("hitch rebuild dev");

            // The declaration lost the branch, and the environment branch is
            // left describing the *old* declaration. `hitch status` compares the
            // two and reports exactly that, which is what makes the state
            // recoverable instead of merely surprising.
            let after = env.read_hitch_config()?;
            assert!(
                !after
                    .environments
                    .get("dev")
                    .unwrap()
                    .branches
                    .contains(&"feature-to-demote-functional".to_string()),
                "the demotion itself is the durable effect and must persist"
            );

            // Fix the base and run the command the message named.
            env.git.run(&["checkout", "hitch-metadata"])?;
            let mut config = env.read_hitch_config()?;
            config.environments.get_mut("dev").unwrap().base = "main".to_string();
            let config_json = serde_json::to_string_pretty(&config)?;
            env.fs.write_file("hitch.json", &config_json)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git
                .run(&["commit", "-m", "Fix dev environment base branch for demote"])?;
            env.git.run(&["checkout", "main"])?;

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let final_config = env.read_hitch_config()?;
            assert!(!final_config
                .environments
                .get("dev")
                .unwrap()
                .branches
                .contains(&"feature-to-demote-functional".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // One voice between the plan and the receipt
    // -------------------------------------------------------------------------

    /// Write branches into `hitch.json` without going through `promote`, which
    /// is what lets a genuinely conflicting pair reach the *build* — `promote`
    /// itself refuses a branch that conflicts with a sibling, so a hold inside a
    /// nested rebuild can only be staged this way.
    fn inject_branches_into_metadata(
        env: &TestEnvironment,
        env_name: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "hitch-metadata"])?;
        let config_str = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
        config["environments"][env_name]["branches"] = serde_json::Value::Array(
            branches
                .iter()
                .map(|b| serde_json::Value::String(b.to_string()))
                .collect(),
        );
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: inject branches into metadata"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// Three branches editing the same file incompatibly, so any build that
    /// composes more than one of them must hold the rest. Declaration order
    /// decides the merge order, so `branch-b` and `branch-c` each conflict with
    /// `branch-a` and are held in turn.
    fn three_conflicting_branches(env: &TestEnvironment) -> anyhow::Result<()> {
        for name in ["branch-a", "branch-b", "branch-c"] {
            env.git.run(&["checkout", "-b", name])?;
            env.fs.write_file("shared.txt", &format!("from {name}\n"))?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", &format!("{name} changes shared.txt")])?;
            env.git.run(&["checkout", "main"])?;
        }
        Ok(())
    }

    /// The nested build used to print a `StepLogger` transcript —
    /// `[1/3] Rebuilding environment 'dev' - Synchronizing branches`, `Merging
    /// 'branch-a'`, `Publishing 'dev'` — between the plan and the receipt, in a
    /// vocabulary the plan had already superseded. Three renderings of one
    /// operation, and the reader had to work out which was the plan.
    ///
    /// `demote` rather than `promote`, because `promote` refuses a branch whose
    /// environment already holds an incompatible pair — the hold has to come
    /// from the *nested* build, and a promote cannot reach one.
    #[test]
    fn test_demote_narrates_the_nested_rebuild_only_once() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            three_conflicting_branches(env)?;
            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev"])
                .execute()?
                .assert_success();
            inject_branches_into_metadata(env, "dev", &["branch-a", "branch-b", "branch-c"])?;

            let result = env
                .hitch
                .run()
                .args(&["demote", "branch-c", "dev"])
                .execute()?;
            let stdout = result.stdout();

            // The plan's own line for the nested rebuild is still there…
            assert!(
                stdout.contains("rebuild dev"),
                "the plan must still say it will rebuild dev:\n{stdout}"
            );
            // …and the transcript that repeated it in another vocabulary is not.
            for transcript in [
                "Synchronizing branches",
                "Publishing 'dev'",
                "Rebuilding environment",
                "Triggering rebuild",
            ] {
                assert!(
                    !stdout.contains(transcript),
                    "the nested build narrated {transcript:?} between the plan and the \
                     receipt:\n{stdout}"
                );
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A hold inside a nested rebuild used to be reported *nowhere* once the
    /// transcript was gone: the nested build's own receipt — which is where
    /// `DependentRebuildOutcome::Rebuilt`'s doc said the holds were recorded —
    /// is thrown away by `rebuild_environment_opts`, and the caller bound
    /// `Ok(_)`. So a demote whose build silently held a branch reported a clean
    /// `✓ rebuild dev`, and only the `Result` block hinted at it.
    ///
    /// The receipt must now name the hold, *and* name the partner: a hold with
    /// no partner is indistinguishable from a base that moved underneath the
    /// branch, and those have different remedies.
    #[test]
    fn test_a_hold_inside_a_nested_rebuild_reaches_the_receipt() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            three_conflicting_branches(env)?;
            env.hitch
                .run()
                .args(&["promote", "branch-a", "dev"])
                .execute()?
                .assert_success();
            inject_branches_into_metadata(env, "dev", &["branch-a", "branch-b", "branch-c"])?;

            let result = env
                .hitch
                .run()
                .args(&["demote", "branch-c", "dev"])
                .execute()?;
            let stdout = result.stdout();

            assert!(
                stdout.contains("branch held: branch-b (conflicts with branch-a)"),
                "the receipt must name the held branch and its partner:\n{stdout}"
            );
            // Not owed, and not clean: both of those would be lies about a build
            // that left a declared branch out.
            assert!(
                !stdout.contains("✓ rebuild dev"),
                "a build that held a branch must not render as a clean rebuild:\n{stdout}"
            );
            assert!(
                !stdout.contains("⧗ rebuild dev"),
                "a hold is not owed work:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A demote's `Result` block names the removed branch once, and every detail
    /// line says what happened. `removed ⊆ changed_inputs` holds by
    /// construction — `health_from_record` walks the *recorded* pins, and a
    /// branch that has left the declaration resolves to no current SHA — so
    /// rendering both lists said the same branch twice, and the SHA line is the
    /// one carrying no action.
    ///
    /// `--no-rebuild`, because that is the only way a removal reaches
    /// `NeedsRebuild` at all: a demote that rebuilds leaves the environment
    /// `realised`, and there is nothing to report. `main` is moved so the block
    /// has a genuine `moved` line too, and the two must not read alike.
    #[test]
    fn test_a_demoted_branch_is_named_once_in_the_result() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            for name in ["feature-1", "feature-2"] {
                env.git.run(&["checkout", "-b", name])?;
                env.fs.write_file(&format!("{name}.txt"), name)?;
                env.git.run(&["add", "."])?;
                env.git.run(&["commit", "-m", &format!("add {name}")])?;
                env.git.run(&["checkout", "main"])?;
                env.hitch
                    .run()
                    .args(&["promote", name, "dev"])
                    .execute()?
                    .assert_success();
            }

            // Revise the branch about to be demoted, so its demotion is a
            // *changed* input as well as a removal — the overlap being asserted.
            env.git.run(&["checkout", "feature-2"])?;
            env.fs.write_file("feature-2.txt", "revised\n")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "revise feature-2"])?;
            env.git.run(&["checkout", "main"])?;
            // And move the base, so there is a `moved` line to render beside it.
            env.git
                .run(&["commit", "--allow-empty", "-m", "main moves"])?;

            let result = env
                .hitch
                .run()
                .args(&["demote", "feature-2", "dev", "--no-rebuild"])
                .execute()?;
            let stdout = result.stdout();

            assert!(
                stdout.contains("feature-2 removed from the declaration"),
                "the removal must be named:\n{stdout}"
            );
            assert!(
                stdout.contains("main moved"),
                "a moved base must say so rather than render a bare arrow:\n{stdout}"
            );
            // Scoped to the `Result` block: the plan legitimately names the
            // branch three more times above (Current, Proposed, the effect), and
            // the assertion is about the one place that used to say it twice.
            let result_block = stdout
                .split_once("Result")
                .expect("the receipt must render a Result block")
                .1;
            assert_eq!(
                result_block.matches("feature-2").count(),
                1,
                "the demoted branch is reported more than once in Result:\n{result_block}"
            );
            assert!(
                !result_block.contains("feature-2 moved"),
                "a removal must not also render as a moved input:\n{result_block}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Item 5: --no-rebuild flag for batching promotes/demotes
    // -------------------------------------------------------------------------

    /// `hitch promote --no-rebuild` should add the branch to metadata but skip
    /// the rebuild step, printing the "Skipping rebuild" message.
    #[test]
    fn test_promote_no_rebuild_skips_rebuild() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feat-no-rebuild"])?;
            env.fs.write_file("feat.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feat-no-rebuild", "dev", "--no-rebuild"])
                .execute()?;

            result
                .assert_success()
                .assert_stdout_contains("promote feat-no-rebuild into 'dev'")
                .assert_stdout_contains("Skipping rebuild");

            // Branch must appear in metadata
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.branches.contains(&"feat-no-rebuild".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Batch workflow: promote several branches with `--no-rebuild`, then run
    /// `hitch rebuild` once. All branches should end up in the env after that
    /// single rebuild.
    #[test]
    fn test_batch_promote_then_rebuild() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create two independent branches
            for branch in &["batch-feat-1", "batch-feat-2"] {
                env.git.run(&["checkout", "-b", branch])?;
                env.fs.write_file(&format!("{}.txt", branch), "content")?;
                env.git.run(&["add", "."])?;
                env.git.run(&["commit", "-m", &format!("Add {}", branch)])?;
                env.git.run(&["checkout", "main"])?;
            }

            // Promote both with --no-rebuild
            for branch in &["batch-feat-1", "batch-feat-2"] {
                env.hitch
                    .run()
                    .args(&["promote", branch, "dev", "--no-rebuild"])
                    .execute()?
                    .assert_success();
            }

            // Both should be in metadata
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.branches.contains(&"batch-feat-1".to_string()));
            assert!(dev_env.branches.contains(&"batch-feat-2".to_string()));

            // Now run a single rebuild
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            // The dev branch should now include commits from both features
            let feat1_exists = env
                .git
                .run(&["log", "dev", "--oneline", "--grep=batch-feat-1"]);
            let feat2_exists = env
                .git
                .run(&["log", "dev", "--oneline", "--grep=batch-feat-2"]);
            assert!(
                feat1_exists.is_ok(),
                "batch-feat-1 commits should be in dev after rebuild"
            );
            assert!(
                feat2_exists.is_ok(),
                "batch-feat-2 commits should be in dev after rebuild"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch demote --no-rebuild` should remove the branch from metadata but
    /// skip the rebuild step.
    #[test]
    fn test_demote_no_rebuild_skips_rebuild() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.git.run(&["checkout", "-b", "feat-demote-no-rebuild"])?;
            env.fs.write_file("feat.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feat"])?;
            env.git.run(&["checkout", "main"])?;

            // Promote normally first
            env.hitch
                .run()
                .args(&["promote", "feat-demote-no-rebuild", "dev"])
                .execute()?
                .assert_success();

            // Demote with --no-rebuild
            let result = env
                .hitch
                .run()
                .args(&["demote", "feat-demote-no-rebuild", "dev", "--no-rebuild"])
                .execute()?;

            result
                .assert_success()
                .assert_stdout_contains("demote feat-demote-no-rebuild out of 'dev'")
                .assert_stdout_contains("Skipping rebuild");

            // Branch must be removed from metadata
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env
                .branches
                .contains(&"feat-demote-no-rebuild".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// When the working tree is dirty, `hitch promote` should auto-stash, run
    /// the rebuild, and restore the stashed changes on completion.
    #[test]
    fn test_promote_auto_stashes_dirty_working_tree() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a feature branch
            env.git.run(&["checkout", "-b", "feature-auto-stash"])?;
            env.fs.write_file("feature.txt", "feature content")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Leave uncommitted changes in the working tree
            env.fs.write_file("wip.txt", "work in progress")?;
            env.git.run(&["add", "."])?; // staged but not committed

            // Promote should succeed despite dirty working tree
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-auto-stash", "dev"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("promote feature-auto-stash into 'dev'");

            // After promotion, wip.txt should be restored
            let wip_content = env.fs.read_file("wip.txt")?;
            assert_eq!(
                wip_content.trim(),
                "work in progress",
                "Auto-stash should have restored wip.txt after promotion"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
