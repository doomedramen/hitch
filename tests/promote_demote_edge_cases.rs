use anyhow::Result;
use std::fs;
use std::process::Command;
use tempfile::tempdir;
use std::path::Path;

/// Simple test environment setup helper
struct TestEnv {
    _dir: tempfile::TempDir, // Use underscore to indicate intentionally unused
    original_dir: std::path::PathBuf,
}

impl TestEnv {
    fn new() -> Result<Self> {
        // Create a unique temp directory for each test
        let temp_dir = tempdir()?;
        let original_dir = std::env::current_dir()?;

        // Ensure we're starting from a clean state
        std::env::set_current_dir(temp_dir.path())?;

        // Double-check that no git lock files exist
        let git_dir = temp_dir.path().join(".git");
        if git_dir.exists() {
            // Clean up any potential lock files from previous runs
            let lock_file = git_dir.join("index.lock");
            if lock_file.exists() {
                fs::remove_file(&lock_file)?;
            }
        }

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

        // Create initial commit
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

    fn create_environment_config(&self, env_name: &str, base_branch: &str, branches: &[&str]) -> Result<()> {
        use std::collections::HashMap;

        let mut environments = HashMap::new();
        let mut branches_vec = Vec::new();
        for branch in branches {
            branches_vec.push(branch.to_string());
        }

        environments.insert(env_name.to_string(), serde_json::json!({
            "base": base_branch,
            "branches": branches_vec,
            "locked": false,
            "lockedBy": null,
            "lockedAt": null,
            "rebuiltAt": null
        }));

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        // Write to hitch-metadata branch (not orphan - it should already exist from hitch init)
        Command::new("git").args(&["checkout", "hitch-metadata"]).output()?;

        // Update hitch.json with the new environment configuration
        fs::write("hitch.json", serde_json::to_string_pretty(&config)?)?;
        Command::new("git").args(&["add", "hitch.json"]).output()?;
        Command::new("git").args(&["commit", "-m", &format!("Add environment '{}'", env_name)]).output()?;
        Command::new("git").args(&["checkout", "main"]).output()?;

        Ok(())
    }

    fn create_branch_and_commit(&self, branch_name: &str, message: &str) -> Result<()> {
        // Ensure we're on main branch first to avoid hitch-metadata .gitignore issues
        Command::new("git").args(&["checkout", "main"]).output()?;

        Command::new("git").args(&["checkout", "-b", branch_name]).output()?;

        // Clean any ignored files from previous operations
        Command::new("git").args(&["clean", "-fd"]).output()?;

        // Use unique filename that won't be ignored by hitch-metadata .gitignore
        // Since hitch-metadata .gitignore has "*", our files will be ignored
        // So we force add them
        let filename = format!("{}.txt", branch_name.replace("/", "_"));
        fs::write(&filename, message)?;
        Command::new("git").args(&["add", "-f", &filename]).output()?;
        Command::new("git").args(&["commit", "-m", message]).output()?;
        Command::new("git").args(&["checkout", "main"]).output()?;
        Ok(())
    }

    fn get_current_branch(&self) -> Result<String> {
        let output = Command::new("git")
            .args(&["branch", "--show-current"])
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(&["branch", "--list", branch])
            .output()?;

        Ok(output.status.success() && !output.stdout.is_empty())
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

    fn create_file(&self, path: &str, content: &str) -> Result<()> {
        fs::write(path, content)?;
        Ok(())
    }

    fn run_git_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new("git")
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
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

/// Test promote command with invalid arguments
#[test]
fn test_promote_invalid_arguments() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Test missing arguments
    let output = test_env.run_hitch_command(&["promote"])?;
    assert!(!output.status.success(), "Promote should fail with missing arguments");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the following required arguments were not provided") || stderr.contains("missing required arguments") || stderr.contains("expected 2 arguments"));

    Ok(())
}

/// Test demote command with invalid arguments
#[test]
fn test_demote_invalid_arguments() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Test missing arguments
    let output = test_env.run_hitch_command(&["demote"])?;
    assert!(!output.status.success(), "Demote should fail with missing arguments");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the following required arguments were not provided") || stderr.contains("missing required arguments") || stderr.contains("expected 2 arguments"));

    Ok(())
}

/// Test promote when not initialized
#[test]
fn test_promote_not_initialized() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Initialize git but not hitch
    test_env.init_git_repo()?;

