//! Integration tests for hitch promote/demote commands

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    
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
                .assert_stdout_contains("Successfully promoted 'feature-1' to environment 'dev'");

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
                .assert_stdout_contains("Successfully demoted 'feature-1' from environment 'dev'");

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
                .assert_stderr_contains("Failed to read hitch.json");

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
                .assert_stderr_contains("Failed to read hitch.json");

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
}
