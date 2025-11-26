use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{ensure_git_environment_ready, with_test_env, SetupLevel};

#[test]
fn test_init_with_environments() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Run hitch init with environments (this is the first init)
        let output = Command::new(&hitch_path)
            .args(["init", "--environments", "dev,qa,staging"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init succeeded
        assert!(output.status.success(), "hitch init should succeed");

        // Check for success message
        assert!(
            full_output.contains("Hitch initialized successfully"),
            "Output should contain success message. Got: {}",
            full_output
        );

        // Check that environments were created
        assert!(
            full_output.contains("Creating 3 environment(s): dev, qa, staging"),
            "Output should mention creating environments. Got: {}",
            full_output
        );

        // Switch to hitch-metadata branch and check hitch.json
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("hitch-metadata")?;

        let hitch_json_content = std::fs::read_to_string(test_env.path().join("hitch.json"))?;

        // Verify environments exist in the config
        assert!(
            hitch_json_content.contains("\"dev\""),
            "Config should contain dev environment"
        );
        assert!(
            hitch_json_content.contains("\"qa\""),
            "Config should contain qa environment"
        );
        assert!(
            hitch_json_content.contains("\"staging\""),
            "Config should contain staging environment"
        );

        // Verify environments have correct default base (main)
        assert!(
            hitch_json_content.contains("\"base\": \"main\""),
            "Environments should default to main base"
        );

        Ok(())
    })
}

#[test]
fn test_init_with_verbose_flag() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Run hitch init with verbose flag (this is the first init)
        let output = Command::new(&hitch_path)
            .args(["init", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init succeeded
        assert!(output.status.success(), "hitch init should succeed");

        // Check for verbose output elements
        assert!(
            full_output.contains("Running pre-check validation"),
            "Verbose output should show pre-check validation"
        );

        assert!(
            full_output.contains("Git repository validation passed"),
            "Verbose output should show git validation passed"
        );

        assert!(
            full_output.contains("Working tree is clean"),
            "Verbose output should show working tree validation"
        );

        Ok(())
    })
}

#[test]
fn test_init_with_no_push_flag() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Run hitch init with no-push flag (this is the first init)
        let output = Command::new(&hitch_path)
            .args(["init", "--no-push", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init succeeded
        assert!(output.status.success(), "hitch init should succeed");

        // Check for no-push message
        assert!(
            full_output.contains("Skipping push due to --no-push flag"),
            "Output should mention skipping push. Got: {}",
            full_output
        );

        // Verify the commit was still made locally
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("hitch-metadata")?;

        // Check that files exist
        assert!(
            test_env.path().join("hitch.json").exists(),
            "hitch.json should exist locally"
        );

        assert!(
            test_env.path().join(".gitignore").exists(),
            ".gitignore should exist locally"
        );

        Ok(())
    })
}

#[test]
fn test_init_error_non_git_repo() -> Result<()> {
    with_test_env(SetupLevel::Basic, |test_env| {
        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Run hitch init in non-git directory
        // Note: SetupLevel::Basic provides a temp directory without git initialization
        let output = Command::new(&hitch_path)
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init failed
        assert!(
            !output.status.success(),
            "hitch init should fail in non-git repo"
        );

        // Check for appropriate error message
        assert!(
            full_output.contains("Error: Not in a git repository"),
            "Output should mention not being in a git repository. Got: {}",
            full_output
        );

        Ok(())
    })
}

#[test]
fn test_init_error_dirty_working_directory() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Create dirty file (uncommitted changes) BEFORE init
        std::fs::write(test_env.path().join("dirty.txt"), "This is uncommitted")?;

        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Run hitch init in dirty directory (this should fail)
        let output = Command::new(&hitch_path)
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init failed
        assert!(
            !output.status.success(),
            "hitch init should fail in dirty directory"
        );

        // Check for appropriate error message
        assert!(
            full_output.contains("Working tree is not clean"),
            "Output should mention dirty working tree. Got: {}",
            full_output
        );

        Ok(())
    })
}