    // Try to promote
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail when hitch not initialized");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("Failed to read hitch.json"));

    Ok(())
}

/// Test demote when not initialized
#[test]
fn test_demote_not_initialized() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Initialize git but not hitch
    test_env.init_git_repo()?;

    // Try to demote
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Demote should fail when hitch not initialized");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("Failed to read hitch.json"));

    Ok(())
}

/// Test promote with non-existent environment
#[test]
fn test_promote_nonexistent_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;

    // Try to promote to non-existent environment
    let output = test_env.run_hitch_command(&["promote", "feature/test", "nonexistent"])?;
    assert!(!output.status.success(), "Promote should fail with non-existent environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"));

    Ok(())
}

/// Test demote with non-existent environment
#[test]
fn test_demote_nonexistent_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;

    // Try to demote from non-existent environment
    let output = test_env.run_hitch_command(&["demote", "feature/test", "nonexistent"])?;
    assert!(!output.status.success(), "Demote should fail with non-existent environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"));

    Ok(())
}

/// Test promote with non-existent branch
#[test]
fn test_promote_nonexistent_branch() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_environment_config("dev", "main", &[])?;

    // Ensure we're on main branch with clean working directory
    Command::new("git").args(&["checkout", "main"]).output()?;
    Command::new("git").args(&["clean", "-fd"]).output()?;

    // Try to promote non-existent branch
    let output = test_env.run_hitch_command(&["promote", "nonexistent", "dev"])?;
    assert!(!output.status.success(), "Promote should fail with non-existent branch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"), "Expected error about non-existent branch, got: {}", stderr);

    Ok(())
}

/// Test promote branch that's already promoted
#[test]
fn test_promote_already_promoted_branch() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &["feature/test"])?;

    // Try to promote already promoted branch
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail with already promoted branch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already promoted"), "Expected error message about already promoted branch, got: {}", stderr);

    Ok(())
}

/// Test demote branch that's not promoted
#[test]
fn test_demote_not_promoted_branch() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &[])?;

    // Try to demote non-promoted branch
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Demote should fail with non-promoted branch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not promoted"));

    Ok(())
}

/// Test promote with dirty working directory
#[test]
fn test_promote_dirty_working_directory() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &[])?;

    // Ensure we're on main branch and clean any gitignore effects
    Command::new("git").args(&["checkout", "main"]).output()?;
    Command::new("git").args(&["clean", "-fd"]).output()?;

    // Create untracked file that won't be ignored
    test_env.create_file("untracked.txt", "This should cause pre-check to fail")?;

    // Try to promote
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail with dirty working directory");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Working tree is not clean") || stderr.contains("not clean") || stderr.contains("unclean") || stderr.contains("clean"), "Expected error about working directory not being clean, got: {}", stderr);

    Ok(())
}

/// Test demote with dirty working directory
#[test]
fn test_demote_dirty_working_directory() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &["feature/test"])?;

    // Ensure we're on main branch and clean any gitignore effects
    Command::new("git").args(&["checkout", "main"]).output()?;
    Command::new("git").args(&["clean", "-fd"]).output()?;

    // Create untracked file that won't be ignored
    test_env.create_file("untracked.txt", "This should cause pre-check to fail")?;

    // Try to demote
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Demote should fail with dirty working directory");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Working tree is not clean") || stderr.contains("not clean") || stderr.contains("unclean") || stderr.contains("clean"), "Expected error about working directory not being clean, got: {}", stderr);

    Ok(())
}

