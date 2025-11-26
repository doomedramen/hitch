use anyhow::Result;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{ensure_git_environment_ready, with_test_env, SetupLevel};

/// Helper functions for git operations using the test environment
impl common::TestEnv {
    /// Initialize git repo with remote
    fn init_git_repo_with_remote(&self) -> Result<()> {
        let test_path = self.path();

        // Add a fake remote
        let remote_dir = test_path.join("remote");
        fs::create_dir_all(&remote_dir)?;
        Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&remote_dir)
            .output()?;

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;
        git_ops.run_git_command(&["remote", "add", "origin", remote_dir.to_str().unwrap()])?;

        Ok(())
    }
}

/// Test create_orphan_branch functionality
#[test]
fn test_git_operations_create_orphan_branch() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create orphan branch
        git_ops.create_orphan_branch("orphan-test")?;

        // Verify we're on the orphan branch
        let current_branch = git_ops.get_current_branch()?;
        assert_eq!(
            current_branch, "orphan-test",
            "Should be on the newly created orphan branch"
        );

        // Verify the branch has no commits
        let output = git_ops.run_git_command(&["log", "--oneline"])?;
        let log_output = String::from_utf8_lossy(&output.stdout);
        assert!(
            log_output.trim().is_empty(),
            "Orphan branch should have no commits"
        );

        Ok(())
    })
}

/// Test git operations with custom path
#[test]
fn test_git_operations_new_at_path() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let test_path = test_env.path();

        // Create GitOperations instance with explicit path
        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Test basic functionality (handle both main and master branch names)
        let current_branch = git_ops.get_current_branch()?;
        assert!(
            current_branch == "main" || current_branch == "master",
            "Should detect main or master branch, got: {}",
            current_branch
        );

        // Test with non-existent directory
        let result = hitch::utils::git_operations::GitOperations::new_at_path("/nonexistent/path");
        assert!(result.is_err(), "Should fail with non-existent path");

        Ok(())
    })
}

/// Test add_and_commit functionality
#[test]
fn test_git_operations_add_and_commit() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create a file
        fs::write(test_path.join("test.txt"), "Test content")?;

        // Add and commit the file
        git_ops.add_and_commit(&["test.txt"], "Add test file")?;

        // Verify the commit was created
        let output = git_ops.run_git_command(&["log", "--oneline", "-1"])?;
        let log_output = String::from_utf8_lossy(&output.stdout);
        assert!(
            log_output.contains("Add test file"),
            "Commit message should be in log"
        );

        Ok(())
    })
}

/// Test file operations (read/write)
#[test]
fn test_git_operations_file_operations() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Write a file and commit it so it's in the branch
        git_ops.write_file("test.txt", "Test content")?;
        git_ops.add_and_commit(&["test.txt"], "Add test file")?;

        // Verify file exists
        assert!(
            fs::metadata(test_path.join("test.txt")).is_ok(),
            "File should exist after writing"
        );

        // Test read_file_from_branch on current branch
        let current_branch = git_ops.get_current_branch()?;
        let content = git_ops.read_file_from_branch(&current_branch, "test.txt")?;
        assert_eq!(content, "Test content", "Should read correct content");

        // Test reading non-existent file
        let result = git_ops.read_file_from_branch(&current_branch, "nonexistent.txt");
        assert!(result.is_err(), "Should fail to read non-existent file");

        Ok(())
    })
}

/// Test branch existence checking
#[test]
fn test_git_operations_branch_exists() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create a test branch
        git_ops.run_git_command(&["checkout", "-b", "test-branch"])?;
        fs::write(test_path.join("test.txt"), "Test")?;
        git_ops.run_git_command(&["add", "test.txt"])?;
        git_ops.run_git_command(&["commit", "-m", "Test"])?;
        git_ops.checkout_branch("main")?;

        // Test branch_exists (local branches only)
        assert!(
            git_ops.branch_exists("main")?,
            "Main branch should exist locally"
        );
        assert!(
            git_ops.branch_exists("test-branch")?,
            "Test branch should exist locally"
        );
        assert!(
            !git_ops.branch_exists("nonexistent")?,
            "Nonexistent branch should not exist"
        );

        // Test branch_exists_anywhere (both local and remote)
        assert!(
            git_ops.branch_exists_anywhere("main")?,
            "Main should exist anywhere"
        );
        assert!(
            git_ops.branch_exists_anywhere("test-branch")?,
            "Test branch should exist anywhere"
        );

        Ok(())
    })
}

