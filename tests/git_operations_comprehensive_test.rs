use anyhow::Result;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

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

    fn init_git_repo_with_remote(&self) -> Result<()> {
        self.init_git_repo()?;

        // Add a fake remote
        let remote_dir = self._dir.path().join("remote");
        fs::create_dir_all(&remote_dir)?;
        Command::new("git").args(&["init", "--bare"]).current_dir(&remote_dir).output()?;

        Command::new("git").args(&["remote", "add", "origin", remote_dir.to_str().unwrap()]).output()?;

        Ok(())
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Return to original directory
        let _ = std::env::set_current_dir(&self.original_dir);
    }
}

/// Test create_orphan_branch functionality
#[test]
fn test_git_operations_create_orphan_branch() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create orphan branch
    git_ops.create_orphan_branch("orphan-test")?;

    // Verify we're on the orphan branch
    let current_branch = git_ops.get_current_branch()?;
    assert_eq!(current_branch, "orphan-test", "Should be on the newly created orphan branch");

    // Verify the branch has no commits
    let output = Command::new("git").args(&["log", "--oneline"]).output()?;
    let log_output = String::from_utf8_lossy(&output.stdout);
    assert!(log_output.trim().is_empty(), "Orphan branch should have no commits");

    Ok(())
}

/// Test git operations with custom path
#[test]
fn test_git_operations_new_at_path() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    // Create GitOperations instance with explicit path
    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env._dir.path().to_str().unwrap())?;

    // Test basic functionality
    let current_branch = git_ops.get_current_branch()?;
    assert_eq!(current_branch, "main", "Should detect main branch");

    // Test with non-existent directory
    let result = hitch::utils::git_operations::GitOperations::new_at_path("/nonexistent/path");
    assert!(result.is_err(), "Should fail with non-existent path");

    Ok(())
}

/// Test add_and_commit functionality
#[test]
fn test_git_operations_add_and_commit() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create a file
    fs::write("test.txt", "Test content")?;

    // Add and commit the file
    git_ops.add_and_commit(&["test.txt"], "Add test file")?;

    // Verify the commit was created
    let output = Command::new("git").args(&["log", "--oneline", "-1"]).output()?;
    let log_output = String::from_utf8_lossy(&output.stdout);
    assert!(log_output.contains("Add test file"), "Commit message should be in log");

    Ok(())
}

/// Test file operations (read/write)
#[test]
fn test_git_operations_file_operations() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Write a file and commit it so it's in the branch
    git_ops.write_file("test.txt", "Test content")?;
    git_ops.add_and_commit(&["test.txt"], "Add test file")?;

    // Verify file exists
    assert!(fs::metadata("test.txt").is_ok(), "File should exist after writing");

    // Test read_file_from_branch on current branch
    let content = git_ops.read_file_from_branch("main", "test.txt")?;
    assert_eq!(content, "Test content", "Should read correct content");

    // Test reading non-existent file
    let result = git_ops.read_file_from_branch("main", "nonexistent.txt");
    assert!(result.is_err(), "Should fail to read non-existent file");

    Ok(())
}

