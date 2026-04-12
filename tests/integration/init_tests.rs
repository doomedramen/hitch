//! Integration tests for hitch init command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_init_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            // Test hitch init (git is already initialized by framework)
            let result = env.hitch.run().args(&["init"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch initialized successfully");

            // Hitch initialization verified by successful command above

            // Verify hitch.json contains expected structure
            let config = env.read_hitch_config()?;
            assert_eq!(config.version, "1.0");
            assert!(config.environments.is_empty());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_init_with_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            // Test hitch init with environments
            let result = env
                .hitch
                .run()
                .args(&["init", "--environments", "dev,qa,prod"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch initialized successfully")
                .assert_stdout_contains("Created environments: dev, qa, prod");

            // Verify environments were created
            let config = env.read_hitch_config()?;
            assert!(config.environments.contains_key("dev"));
            assert!(config.environments.contains_key("qa"));
            assert!(config.environments.contains_key("prod"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_init_already_initialized() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            // Initialize hitch first
            env.hitch.run().args(&["init"]).execute()?.assert_success();

            // Try to initialize again - should fail
            let result = env.hitch.run().args(&["init"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Hitch is already initialized");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