/// Test promote to locked environment without force
#[test]
fn test_promote_locked_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &[])?;

    // Lock the environment manually
    let mut config_content = String::new();
    {
        use std::collections::HashMap;
        let mut environments = HashMap::new();
        environments.insert("dev".to_string(), serde_json::json!({
            "base": "main",
            "branches": [],
            "locked": true,
            "lockedBy": "test@example.com",
            "lockedAt": "2024-01-01T00:00:00Z",
            "rebuiltAt": null
        }));

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        config_content = serde_json::to_string_pretty(&config)?;
    }

    // Update hitch.json with locked environment
    Command::new("git").args(&["checkout", "hitch-metadata"]).output()?;
    fs::write("hitch.json", config_content)?;
    Command::new("git").args(&["add", "hitch.json"]).output()?;
    Command::new("git").args(&["commit", "-m", "Lock environment"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    // Try to promote to locked environment
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail to locked environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("locked") || stderr.contains("currently locked"));

    Ok(())
}

/// Test demote from locked environment without force
#[test]
fn test_demote_locked_environment() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &["feature/test"])?;

    // Lock the environment manually
    let mut config_content = String::new();
    {
        use std::collections::HashMap;
        let mut environments = HashMap::new();
        environments.insert("dev".to_string(), serde_json::json!({
            "base": "main",
            "branches": ["feature/test"],
            "locked": true,
            "lockedBy": "test@example.com",
            "lockedAt": "2024-01-01T00:00:00Z",
            "rebuiltAt": null
        }));

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        config_content = serde_json::to_string_pretty(&config)?;
    }

    // Update hitch.json with locked environment
    Command::new("git").args(&["checkout", "hitch-metadata"]).output()?;
    fs::write("hitch.json", config_content)?;
    Command::new("git").args(&["add", "hitch.json"]).output()?;
    Command::new("git").args(&["commit", "-m", "Lock environment"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    // Try to demote from locked environment
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Demote should fail to locked environment");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("locked") || stderr.contains("currently locked"));

    Ok(())
}

/// Test promote command in non-git repository
#[test]
fn test_promote_non_git_repository() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Don't initialize git, just try to promote
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail in non-git repository");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Not in a Git repository") || stderr.contains("git repository"));

    Ok(())
}

/// Test demote command in non-git repository
#[test]
fn test_demote_non_git_repository() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Don't initialize git, just try to demote
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Demote should fail in non-git repository");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Not in a Git repository") || stderr.contains("git repository"));

    Ok(())
}

/// Test promote and demote workflow integration
#[test]
fn test_promote_demote_integration_workflow() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &[])?;

    // 1. Promote the branch
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    if !output.status.success() {
        println!("Promote failed in integration workflow:");
        println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success(), "Promote should succeed");

    // Verify we're back on main branch
    let current_branch = test_env.get_current_branch()?;
    assert_eq!(current_branch, "main", "Should be back on main branch after promote");

    // Verify dev branch was rebuilt
    assert!(test_env.branch_exists("dev")?, "Dev branch should exist after promote");

    // 2. Demote the branch
    let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
    assert!(output.status.success(), "Demote should succeed");

    // Verify we're back on main branch
    let current_branch = test_env.get_current_branch()?;
    assert_eq!(current_branch, "main", "Should be back on main branch after demote");

    // Verify dev branch was rebuilt again
    assert!(test_env.branch_exists("dev")?, "Dev branch should still exist after demote");

    Ok(())
}

/// Test promote with environment that has missing base branch
#[test]
fn test_promote_environment_missing_base_branch() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;

    // Create environment with non-existent base branch
    test_env.create_environment_config("dev", "nonexistent-base", &[])?;

    // Try to promote
    let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output.status.success(), "Promote should fail when environment has missing base branch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist") || stderr.contains("Base branch"));

    Ok(())
}

/// Test concurrent operations simulation
#[test]
fn test_concurrent_operations_simulation() -> Result<()> {
    let test_env = TestEnv::new()?;

    test_env.init_git_repo()?;
    test_env.run_hitch_init()?;
    test_env.create_branch_and_commit("feature/test", "Test feature")?;
    test_env.create_environment_config("dev", "main", &[])?;

    // Simulate concurrent access by running promote twice in sequence quickly
    let output1 = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(output1.status.success(), "First promote should succeed");

    let current_branch = test_env.get_current_branch()?;
    assert_eq!(current_branch, "main", "Should be back on main branch");

    // Second promote should fail because branch is already promoted
    let output2 = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
    assert!(!output2.status.success(), "Second promote should fail because branch is already promoted");

    Ok(())
}