use anyhow::Result;
use chrono::Utc;
use std::process::Command;
use tempfile::tempdir;

/// Simple ANSI code stripper for test assertions
fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ANSI escape sequence
            if chars.next() == Some('[') {
                // Skip until we hit the end character (a-z)
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_alphabetic() {
                        chars.next(); // consume the end character
                        break;
                    }
                    chars.next(); // consume part of the sequence
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[test]
fn test_status_basic_display() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch with environments
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "main",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    },
    "staging": {
      "base": "main",
      "branches": ["feature/test"],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": "2025-01-14T10:00:00Z"
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command
    let output = Command::new(&hitch_path)
        .args(&["status"])
        .current_dir(&temp_dir)
        .output()?;

    // Strip ANSI color codes for easier assertions
    let stdout = String::from_utf8(output.stdout.clone())?;
    let stderr = String::from_utf8(output.stderr.clone())?;
    let clean_stdout = strip_ansi_codes(&stdout);
    let clean_output = format!("{}{}", clean_stdout, stderr);

    assert!(output.status.success(), "status command should succeed");

    // Verify basic status display elements (using cleaned text without ANSI codes)
    assert!(clean_output.contains("Environments (2 total):"));
    assert!(clean_output.contains("dev  (main)"));
    assert!(clean_output.contains("staging  (main)"));
    assert!(clean_output.contains("Branches: None"));
    assert!(clean_output.contains("feature/test"));
    assert!(clean_output.contains("Last rebuild: Never"));
    assert!(clean_output.contains("Last rebuild: 2025-01-14 10:00 UTC"));

    Ok(())
}

#[test]
fn test_status_locked_environment() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch with locked environment
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let locked_time = Utc::now();
    let hitch_config = format!(r#"{{
  "version": "1.0",
  "environments": {{
    "prod": {{
      "base": "main",
      "branches": ["feature/login", "feature/ui"],
      "locked": true,
      "locked_by": "admin@example.com",
      "locked_at": "{}",
      "rebuilt_at": null
    }}
  }}
}}"#, locked_time.format("%Y-%m-%dT%H:%M:%SZ"));

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command
    let output = Command::new(&hitch_path)
        .args(&["status"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "status command should succeed");

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let clean_stdout = strip_ansi_codes(&stdout);
    let clean_output = format!("{}{}", clean_stdout, stderr);

    // Verify locked environment display
    assert!(clean_output.contains("Environments (1 total):"));
    assert!(clean_output.contains("prod  (main)"));
    assert!(clean_output.contains("Locked by admin@example.com"));
    assert!(clean_output.contains("feature/login"));
    assert!(clean_output.contains("feature/ui"));
    assert!(clean_output.contains("Last rebuild: Never"));
    assert!(clean_output.contains("Never rebuilt"));

    Ok(())
}

#[test]
fn test_status_not_initialized() -> Result<()> {
    let temp_dir = tempdir()?;

    // Initialize git repo but don't initialize hitch
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

    // Run status command without hitch being initialized
    let output = Command::new(&hitch_path)
        .args(&["status"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(!output.status.success(), "status command should fail when hitch not initialized");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Failed to read hitch.json") ||
            stderr.contains("Failed to access hitch metadata") ||
            stderr.contains("Failed to checkout branch"));

    Ok(())
}

#[test]
fn test_status_verbose_output() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "main",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command with verbose flag
    let output = Command::new(&hitch_path)
        .args(&["status", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout.clone())?;
    let stderr = String::from_utf8(output.stderr.clone())?;

    assert!(output.status.success(), "status command should succeed");

    // Verify verbose output (verbose messages go to stdout)
    assert!(stdout.contains("Starting status command"));
    assert!(stdout.contains("Using git show approach for read-only status access"));
    assert!(stdout.contains("Successfully retrieved metadata using: git_show"));
    assert!(stdout.contains("Status command completed successfully"));

    Ok(())
}

#[test]
fn test_status_unclean_working_directory() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {}
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    // Create unclean working directory (uncommitted changes)
    std::fs::write(temp_dir.path().join("README.md"), "# Test Modified\n")?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command with verbose flag to see which method is used
    let output = Command::new(&hitch_path)
        .args(&["status", "--verbose"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "status command should succeed even with unclean working directory");

    let stdout = String::from_utf8(output.stdout)?;

    // Verify it uses git show approach (status command always uses this)
    assert!(stdout.contains("Using git show approach for read-only status access"));
    assert!(stdout.contains("Successfully retrieved metadata using: git_show"));

    Ok(())
}

#[test]
fn test_status_empty_environments() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch with empty environments
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {}
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command
    let output = Command::new(&hitch_path)
        .args(&["status"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "status command should succeed");

    let stdout = String::from_utf8(output.stdout)?;

    // Verify empty environments message
    assert!(stdout.contains("No environments configured."));

    Ok(())
}

#[test]
fn test_status_environmental_sorting() -> Result<()> {
    let temp_dir = tempdir()?;

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

    // Initialize hitch with environments out of alphabetical order
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "zebra": {
      "base": "main",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    },
    "alpha": {
      "base": "main",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    },
    "beta": {
      "base": "main",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(&temp_dir)
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run status command
    let output = Command::new(&hitch_path)
        .args(&["status"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "status command should succeed");

    let stdout = String::from_utf8(output.stdout)?;
    let clean_stdout = strip_ansi_codes(&stdout);

    // Verify environments are displayed in alphabetical order
    let lines: Vec<&str> = clean_stdout.lines().collect();
    let mut env_order = Vec::new();

    for line in lines {
        if line.contains("  (main)") {
            let env_name = line.split("  (main)").next().unwrap().trim();
            env_order.push(env_name);
        }
    }

    assert_eq!(env_order, vec!["alpha", "beta", "zebra"], "Environments should be displayed in alphabetical order");

    Ok(())
}