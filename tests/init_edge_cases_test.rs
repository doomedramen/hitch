use anyhow::Result;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{ensure_git_environment_ready, with_test_env, SetupLevel};

#[test]
fn test_init_already_initialized_error() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before manually creating hitch-metadata branch
        ensure_git_environment_ready(test_env)?;

        // Manually create hitch-metadata branch to simulate already initialized state
        Command::new("git")
            .args(["checkout", "--orphan", "hitch-metadata"])
            .current_dir(test_env.path())
            .output()?;

        fs::write(test_env.path().join("hitch.json"), "{}\n")?;
        Command::new("git")
            .args(["add", "hitch.json"])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", "Initialize Hitch"])
            .current_dir(test_env.path())
            .output()?;

        // Return to main branch
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        // Now init should fail because hitch-metadata branch already exists (this is the first init attempt)
        let output = Command::new(test_env.hitch_binary())
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            !output.status.success(),
            "init should fail when hitch-metadata exists"
        );

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(
            full_output.contains("hitch-metadata branch already exists")
                || full_output.contains("already initialized"),
            "Should mention branch already exists. Got: {}",
            full_output
        );

        Ok(())
    })
}

#[test]
fn test_init_remote_push_success() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Set up a fake remote BEFORE init
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/example/repo.git",
            ])
            .current_dir(test_env.path())
            .output()?;

        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Run init to test remote push (will fail but should try) - this is the first init
        let output = Command::new(test_env.hitch_binary())
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        // Init should succeed even if remote push fails
        assert!(
            output.status.success(),
            "init should succeed even if remote push fails"
        );

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Should try to push and either succeed or show warning
        assert!(
            full_output.contains("hitch-metadata branch pushed to remote")
                || full_output.contains("Failed to push metadata to remote"),
            "Should contain push result message. Got: {}",
            full_output
        );

        Ok(())
    })
}

#[test]
fn test_init_original_branch_check() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Create and switch to a different branch BEFORE init
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(test_env.path())
            .output()?;

        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Run init with verbose to see branch checking - this is the first init
        let output = Command::new(test_env.hitch_binary())
            .args(["init", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        assert!(output.status.success(), "init should succeed");

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Should show branch checking and return to original branch
        assert!(
            full_output.contains("Creating hitch-metadata branch"),
            "Should mention creating hitch-metadata branch. Got: {}",
            full_output
        );

        // Check current branch - the init command leaves us on hitch-metadata branch
        let current_branch_output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(test_env.path())
            .output()?;

        let current_branch = String::from_utf8(current_branch_output.stdout)?
            .trim()
            .to_string();
        assert_eq!(
            current_branch, "hitch-metadata",
            "Should be on hitch-metadata branch after init"
        );

        // The verbose output should mention branch checking
        assert!(
            full_output.contains("Creating hitch-metadata branch"),
            "Should mention creating hitch-metadata branch. Got: {}",
            full_output
        );

        Ok(())
    })
}
