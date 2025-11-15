use anyhow::Result;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

/// Helper extension trait for TestEnv to provide custom methods needed by these tests
trait TestEnvExt {
    fn create_environment_config(
        &self,
        env_name: &str,
        base_branch: &str,
        locked: bool,
    ) -> Result<()>;
    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output>;
}

impl TestEnvExt for TestEnv {
    fn create_environment_config(
        &self,
        env_name: &str,
        base_branch: &str,
        locked: bool,
    ) -> Result<()> {
        use std::collections::HashMap;

        let mut environments = HashMap::new();
        environments.insert(
            env_name.to_string(),
            serde_json::json!({
                "base": base_branch,
                "branches": [],
                "locked": locked,
                "lockedBy": if locked { Some("test@example.com".to_string()) } else { None },
                "lockedAt": if locked { Some("2024-01-01T00:00:00Z".to_string()) } else { None },
                "rebuiltAt": null
            }),
        );

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        Command::new("git")
            .args(&["checkout", "hitch-metadata"])
            .current_dir(self.path())
            .output()?;
        fs::write(
            self.path().join("hitch.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        Command::new("git")
            .args(&["add", "hitch.json"])
            .current_dir(self.path())
            .output()?;
        Command::new("git")
            .args(&["commit", "-m", "Update environment configuration"])
            .current_dir(self.path())
            .output()?;
        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(self.path())
            .output()?;

        Ok(())
    }

    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new(&self.hitch_binary())
            .args(args)
            .current_dir(self.path())
            .output()?;

        Ok(output)
    }
}

/// Test unlock command with valid locked environment
#[test]
fn test_unlock_valid_locked_environment() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        test_env.create_environment_config("dev", "main", true)?;

        // Try to unlock
        let output = test_env.run_hitch_command(&["unlock", "dev"])?;
        assert!(
            output.status.success(),
            "Unlock should succeed for valid locked environment"
        );

        Ok(())
    })
}

/// Test unlock command with non-existent environment
#[test]
fn test_unlock_nonexistent_environment() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Try to unlock non-existent environment
        let output = test_env.run_hitch_command(&["unlock", "nonexistent"])?;
        assert!(
            !output.status.success(),
            "Unlock should fail with non-existent environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist") || stderr.contains("not found"));

        Ok(())
    })
}

/// Test unlock command with unlocked environment
#[test]
fn test_unlock_unlocked_environment() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        test_env.create_environment_config("dev", "main", false)?;

        // Try to unlock already unlocked environment
        let output = test_env.run_hitch_command(&["unlock", "dev"])?;
        assert!(
            !output.status.success(),
            "Unlock should fail with unlocked environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("not currently locked"));

        Ok(())
    })
}

/// Test unlock command with missing arguments
#[test]
fn test_unlock_missing_arguments() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Test missing arguments
        let output = test_env.run_hitch_command(&["unlock"])?;
        assert!(
            !output.status.success(),
            "Unlock should fail with missing arguments"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("the following required arguments were not provided")
                || stderr.contains("expected 1 argument")
        );

        Ok(())
    })
}

/// Test unlock command without hitch initialization
#[test]
fn test_unlock_not_initialized() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Try to unlock without hitch init
        let output = test_env.run_hitch_command(&["unlock", "dev"])?;
        assert!(
            !output.status.success(),
            "Unlock should fail when hitch not initialized"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("not found") || stderr.contains("Failed to read"));

        Ok(())
    })
}

/// Test unlock command with verbose flag
#[test]
fn test_unlock_verbose_output() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        test_env.create_environment_config("dev", "main", true)?;

        // Try to unlock with verbose flag
        let output = test_env.run_hitch_command(&["unlock", "dev", "--verbose"])?;
        assert!(
            output.status.success(),
            "Unlock should succeed with verbose flag"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Validating unlock preconditions")
                || stdout.contains("✓ Unlock validation passed")
        );

        Ok(())
    })
}

/// Test unlock command workflow integration
#[test]
fn test_unlock_workflow_integration() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        test_env.create_environment_config("dev", "main", true)?;

        // 1. Unlock the environment
        let output = test_env.run_hitch_command(&["unlock", "dev"])?;
        assert!(output.status.success(), "Unlock should succeed");

        // 2. Verify unlock message
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Successfully unlocked"));

        Ok(())
    })
}

/// Test unlock command basic functionality
#[test]
fn test_unlock_basic_functionality() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Create environment locked by different user
        let mut environments = std::collections::HashMap::new();
        environments.insert(
            "dev".to_string(),
            serde_json::json!({
                "base": "main",
                "branches": [],
                "locked": true,
                "lockedBy": "other@example.com",
                "lockedAt": "2024-01-01T00:00:00Z",
                "rebuiltAt": null
            }),
        );

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        Command::new("git")
            .args(&["checkout", "hitch-metadata"])
            .current_dir(test_env.path())
            .output()?;
        fs::write(
            test_env.path().join("hitch.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        Command::new("git")
            .args(&["add", "hitch.json"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(&["commit", "-m", "Create locked environment"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        // For now, just test that unlocking works
        // TODO: Add proper user validation testing with different git configs
        let output = test_env.run_hitch_command(&["unlock", "dev"])?;
        assert!(
            output.status.success(),
            "Unlock should succeed for locked environment"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Successfully unlocked"));

        Ok(())
    })
}
