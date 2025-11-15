use anyhow::Result;
use std::fs;
use std::process::Command;
use tempfile::tempdir;
use std::path::Path;

/// Simple test environment setup helper
struct TestEnv {
    _dir: tempfile::TempDir,
    original_dir: std::path::PathBuf,
}

impl TestEnv {
    fn new() -> Result<Self> {
        let temp_dir = tempdir()?;
        let original_dir = std::env::current_dir()?;

        std::env::set_current_dir(temp_dir.path())?;

        Ok(TestEnv {
            _dir: temp_dir,
            original_dir,
        })
    }

    fn path(&self) -> &Path {
        self._dir.path()
    }

    fn init_git_repo(&self) -> Result<()> {
        Command::new("git").args(&["init"]).output()?;
        Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
        Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

        fs::write("README.md", "# Test Repository")?;
        Command::new("git").args(&["add", "README.md"]).output()?;
        Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

        Ok(())
    }

    fn run_hitch_init(&self) -> Result<()> {
        let binary_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("hitch");

        let output = Command::new(&binary_path)
            .args(&["init"])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to run hitch init: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(())
    }

    fn create_environment_config(&self, env_name: &str, base_branch: &str, locked: bool) -> Result<()> {
        use std::collections::HashMap;

        let mut environments = HashMap::new();
        environments.insert(env_name.to_string(), serde_json::json!({
            "base": base_branch,
            "branches": [],
            "locked": locked,
            "lockedBy": if locked { Some("test@example.com".to_string()) } else { None },
            "lockedAt": if locked { Some("2024-01-01T00:00:00Z".to_string()) } else { None },
            "rebuiltAt": null
        }));

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        Command::new("git").args(&["checkout", "hitch-metadata"]).output()?;
        fs::write("hitch.json", serde_json::to_string_pretty(&config)?)?;
        Command::new("git").args(&["add", "hitch.json"]).output()?;
        Command::new("git").args(&["commit", "-m", "Update environment configuration"]).output()?;
        Command::new("git").args(&["checkout", "main"]).output()?;

        Ok(())
    }

    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let binary_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("hitch");

        let output = Command::new(&binary_path)
            .args(args)
            .output()?;

        Ok(output)
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Return to original directory
        let _ = std::env::set_current_dir(&self.original_dir);

        // Clean up any git lock files that might remain
        if let Ok(git_dir) = self._dir.path().join(".git").read_dir() {
            for entry in git_dir.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".lock") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

/// Test unlock command with valid locked environment
#[test]
fn test_unlock_valid_locked_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_environment_config("dev", "main", true)?;

    // Try to unlock
    let output = test_env.run_hitch_command(&["unlock", "dev"])?;
    assert!(output.status.success(), "Unlock should succeed for valid locked environment");

    Ok(())
}

/// Test unlock command with non-existent environment
#[test]
fn test_unlock_nonexistent_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;

    // Try to unlock non-existent environment
    let output = test_env.run_hitch_command(&["unlock", "nonexistent"])?;
    assert!(!output.status.success(), "Unlock should fail with non-existent environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist") || stderr.contains("not found"));

    Ok(())
}

/// Test unlock command with unlocked environment
#[test]
fn test_unlock_unlocked_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_environment_config("dev", "main", false)?;

    // Try to unlock already unlocked environment
    let output = test_env.run_hitch_command(&["unlock", "dev"])?;
    assert!(!output.status.success(), "Unlock should fail with unlocked environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not currently locked"));

    Ok(())
}

/// Test unlock command with missing arguments
#[test]
fn test_unlock_missing_arguments() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Test missing arguments
    let output = test_env.run_hitch_command(&["unlock"])?;
    assert!(!output.status.success(), "Unlock should fail with missing arguments");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the following required arguments were not provided") || stderr.contains("expected 1 argument"));

    Ok(())
}

/// Test unlock command without hitch initialization
#[test]
fn test_unlock_not_initialized() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;

    // Try to unlock without hitch init
    let output = test_env.run_hitch_command(&["unlock", "dev"])?;
    assert!(!output.status.success(), "Unlock should fail when hitch not initialized");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("Failed to read"));

    Ok(())
}

/// Test unlock command with verbose flag
#[test]
fn test_unlock_verbose_output() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_environment_config("dev", "main", true)?;

    // Try to unlock with verbose flag
    let output = test_env.run_hitch_command(&["unlock", "dev", "--verbose"])?;
    assert!(output.status.success(), "Unlock should succeed with verbose flag");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Validating unlock preconditions") || stdout.contains("✓ Unlock validation passed"));

    Ok(())
}

/// Test unlock command workflow integration
#[test]
fn test_unlock_workflow_integration() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_environment_config("dev", "main", true)?;

    // 1. Unlock the environment
    let output = test_env.run_hitch_command(&["unlock", "dev"])?;
    assert!(output.status.success(), "Unlock should succeed");

    // 2. Verify unlock message
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully unlocked"));

    Ok(())
}

/// Test unlock command basic functionality
#[test]
fn test_unlock_basic_functionality() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;

    // Create environment locked by different user
    let mut environments = std::collections::HashMap::new();
    environments.insert("dev".to_string(), serde_json::json!({
        "base": "main",
        "branches": [],
        "locked": true,
        "lockedBy": "other@example.com",
        "lockedAt": "2024-01-01T00:00:00Z",
        "rebuiltAt": null
    }));

    let config = serde_json::json!({
        "version": "1.0.0",
        "environments": environments
    });

    Command::new("git").args(&["checkout", "hitch-metadata"]).output()?;
    fs::write("hitch.json", serde_json::to_string_pretty(&config)?)?;
    Command::new("git").args(&["add", "hitch.json"]).output()?;
    Command::new("git").args(&["commit", "-m", "Create locked environment"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    // For now, just test that unlocking works
    // TODO: Add proper user validation testing with different git configs
    let output = test_env.run_hitch_command(&["unlock", "dev"])?;
    assert!(output.status.success(), "Unlock should succeed for locked environment");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully unlocked"));

    Ok(())
}