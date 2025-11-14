use anyhow::Result;
use std::process::Command;

#[test]
fn test_init_with_environments() -> Result<()> {
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

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init with environments
    let output = Command::new(&hitch_path)
        .args(&["init", "--environments", "dev,qa,staging"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
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
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_json_content = std::fs::read_to_string(temp_dir.path().join("hitch.json"))?;

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
}

#[test]
fn test_init_with_verbose_flag() -> Result<()> {
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

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init with verbose flag
    let output = Command::new(&hitch_path)
        .args(&["init", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
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
}

#[test]
fn test_init_with_no_push_flag() -> Result<()> {
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

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init with no-push flag
    let output = Command::new(&hitch_path)
        .args(&["init", "--no-push", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
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
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    // Check that files exist
    assert!(
        temp_dir.path().join("hitch.json").exists(),
        "hitch.json should exist locally"
    );

    assert!(
        temp_dir.path().join(".gitignore").exists(),
        ".gitignore should exist locally"
    );

    Ok(())
}

#[test]
fn test_init_error_non_git_repo() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init in non-git directory
    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
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
}

#[test]
fn test_init_error_dirty_working_directory() -> Result<()> {
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

    // Create dirty file (uncommitted changes)
    std::fs::write(temp_dir.path().join("dirty.txt"), "This is uncommitted")?;

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init in dirty directory
    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
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
}
