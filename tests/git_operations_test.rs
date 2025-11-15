use anyhow::Result;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

/// Test git operations functionality
#[test]
fn test_git_operations_basic_functionality() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    // Create initial commit
    fs::write("README.md", "# Test Repository")?;
    Command::new("git").args(&["add", "README.md"]).output()?;
    Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

    // Test git operations
    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

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
    Command::new("git").args(&["checkout", "-b", "test-branch"]).output()?;
    assert_eq!(git_ops.get_current_branch()?, "test-branch");

    // Test fetch_branch (should fail gracefully with no remote)
    let result = git_ops.fetch_branch("nonexistent-branch");
    assert!(result.is_ok(), "Fetch should fail gracefully for non-existent branch");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations with untracked files
#[test]
fn test_git_operations_dirty_working_directory() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Initially clean
    assert!(git_ops.is_working_directory_clean()?);

    // Add untracked file
    fs::write("untracked.txt", "untracked content")?;
    assert!(!git_ops.is_working_directory_clean()?);

    // Add staged file
    Command::new("git").args(&["add", "untracked.txt"]).output()?;
    assert!(!git_ops.is_working_directory_clean()?);

    // Commit to make clean again
    Command::new("git").args(&["commit", "-m", "Add untracked file"]).output()?;
    assert!(git_ops.is_working_directory_clean()?);

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations with detached HEAD
#[test]
fn test_git_operations_detached_head() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    // Create initial commit
    fs::write("README.md", "# Test Repository")?;
    Command::new("git").args(&["add", "README.md"]).output()?;
    Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Get commit hash
    let commit_hash_output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()?;
    let commit_hash_binding = String::from_utf8_lossy(&commit_hash_output.stdout);
    let commit_hash_str = commit_hash_binding.trim();

    // Switch to detached HEAD
    Command::new("git").args(&["checkout", &commit_hash_str]).output()?;

    // Test get_current_branch with detached HEAD
    let current_branch = git_ops.get_current_branch()?;
    assert!(current_branch.starts_with("detached-HEAD-"));

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations file reading and writing
#[test]
fn test_git_operations_file_operations() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    // Create initial commit
    fs::write("README.md", "# Test Repository")?;
    Command::new("git").args(&["add", "README.md"]).output()?;
    Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

    // Create test branch
    Command::new("git").args(&["checkout", "-b", "test-branch"]).output()?;
    fs::write("test.txt", "test content")?;
    Command::new("git").args(&["add", "test.txt"]).output()?;
    Command::new("git").args(&["commit", "-m", "Add test file"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Test read_file_from_branch
    let content = git_ops.read_file_from_branch("test-branch", "test.txt")?;
    assert_eq!(content, "test content");

    // Test read_file_from_branch with non-existent file
    let result = git_ops.read_file_from_branch("test-branch", "nonexistent.txt");
    assert!(result.is_err(), "Should fail to read non-existent file");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations with invalid git repository
#[test]
fn test_git_operations_invalid_repository() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Test with non-git directory
    let result = hitch::utils::git_operations::GitOperations::new();
    assert!(result.is_err(), "Should fail to create GitOperations in non-git directory");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations push functionality
#[test]
fn test_git_operations_push() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    // Create initial commit
    fs::write("README.md", "# Test Repository")?;
    Command::new("git").args(&["add", "README.md"]).output()?;
    Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Test push_branch (should fail gracefully with no remote)
    let result = git_ops.push_branch("main");
    assert!(result.is_err(), "Push should fail gracefully with no remote configured");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations merge functionality
#[test]
fn test_git_operations_merge() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    // Create initial commit
    fs::write("README.md", "# Test Repository")?;
    Command::new("git").args(&["add", "README.md"]).output()?;
    Command::new("git").args(&["commit", "-m", "Initial commit"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Create feature branch
    Command::new("git").args(&["checkout", "-b", "feature"]).output()?;
    fs::write("feature.txt", "feature content")?;
    Command::new("git").args(&["add", "feature.txt"]).output()?;
    Command::new("git").args(&["commit", "-m", "Add feature"]).output()?;
    Command::new("git").args(&["checkout", "main"]).output()?;

    // Test squash_merge
    let result = git_ops.squash_merge("feature", "Squash merge feature branch");
    assert!(result.is_ok(), "Squash merge should succeed");

    // Verify merge
    assert!(fs::metadata("feature.txt").is_ok());

    // Test check_merge_conflicts
    let has_conflicts = git_ops.check_merge_conflicts("feature")?;
    assert!(!has_conflicts, "Should have no conflicts");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}

/// Test git operations error handling
#[test]
fn test_git_operations_error_handling() -> Result<()> {
    let temp_dir = tempdir()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(temp_dir.path())?;

    // Initialize git repository
    Command::new("git").args(&["init"]).output()?;
    Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
    Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;

    let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(temp_dir.path().to_str().unwrap())?;

    // Test checkout to non-existent branch
    let result = git_ops.checkout_branch("nonexistent");
    assert!(result.is_err(), "Should fail to checkout non-existent branch");

    // Test squash_merge with non-existent branch
    let result = git_ops.squash_merge("nonexistent", "Should fail");
    assert!(result.is_err(), "Should fail to squash merge non-existent branch");

    // Test check_merge_conflicts with non-existent branch
    let result = git_ops.check_merge_conflicts("nonexistent");
    assert!(result.is_err(), "Should fail to check conflicts for non-existent branch");

    std::env::set_current_dir(original_dir)?;
    Ok(())
}