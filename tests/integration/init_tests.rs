//! Integration tests for hitch init command

#[cfg(test)]
mod tests {
    use crate::test_framework::*;

    #[test]
    fn test_hitch_init_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Test hitch init (git is already initialized by framework)
            let result = env.hitch.run().args(&["init"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Hitch initialized successfully");

            // Verify hitch was initialized
            env.assert.hitch_initialized(&env.fs).unwrap();

            // Verify hitch.json contains expected structure
            let config: serde_json::Value = env.fs.read_json("hitch.json")?;
            assert_eq!(config["version"], "1.0");
            assert!(config["environments"].as_object().unwrap().is_empty());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_init_with_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
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
            env.assert.hitch_environment_exists(&env.fs, "dev")?;
            env.assert.hitch_environment_exists(&env.fs, "qa")?;
            env.assert.hitch_environment_exists(&env.fs, "prod")?;

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_init_already_initialized() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
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