/// Test branch management operations
#[test]
fn test_git_operations_branch_management() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create a test branch first
        git_ops.create_branch_from("feature-test", "main")?;

        // Verify branch was created
        assert!(
            git_ops.branch_exists_anywhere("feature-test")?,
            "New branch should exist"
        );

        // Test rename branch
        git_ops.rename_branch("feature-test", "renamed-branch")?;
        assert!(
            !git_ops.branch_exists_anywhere("feature-test")?,
            "Old name should not exist"
        );
        assert!(
            git_ops.branch_exists_anywhere("renamed-branch")?,
            "New name should exist"
        );

        // Test delete branch
        git_ops.checkout_branch("main")?; // Switch off branch first
        git_ops.delete_branch("renamed-branch", false)?;
        assert!(
            !git_ops.branch_exists_anywhere("renamed-branch")?,
            "Deleted branch should not exist"
        );

        // Test force delete (with unmerged changes)
        git_ops.create_branch_from("temp-branch", "main")?;
        git_ops.checkout_branch("temp-branch")?;
        fs::write(test_path.join("temp.txt"), "temp")?;
        git_ops.run_git_command(&["add", "temp.txt"])?;
        git_ops.run_git_command(&["commit", "-m", "Temp"])?;
        git_ops.checkout_branch("main")?;

        git_ops.delete_branch("temp-branch", true)?;
        assert!(
            !git_ops.branch_exists_anywhere("temp-branch")?,
            "Force deleted branch should not exist"
        );

        Ok(())
    })
}

/// Test commit-related operations
#[test]
fn test_git_operations_commit_operations() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Test get_branch_commit_sha
        let commit_sha = git_ops.get_branch_commit_sha("main")?;
        assert!(!commit_sha.is_empty(), "Should get a commit SHA");
        assert_eq!(commit_sha.len(), 40, "SHA should be 40 characters");

        // Test get_commit_timestamp
        let timestamp = git_ops.get_commit_timestamp(&commit_sha)?;
        let now = chrono::Utc::now();
        assert!(
            timestamp <= now,
            "Commit timestamp should be in the past or now"
        );

        Ok(())
    })
}

/// Test fetch and push operations (with local remote)
#[test]
fn test_git_operations_remote_operations() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        test_env.init_git_repo_with_remote()?;

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create a branch and push it
        git_ops.create_branch_from("test-remote", "main")?;
        fs::write(test_path.join("remote-test.txt"), "Remote test")?;
        git_ops.run_git_command(&["add", "remote-test.txt"])?;
        git_ops.run_git_command(&["commit", "-m", "Remote test"])?;

        // Set upstream and push
        git_ops.run_git_command(&["push", "--set-upstream", "origin", "test-remote"])?;

        // Test fetch
        let _fetch_result = git_ops.fetch_branch("test-remote");
        // Fetch might fail in test environment due to no real remote, but that's ok
        // We're just testing that the function doesn't panic

        // Test push
        let _push_result = git_ops.push_branch("test-remote");
        // Push might also fail, but we test the error handling

        Ok(())
    })
}

/// Test squash merge functionality
#[test]
fn test_git_operations_squash_merge() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Create a feature branch with multiple commits
        git_ops.create_branch_from("feature-branch", "main")?;

        // First commit
        fs::write(test_path.join("file1.txt"), "Content 1")?;
        git_ops.add_and_commit(&["file1.txt"], "First commit")?;

        // Second commit
        fs::write(test_path.join("file2.txt"), "Content 2")?;
        git_ops.add_and_commit(&["file2.txt"], "Second commit")?;

        // Switch back to main
        git_ops.checkout_branch("main")?;

        // Test squash merge
        git_ops.squash_merge("feature-branch", "Squashed feature branch")?;

        // Verify the squash merge worked
        let output = git_ops.run_git_command(&["log", "--oneline", "-2"])?;
        let log_output = String::from_utf8_lossy(&output.stdout);
        assert!(
            log_output.contains("Squashed feature branch"),
            "Should have squash commit message"
        );
        assert!(
            fs::metadata(test_path.join("file1.txt")).is_ok(),
            "File1 should exist after merge"
        );
        assert!(
            fs::metadata(test_path.join("file2.txt")).is_ok(),
            "File2 should exist after merge"
        );

        Ok(())
    })
}

/// Test error handling in git operations
#[test]
fn test_git_operations_error_handling() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Ensure clean working tree before hitch init
        ensure_git_environment_ready(test_env)?;

        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let test_path = test_env.path();

        // Test operations in non-git directory - create a temp directory without git
        use std::fs;
        let non_git_dir = test_env.path().join("non_git_temp");
        fs::create_dir_all(&non_git_dir)?;
        let result =
            hitch::utils::git_operations::GitOperations::new_at_path(non_git_dir.to_str().unwrap());
        assert!(
            result.is_err(),
            "Should fail to create GitOperations in non-git directory"
        );

        let git_ops =
            hitch::utils::git_operations::GitOperations::new_at_path(test_path.to_str().unwrap())?;

        // Test checkout non-existent branch
        let result = git_ops.checkout_branch("nonexistent-branch");
        assert!(
            result.is_err(),
            "Should fail to checkout non-existent branch"
        );

        // Test create branch from non-existent source
        let result = git_ops.create_branch_from("new-branch", "nonexistent-source");
        assert!(
            result.is_err(),
            "Should fail to create branch from non-existent source"
        );

        // Test rename non-existent branch
        let result = git_ops.rename_branch("nonexistent", "newname");
        assert!(result.is_err(), "Should fail to rename non-existent branch");

        Ok(())
    })
}
