use anyhow::Result;
use std::fs;
use std::process::Command;

mod common;
use common::TestEnv;

/// Test git operations functionality
#[test]
fn test_git_operations_basic_functionality() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    // Test git operations
    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Test get_current_branch
    let current_branch = git_ops.get_current_branch()?;
    assert_eq!(current_branch, "main");

    // Test get_user_email
    let email = git_ops.get_user_email()?;
    // Don't assert specific email as it may vary by system, just verify it's a valid email format
    assert!(email.contains('@'), "Email should contain @ symbol: {}", email);

    // Test is_working_directory_clean
    let is_clean = git_ops.is_working_directory_clean()?;
    assert!(is_clean);

    // Test branch_exists
    assert!(git_ops.branch_exists("main")?);
    assert!(!git_ops.branch_exists("nonexistent")?);

    // Test checkout_branch
    Command::new("git").args(&["checkout", "-b", "test-branch"]).current_dir(test_env.path()).output()?;
    assert_eq!(git_ops.get_current_branch()?, "test-branch");

    // Test fetch_branch (should fail gracefully with no remote)
    let result = git_ops.fetch_branch("nonexistent-branch");
    assert!(result.is_ok(), "Fetch should fail gracefully for non-existent branch");

    Ok(())
}

/// Test git operations with untracked files
#[test]
fn test_git_operations_dirty_working_directory() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Initially clean
    assert!(git_ops.is_working_directory_clean()?);

    // Add untracked file
    fs::write(test_env.path().join("untracked.txt"), "untracked content")?;
    assert!(!git_ops.is_working_directory_clean()?);

    // Add staged file
    Command::new("git").args(&["add", "untracked.txt"]).current_dir(test_env.path()).output()?;
    assert!(!git_ops.is_working_directory_clean()?);

    // Commit to make clean again
    Command::new("git").args(&["commit", "-m", "Add untracked file"]).current_dir(test_env.path()).output()?;
    assert!(git_ops.is_working_directory_clean()?);

    Ok(())
}

/// Test git operations with detached HEAD
#[test]
fn test_git_operations_detached_head() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Get commit hash
    let commit_hash_output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(test_env.path())
        .output()?;
    let commit_hash_binding = String::from_utf8_lossy(&commit_hash_output.stdout);
    let commit_hash_str = commit_hash_binding.trim();

    // Switch to detached HEAD
    Command::new("git").args(&["checkout", &commit_hash_str]).current_dir(test_env.path()).output()?;

    // Test get_current_branch with detached HEAD
    let current_branch = git_ops.get_current_branch()?;
    assert!(current_branch.starts_with("detached-HEAD-"));

    Ok(())
}

/// Test git operations file reading and writing
#[test]
fn test_git_operations_file_operations() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    // Create test branch
    Command::new("git").args(&["checkout", "-b", "test-branch"]).current_dir(test_env.path()).output()?;
    fs::write(test_env.path().join("test.txt"), "test content")?;
    Command::new("git").args(&["add", "test.txt"]).current_dir(test_env.path()).output()?;
    Command::new("git").args(&["commit", "-m", "Add test file"]).current_dir(test_env.path()).output()?;
    Command::new("git").args(&["checkout", "main"]).current_dir(test_env.path()).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Test read_file_from_branch
    let content = git_ops.read_file_from_branch("test-branch", "test.txt")?;
    assert_eq!(content, "test content");

    // Test read_file_from_branch with non-existent file
    let result = git_ops.read_file_from_branch("test-branch", "nonexistent.txt");
    assert!(result.is_err(), "Should fail to read non-existent file");

    Ok(())
}

/// Test git operations with invalid git repository
#[test]
fn test_git_operations_invalid_repository() -> Result<()> {
    // Test with non-git directory
    let result = hitch::utils::git_operations::GitOperations::new();
    assert!(result.is_err(), "Should fail to create GitOperations in non-git directory");

    Ok(())
}

/// Test git operations push functionality
#[test]
fn test_git_operations_push() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Test push_branch (should fail gracefully with no remote)
    let result = git_ops.push_branch("main");
    assert!(result.is_err(), "Push should fail gracefully with no remote configured");

    Ok(())
}

/// Test git operations merge functionality
#[test]
fn test_git_operations_merge() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Create feature branch
    Command::new("git").args(&["checkout", "-b", "feature"]).current_dir(test_env.path()).output()?;
    fs::write(test_env.path().join("feature.txt"), "feature content")?;
    Command::new("git").args(&["add", "feature.txt"]).current_dir(test_env.path()).output()?;
    Command::new("git").args(&["commit", "-m", "Add feature"]).current_dir(test_env.path()).output()?;
    Command::new("git").args(&["checkout", "main"]).current_dir(test_env.path()).output()?;

    // Test squash_merge
    let result = git_ops.squash_merge("feature", "Squash merge feature branch");
    assert!(result.is_ok(), "Squash merge should succeed");

    // Verify merge
    assert!(fs::metadata(test_env.path().join("feature.txt")).is_ok());

    // Test check_merge_conflicts
    let has_conflicts = git_ops.check_merge_conflicts("feature")?;
    assert!(!has_conflicts, "Should have no conflicts");

    Ok(())
}

/// Test git operations error handling
#[test]
fn test_git_operations_error_handling() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(test_env.path().to_str().unwrap())?;

    // Test checkout to non-existent branch
    let result = git_ops.checkout_branch("nonexistent");
    assert!(result.is_err(), "Should fail to checkout non-existent branch");

    // Test squash_merge with non-existent branch
    let result = git_ops.squash_merge("nonexistent", "Should fail");
    assert!(result.is_err(), "Should fail to squash merge non-existent branch");

    // Test check_merge_conflicts with non-existent branch
    let result = git_ops.check_merge_conflicts("nonexistent");
    assert!(result.is_err(), "Should fail to check conflicts for non-existent branch");

    Ok(())
}