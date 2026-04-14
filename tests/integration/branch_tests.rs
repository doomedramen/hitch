//! Integration tests for hitch branch command

#[cfg(test)]
mod tests {
    use crate::test_framework::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_branch_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add environments to promote to
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

            // Create a develop branch to use as the base
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Run hitch branch to create feature/foo from develop and promote to dev and qa
            let result = env
                .hitch
                .run()
                .args(&[
                    "branch",
                    "feature/foo",
                    "develop",
                    "--to",
                    "dev",
                    "--to",
                    "qa",
                ])
                .execute()?;
            result.assert_success().assert_stdout_contains(
                "Branch 'feature/foo' created from 'develop' and checked out!",
            );

            // Verify the branch exists and is checked out
            let current_branch = env.git.get_current_branch()?;
            assert_eq!(current_branch, "feature/foo");

            // Verify the branch is promoted to dev and qa in metadata
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            let qa_env = config.environments.get("qa").unwrap();
            assert!(dev_env.branches.contains(&"feature/foo".to_string()));
            assert!(qa_env.branches.contains(&"feature/foo".to_string()));
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_branch_missing_env() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create a develop branch to use as the base
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Try to promote to a non-existent environment
            let result = env
                .hitch
                .run()
                .args(&["branch", "feature/bar", "develop", "--to", "doesnotexist"])
                .execute()?;
            result.assert_failure().assert_stderr_contains(
                "Promotion target environment 'doesnotexist' does not exist",
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