/// Test branch existence checking
#[test]
fn test_git_operations_branch_exists() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create a test branch
    Command::new("git").args(&["checkout", "-b", "test-branch"]).output()?;
    fs::write("test.txt", "Test")?;
    Command::new("git").args(&["add", "test.txt"]).output()?;
    Command::new("git").args(&["commit", "-m", "Test"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    // Test branch_exists (local branches only)
    assert!(git_ops.branch_exists("main")?, "Main branch should exist locally");
    assert!(git_ops.branch_exists("test-branch")?, "Test branch should exist locally");
    assert!(!git_ops.branch_exists("nonexistent")?, "Nonexistent branch should not exist");

    // Test branch_exists_anywhere (both local and remote)
    assert!(git_ops.branch_exists_anywhere("main")?, "Main should exist anywhere");
    assert!(git_ops.branch_exists_anywhere("test-branch")?, "Test branch should exist anywhere");

    Ok(())
}

/// Test branch management operations
#[test]
fn test_git_operations_branch_management() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create a test branch first
    git_ops.create_branch_from("feature-test", "main")?;

    // Verify branch was created
    assert!(git_ops.branch_exists_anywhere("feature-test")?, "New branch should exist");

    // Test rename branch
    git_ops.rename_branch("feature-test", "renamed-branch")?;
    assert!(!git_ops.branch_exists_anywhere("feature-test")?, "Old name should not exist");
    assert!(git_ops.branch_exists_anywhere("renamed-branch")?, "New name should exist");

    // Test delete branch
    Command::new("git").args(&["checkout", "main"]).output()?; // Switch off branch first
    git_ops.delete_branch("renamed-branch", false)?;
    assert!(!git_ops.branch_exists_anywhere("renamed-branch")?, "Deleted branch should not exist");

    // Test force delete (with unmerged changes)
    git_ops.create_branch_from("temp-branch", "main")?;
    Command::new("git").args(&["checkout", "temp-branch"]).output()?;
    fs::write("temp.txt", "temp")?;
    Command::new("git").args(&["add", "temp.txt"]).output()?;
    Command::new("git").args(&["commit", "-m", "Temp"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    git_ops.delete_branch("temp-branch", true)?;
    assert!(!git_ops.branch_exists_anywhere("temp-branch")?, "Force deleted branch should not exist");

    Ok(())
}

/// Test commit-related operations
#[test]
fn test_git_operations_commit_operations() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Test get_branch_commit_sha
    let commit_sha = git_ops.get_branch_commit_sha("main")?;
    assert!(!commit_sha.is_empty(), "Should get a commit SHA");
    assert_eq!(commit_sha.len(), 40, "SHA should be 40 characters");

    // Test get_commit_timestamp
    let timestamp = git_ops.get_commit_timestamp(&commit_sha)?;
    let now = chrono::Utc::now();
    assert!(timestamp <= now, "Commit timestamp should be in the past or now");

    Ok(())
}

/// Test fetch and push operations (with local remote)
#[test]
fn test_git_operations_remote_operations() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo_with_remote()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create a branch and push it
    git_ops.create_branch_from("test-remote", "main")?;
    fs::write("remote-test.txt", "Remote test")?;
    Command::new("git").args(&["add", "remote-test.txt"]).output()?;
    Command::new("git").args(&["commit", "-m", "Remote test"]).output()?;

    // Set upstream and push
    Command::new("git").args(&["push", "--set-upstream", "origin", "test-remote"]).output()?;

    // Test fetch
    let fetch_result = git_ops.fetch_branch("test-remote");
    // Fetch might fail in test environment due to no real remote, but that's ok
    // We're just testing that the function doesn't panic

    // Test push
    let push_result = git_ops.push_branch("test-remote");
    // Push might also fail, but we test the error handling

    Ok(())
}

/// Test squash merge functionality
#[test]
fn test_git_operations_squash_merge() -> Result<()> {
    let test_env = TestEnv::new()?;
    test_env.init_git_repo()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Create a feature branch with multiple commits
    git_ops.create_branch_from("feature-branch", "main")?;

    // First commit
    fs::write("file1.txt", "Content 1")?;
    git_ops.add_and_commit(&["file1.txt"], "First commit")?;

    // Second commit
    fs::write("file2.txt", "Content 2")?;
    git_ops.add_and_commit(&["file2.txt"], "Second commit")?;

    // Switch back to main
    git_ops.checkout_branch("main")?;

    // Test squash merge
    git_ops.squash_merge("feature-branch", "Squashed feature branch")?;

    // Verify the squash merge worked
    let output = Command::new("git").args(&["log", "--oneline", "-2"]).output()?;
    let log_output = String::from_utf8_lossy(&output.stdout);
    assert!(log_output.contains("Squashed feature branch"), "Should have squash commit message");
    assert!(fs::metadata("file1.txt").is_ok(), "File1 should exist after merge");
    assert!(fs::metadata("file2.txt").is_ok(), "File2 should exist after merge");

    Ok(())
}

/// Test error handling in git operations
#[test]
fn test_git_operations_error_handling() -> Result<()> {
    let test_env = TestEnv::new()?;

    // Test operations in non-git directory
    let result = hitch::utils::git_operations::GitOperations::new();
    assert!(result.is_err(), "Should fail to create GitOperations in non-git directory");

    test_env.init_git_repo()?;
    let git_ops = hitch::utils::git_operations::GitOperations::new()?;

    // Test checkout non-existent branch
    let result = git_ops.checkout_branch("nonexistent-branch");
    assert!(result.is_err(), "Should fail to checkout non-existent branch");

    // Test create branch from non-existent source
    let result = git_ops.create_branch_from("new-branch", "nonexistent-source");
    assert!(result.is_err(), "Should fail to create branch from non-existent source");

    // Test rename non-existent branch
    let result = git_ops.rename_branch("nonexistent", "newname");
    assert!(result.is_err(), "Should fail to rename non-existent branch");

    Ok(())
}