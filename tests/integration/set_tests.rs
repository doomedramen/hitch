//! Integration tests for hitch set command

#[cfg(test)]
mod tests {
    use crate::test_framework::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_set_base_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with main as base
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Change base branch to develop
            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "develop"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'dev'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "develop");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_requires_approval() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Enable approval requirement WITH an approver (required for valid config)
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert!(prod_env.requires_approval);
            assert_eq!(prod_env.approvers.len(), 1);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_min_approvals() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and two approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Set minimum approvals
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--min-approvals", "2"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.min_approvals, 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_add_approvers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and an initial approver
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "initial@example.com",
                ])
                .execute()?
                .assert_success();

            // Add more approvers
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 3);
            assert!(prod_env
                .approvers
                .contains(&"initial@example.com".to_string()));
            assert!(prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));
            assert!(prod_env.approvers.contains(&"bob@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_remove_approver() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Remove an approver (leave at least one to keep config valid)
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--remove-approver", "bob@example.com"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 1);
            assert!(prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));
            assert!(!prod_env.approvers.contains(&"bob@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_approvers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and initial approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "1",
                    "--set-approvers",
                    "alice@example.com",
                ])
                .execute()?
                .assert_success();

            // Set complete list of approvers (replaces existing)
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--set-approvers",
                    "charlie@example.com",
                    "--set-approvers",
                    "dave@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 2);
            assert!(prod_env
                .approvers
                .contains(&"charlie@example.com".to_string()));
            assert!(prod_env.approvers.contains(&"dave@example.com".to_string()));
            assert!(!prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_no_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to set without any changes
            let result = env.hitch.run().args(&["set", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No changes specified");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Try to update nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["set", "nonexistent", "--base", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_invalid_base_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to set nonexistent base branch
            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_invalid_email_format() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Try to add approver with invalid email
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--add-approver", "invalid-email"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Invalid email format");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_min_approvals_zero() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Try to set min_approvals to 0
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--min-approvals", "0"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Minimum approvals must be at least 1");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_multiple_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with main as base
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Apply multiple changes at once
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--base",
                    "develop",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "1",
                    "--add-approver",
                    "alice@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Successfully updated environment 'dev'");

            // Verify all changes were applied
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "develop");
            assert!(dev_env.requires_approval);
            assert_eq!(dev_env.min_approvals, 1);
            assert_eq!(dev_env.approvers.len(), 1);
            assert!(dev_env.approvers.contains(&"alice@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add production environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Configure approval workflow (all in one command)
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "2",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Verify configuration
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert!(prod_env.requires_approval);
            assert_eq!(prod_env.min_approvals, 2);
            assert_eq!(prod_env.approvers.len(), 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
