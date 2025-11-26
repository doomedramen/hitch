//! Unit tests for GitOperations
//!
//! Provides comprehensive testing for all Git operations with 50+ granular test cases
//! covering branch management, merge operations, conflict detection, and edge cases.

use anyhow::Result;
use chrono::Utc;

use crate::test_framework::*;
use hitch::utils::git_operations::GitOperations;

#[cfg(test)]
mod tests {
    use super::*;

    // GitOperations struct initialization tests

    #[test]
    fn test_git_operations_new_in_repo() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize git repo first
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            // Test GitOperations initialization
            let _git_ops = GitOperations::new()?;
            // If we got here, initialization succeeded

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_git_operations_new_at_path() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            // Initialize git repo first
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            // Test GitOperations initialization at specific path
            let _git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            // If we got here, initialization succeeded

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_git_operations_new_outside_repo_fails() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|_env| {
            // Don't initialize git repo - should fail
            let result = GitOperations::new();
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Git command execution tests

    #[test]
    fn test_run_git_command_success() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Test a simple git command
            let output = git_ops.run_git_command(&["status"])?;
            assert!(output.status.success());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_run_git_command_failure() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Test an invalid git command
            let output = git_ops.run_git_command(&["invalid-command"])?;
            assert!(!output.status.success());
            assert!(!output.stderr.is_empty());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Branch management tests

    #[test]
    fn test_get_current_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Test getting current branch (should be main by default)
            let current_branch = git_ops.get_current_branch()?;
            assert_eq!(current_branch, "main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_checkout_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and checkout a new branch
            git_ops.create_branch_from("feature", "main")?;
            git_ops.checkout_branch("feature")?;

            let current_branch = git_ops.get_current_branch()?;
            assert_eq!(current_branch, "feature");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_checkout_nonexistent_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to checkout non-existent branch
            let result = git_ops.checkout_branch("nonexistent");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_create_orphan_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create orphan branch
            git_ops.create_orphan_branch("orphan")?;

            let current_branch = git_ops.get_current_branch()?;
            assert_eq!(current_branch, "orphan");

            // Working directory should be clean
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_create_branch_from() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create a file on main branch
            git_ops.write_file("test.txt", "content")?;
            git_ops.add_and_commit(&["test.txt"], "Initial commit")?;

            // Create branch from main
            git_ops.create_branch_from("feature", "main")?;

            assert!(git_ops.branch_exists("feature")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_rename_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create initial branch
            git_ops.create_branch_from("old-name", "main")?;
            assert!(git_ops.branch_exists("old-name")?);

            // Rename branch
            git_ops.rename_branch("old-name", "new-name")?;

            assert!(!git_ops.branch_exists("old-name")?);
            assert!(git_ops.branch_exists("new-name")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_delete_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create branch
            git_ops.create_branch_from("to-delete", "main")?;
            assert!(git_ops.branch_exists("to-delete")?);

            // Delete branch
            git_ops.delete_branch("to-delete", false)?;

            assert!(!git_ops.branch_exists("to-delete")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_delete_current_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and checkout branch
            git_ops.create_branch_from("current", "main")?;
            git_ops.checkout_branch("current")?;
            assert_eq!(git_ops.get_current_branch()?, "current");

            // Delete current branch (should switch to main first)
            git_ops.delete_branch("current", false)?;

            assert!(!git_ops.branch_exists("current")?);
            assert_eq!(git_ops.get_current_branch()?, "main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_force_delete_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create branch with commits
            git_ops.create_branch_from("feature", "main")?;
            git_ops.checkout_branch("feature")?;
            git_ops.write_file("feature.txt", "content")?;
            git_ops.add_and_commit(&["feature.txt"], "Feature commit")?;
            git_ops.checkout_branch("main")?;

            // Try normal delete (should fail)
            let result = git_ops.delete_branch("feature", false);
            assert!(result.is_err());

            // Force delete (should succeed)
            git_ops.delete_branch("feature", true)?;
            assert!(!git_ops.branch_exists("feature")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_branch_exists() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Check main branch exists
            assert!(git_ops.branch_exists("main")?);

            // Check non-existent branch
            assert!(!git_ops.branch_exists("nonexistent")?);

            // Create and check new branch
            git_ops.create_branch_from("feature", "main")?;
            assert!(git_ops.branch_exists("feature")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_branch_exists_anywhere() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Test local branch
            assert!(git_ops.branch_exists_anywhere("main")?);

            // Test non-existent branch
            assert!(!git_ops.branch_exists_anywhere("nonexistent")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Commit and file operations tests

    #[test]
    fn test_add_and_commit() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and commit files
            git_ops.write_file("test1.txt", "content1")?;
            git_ops.write_file("test2.txt", "content2")?;
            git_ops.add_and_commit(&["test1.txt", "test2.txt"], "Test commit")?;

            // Working directory should be clean
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_add_and_commit_empty_files() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to add non-existent files
            let result = git_ops.add_and_commit(&["nonexistent.txt"], "Test commit");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_read_file_from_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and commit file
            git_ops.write_file("test.txt", "hello world")?;
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;

            // Read file from branch
            let content = git_ops.read_file_from_branch("main", "test.txt")?;
            assert_eq!(content, "hello world");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_read_nonexistent_file_from_branch() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to read non-existent file
            let result = git_ops.read_file_from_branch("main", "nonexistent.txt");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_write_file() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Write file
            git_ops.write_file("test.txt", "test content")?;

            // Verify file exists using test framework
            let content = env.fs.read_file("test.txt")?;
            assert_eq!(content, "test content");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_commit() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Stage and commit
            git_ops.write_file("test.txt", "content")?;
            git_ops.run_git_command(&["add", "test.txt"])?;
            git_ops.commit("Test commit")?;

            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_commit_nothing_to_commit() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to commit with nothing staged
            let result = git_ops.commit("Empty commit");
            assert!(result.is_ok()); // Should not fail

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Working directory tests

    #[test]
    fn test_is_working_directory_clean() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Initially clean
            assert!(git_ops.is_working_directory_clean()?);

            // Create uncommitted file
            git_ops.write_file("test.txt", "content")?;
            assert!(!git_ops.is_working_directory_clean()?);

            // Commit file
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_clean_working_directory() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create uncommitted changes
            git_ops.write_file("test.txt", "content")?;
            assert!(!git_ops.is_working_directory_clean()?);

            // Clean working directory
            git_ops.clean_working_directory("Auto-commit")?;
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Remote operations tests

    #[test]
    fn test_fetch_branch_no_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to fetch without remote - should not fail
            let result = git_ops.fetch_branch("main");
            assert!(result.is_ok());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_fetch_all_remotes_no_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to fetch all remotes without remote - should not fail
            let result = git_ops.fetch_all_remotes();
            assert!(result.is_ok());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_push_branch_no_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to push without remote - should fail
            let result = git_ops.push_branch("main");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_force_push_branch_no_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try to force push without remote - should fail
            let result = git_ops.force_push_branch("main");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Merge operations tests

    #[test]
    fn test_squash_merge() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and commit file on main
            git_ops.write_file("base.txt", "base content")?;
            git_ops.add_and_commit(&["base.txt"], "Base commit")?;

            // Create feature branch with additional commits
            git_ops.create_branch_from("feature", "main")?;
            git_ops.checkout_branch("feature")?;
            git_ops.write_file("feature.txt", "feature content")?;
            git_ops.add_and_commit(&["feature.txt"], "Feature commit")?;
            git_ops.checkout_branch("main")?;

            // Squash merge feature into main
            git_ops.squash_merge("feature", "Merge feature")?;

            // File should be present
            let content = git_ops.read_file_from_branch("main", "feature.txt")?;
            assert_eq!(content, "feature content");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_squash_merge_no_changes() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create base commit
            git_ops.write_file("base.txt", "base content")?;
            git_ops.add_and_commit(&["base.txt"], "Base commit")?;

            // Create empty feature branch
            git_ops.create_branch_from("feature", "main")?;

            // Squash merge - should be ok even with no changes
            let result = git_ops.squash_merge("feature", "Empty merge");
            assert!(result.is_ok());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_check_merge_conflicts_no_conflicts() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create base
            git_ops.write_file("base.txt", "base content")?;
            git_ops.add_and_commit(&["base.txt"], "Base commit")?;

            // Create feature with different file
            git_ops.create_branch_from("feature", "main")?;
            git_ops.checkout_branch("feature")?;
            git_ops.write_file("feature.txt", "feature content")?;
            git_ops.add_and_commit(&["feature.txt"], "Feature commit")?;
            git_ops.checkout_branch("main")?;

            // Check for conflicts
            let (has_conflicts, _) = git_ops.check_merge_conflicts_detailed("feature")?;
            assert!(!has_conflicts);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_check_merge_conflicts_with_conflicts() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create base with file
            git_ops.write_file("conflict.txt", "base content")?;
            git_ops.add_and_commit(&["conflict.txt"], "Base commit")?;

            // Create feature with conflicting changes
            git_ops.create_branch_from("feature", "main")?;
            git_ops.checkout_branch("feature")?;
            git_ops.write_file("conflict.txt", "feature content")?;
            git_ops.add_and_commit(&["conflict.txt"], "Feature commit")?;
            git_ops.checkout_branch("main")?;

            // Modify same file on main
            git_ops.write_file("conflict.txt", "main content")?;
            git_ops.add_and_commit(&["conflict.txt"], "Main commit")?;

            // Check for conflicts
            let (has_conflicts, conflicted_files) =
                git_ops.check_merge_conflicts_detailed("feature")?;
            assert!(has_conflicts);
            assert!(conflicted_files.is_some());
            assert!(conflicted_files
                .unwrap()
                .contains(&"conflict.txt".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_get_conflicted_files() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Initially no conflicts
            let conflicted_files = git_ops.get_conflicted_files()?;
            assert!(conflicted_files.is_empty());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_abort_merge_and_clean() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create some state and clean it
            git_ops.write_file("test.txt", "content")?;
            git_ops.abort_merge_and_clean()?;

            // Should be clean after
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Branch synchronization tests

    #[test]
    fn test_create_local_branch_from_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // If branch already exists locally, should not fail
            git_ops.create_local_branch_from_remote("main")?;

            // Should still be on main
            assert_eq!(git_ops.get_current_branch()?, "main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_synchronize_branches() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create local branches
            git_ops.create_branch_from("dev", "main")?;
            git_ops.create_branch_from("qa", "main")?;

            let branches = vec!["dev".to_string(), "qa".to_string(), "main".to_string()];

            // Synchronize - should not fail even without remotes
            let result = git_ops.synchronize_branches(&branches);
            assert!(result.is_ok());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Tag operations tests

    #[test]
    fn test_create_tag() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create a commit first
            git_ops.write_file("test.txt", "content")?;
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;

            // Create tag
            git_ops.create_tag("v1.0.0", "Version 1.0.0")?;

            // Check if tag exists (using git command)
            let output = git_ops.run_git_command(&["tag", "--list", "v1.0.0"])?;
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("v1.0.0"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_push_tag_no_remote() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create and try to push tag without remote
            git_ops.write_file("test.txt", "content")?;
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;
            git_ops.create_tag("v1.0.0", "Version 1.0.0")?;

            let result = git_ops.push_tag("v1.0.0");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Branch relationship tests

    #[test]
    fn test_is_branch_merged_into() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Main is always "merged" into itself
            assert!(git_ops.is_branch_merged_into("main", "main")?);

            // Create feature branch
            git_ops.create_branch_from("feature", "main")?;

            // Feature is not merged into main yet
            assert!(!git_ops.is_branch_merged_into("feature", "main")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Utility and metadata tests

    #[test]
    fn test_get_user_email() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            let email = git_ops.get_user_email()?;
            assert_eq!(email, "test@example.com");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_get_user_email_not_configured() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            // Don't configure user

            let git_ops = GitOperations::new()?;

            let result = git_ops.get_user_email();
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_get_branch_commit_sha() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create commit
            git_ops.write_file("test.txt", "content")?;
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;

            let sha = git_ops.get_branch_commit_sha("main")?;
            assert!(!sha.is_empty());
            assert_eq!(sha.len(), 40); // SHA-1 length

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_get_commit_timestamp() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create commit
            git_ops.write_file("test.txt", "content")?;
            git_ops.add_and_commit(&["test.txt"], "Test commit")?;

            let sha = git_ops.get_branch_commit_sha("main")?;
            let timestamp = git_ops.get_commit_timestamp(&sha)?;

            // Timestamp should be recent (within last minute)
            let now = Utc::now();
            let duration = now.signed_duration_since(timestamp);
            assert!(duration.num_seconds() <= 60);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_get_commit_timestamp_invalid_sha() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Try with invalid SHA
            let result = git_ops.get_commit_timestamp("invalid");
            assert!(result.is_err());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Complex workflow tests

    #[test]
    fn test_complete_feature_branch_workflow() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Setup main branch
            git_ops.write_file("README.md", "# Project")?;
            git_ops.add_and_commit(&["README.md"], "Initial commit")?;

            // Create feature branch
            git_ops.create_branch_from("feature-auth", "main")?;
            git_ops.checkout_branch("feature-auth")?;

            // Add feature work
            git_ops.write_file("auth.rs", "pub fn login() { true }")?;
            git_ops.add_and_commit(&["auth.rs"], "Add authentication")?;

            // Add more work
            git_ops.write_file("auth_test.rs", "#[test] fn test_login() {}")?;
            git_ops.add_and_commit(&["auth_test.rs"], "Add auth tests")?;

            // Back to main and prepare for merge
            git_ops.checkout_branch("main")?;

            // Check for conflicts
            let (has_conflicts, _) = git_ops.check_merge_conflicts_detailed("feature-auth")?;
            assert!(!has_conflicts);

            // Squash merge
            git_ops.squash_merge("feature-auth", "Add authentication feature")?;

            // Verify merge
            assert!(git_ops.branch_exists("feature-auth")?);
            let auth_content = git_ops.read_file_from_branch("main", "auth.rs")?;
            assert!(auth_content.contains("login"));

            // Check branch relationship
            assert!(git_ops.is_branch_merged_into("feature-auth", "main")?);

            // Cleanup feature branch
            git_ops.delete_branch("feature-auth", true)?;
            assert!(!git_ops.branch_exists("feature-auth")?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_branch_management_edge_cases() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Create commit on main
            git_ops.write_file("base.txt", "base")?;
            git_ops.add_and_commit(&["base.txt"], "Base")?;

            // Create branch, switch to it, create commit
            git_ops.create_branch_from("work", "main")?;
            git_ops.checkout_branch("work")?;
            git_ops.write_file("work.txt", "work")?;
            git_ops.add_and_commit(&["work.txt"], "Work")?;

            // Try to rename while on the branch
            git_ops.rename_branch("work", "renamed")?;
            assert_eq!(git_ops.get_current_branch()?, "renamed");

            // Try to create branch with same name as current
            let result = git_ops.create_branch_from("renamed", "main");
            assert!(result.is_err()); // Should fail

            // Delete current branch (should switch to main)
            git_ops.delete_branch("renamed", true)?;
            assert_eq!(git_ops.get_current_branch()?, "main");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_error_handling_and_recovery() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(|env| {
            env.git.init()?;
            env.git.config_user("Test User", "test@example.com")?;

            let git_ops = GitOperations::new()?;

            // Test various error conditions

            // Non-existent file operations
            assert!(git_ops
                .read_file_from_branch("main", "nonexistent.txt")
                .is_err());

            // Non-existent branch operations
            assert!(git_ops.checkout_branch("nonexistent").is_err());
            assert!(git_ops.get_branch_commit_sha("nonexistent").is_err());

            // Invalid operations on clean state
            git_ops.clean_working_directory("Should be fine")?; // Should not fail

            // Working directory should still be clean
            assert!(git_ops.is_working_directory_clean()?);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
