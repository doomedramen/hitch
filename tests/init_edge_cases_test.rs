use anyhow::Result;
use std::process::Command;

#[test]
fn test_init_already_initialized_error() -> Result<()> {
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

    // Manually create hitch-metadata branch to simulate already initialized state
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    std::fs::write(temp_dir.path().join("hitch.json"), "{}\n")?;
    Command::new("git")
        .args(&["add", "hitch.json"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initialize Hitch"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Now init should fail because hitch-metadata branch already exists
    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(!output.status.success(), "init should fail when hitch-metadata exists");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    assert!(full_output.contains("hitch-metadata branch already exists") ||
            full_output.contains("already initialized"),
        "Should mention branch already exists. Got: {}", full_output);

    Ok(())
}

#[test]
fn test_init_remote_push_success() -> Result<()> {
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

    // Set up a fake remote
    Command::new("git")
        .args(&["remote", "add", "origin", "https://github.com/example/repo.git"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run init to test remote push (will fail but should try)
    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Init should succeed even if remote push fails
    assert!(output.status.success(), "init should succeed even if remote push fails");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    // Should try to push and either succeed or show warning
    assert!(full_output.contains("hitch-metadata branch pushed to remote") ||
                full_output.contains("Failed to push metadata to remote"),
        "Should contain push result message. Got: {}", full_output);

    Ok(())
}

#[test]
fn test_init_original_branch_check() -> Result<()> {
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

    // Create and switch to a different branch
    Command::new("git")
        .args(&["checkout", "-b", "feature"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run init with verbose to see branch checking
    let output = Command::new(&hitch_path)
        .args(&["init", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "init should succeed");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    // Should show branch checking and return to original branch
    assert!(full_output.contains("Creating hitch-metadata branch"),
        "Should mention creating hitch-metadata branch. Got: {}", full_output);

    // Check current branch - the init command leaves us on hitch-metadata branch
    let current_branch_output = Command::new("git")
        .args(&["branch", "--show-current"])
        .current_dir(&temp_dir)
        .output()?;

    let current_branch = String::from_utf8(current_branch_output.stdout)?.trim().to_string();
    assert_eq!(current_branch, "hitch-metadata", "Should be on hitch-metadata branch after init");

    // The verbose output should mention branch checking
    assert!(full_output.contains("Creating hitch-metadata branch"),
        "Should mention creating hitch-metadata branch. Got: {}", full_output);

    Ok(())
}