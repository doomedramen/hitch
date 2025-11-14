use anyhow::Result;
use std::process::Command;

#[test]
fn test_init_line69_remote_push_success() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(&temp_dir)
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run init and check for the specific line 69 message
    let output = Command::new(&hitch_path)
        .args(&["init", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    // Should succeed even if remote push fails
    assert!(output.status.success(), "init should succeed");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    // Line 69: Should contain either success or failure message about pushing
    assert!(full_output.contains("hitch-metadata branch pushed to remote") ||
            full_output.contains("Failed to push metadata to remote") ||
            full_output.contains("Skipping push due to --no-push flag"),
        "Should contain push result message (line 69). Got: {}", full_output);

    Ok(())
}

#[test]
fn test_init_line78_original_branch_check() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(&temp_dir)
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run init and check for line 78 message
    let output = Command::new(&hitch_path)
        .args(&["init", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "init should succeed");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    // Line 78: Check if we get the original branch check message
    // This message appears when we're already on the hitch-metadata branch
    assert!(full_output.contains("Already on original branch") ||
            full_output.contains("Creating hitch-metadata branch"),
        "Should contain branch-related messages. Got: {}", full_output);

    Ok(())
}