use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
mod error_handling_tests {
    use super::*;

    /// Helper to ensure working tree is clean before hitch operations
    fn ensure_clean_working_tree(test_env: &TestEnv) -> Result<()> {
        // Clean up any existing changes first
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(test_env.path())
            .output()?;

        let status_str = String::from_utf8_lossy(&status_output.stdout);

        if !status_str.trim().is_empty() {
            // There are uncommitted changes, add and commit them
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(test_env.path())
                .output()?;

            let commit_output = Command::new("git")
                .args(["commit", "-m", "Clean up test environment"])
                .current_dir(test_env.path())
                .output()?;

            // Don't treat "nothing to commit" as an error
            if !commit_output.status.success() {
                let stderr = String::from_utf8_lossy(&commit_output.stderr);
                let stdout = String::from_utf8_lossy(&commit_output.stdout);
                if !(stderr.contains("nothing to commit") || stdout.contains("nothing to commit")) {
                    return Err(anyhow::anyhow!(
                        "Failed to commit: stderr={}, stdout={}",
                        stderr,
                        stdout
                    ));
                }
            }
        }

        Ok(())
    }

    /// Helper to clean up after hitch init (it leaves the working tree dirty)
    fn cleanup_after_hitch_init(test_env: &TestEnv) -> Result<()> {
        // Check git status after hitch init
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(test_env.path())
            .output()?;

        let status_str = String::from_utf8_lossy(&status_output.stdout);

        if !status_str.trim().is_empty() {
            // Hitch init leaves changes (hitch.json), commit them
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add hitch configuration"])
                .current_dir(test_env.path())
                .output()?;
        }

        Ok(())
    }

    /// Helper to run hitch command in test environment
    fn run_hitch_command(test_env: &TestEnv, args: &[&str]) -> Result<std::process::Output> {
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .output()?;

        Ok(output)
    }

    /// Helper to run hitch command and expect failure
    fn run_hitch_command_expect_failure(test_env: &TestEnv, args: &[&str]) -> Result<std::process::Output> {
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .output()?;

        if output.status.success() {
            return Err(anyhow::anyhow!(
                "Expected hitch command to fail, but it succeeded: hitch {}",
                args.join(" ")
            ));
        }

        Ok(output)
    }

    /// Helper to create and commit a file
    fn create_and_commit_file(test_env: &TestEnv, filename: &str, content: &str) -> Result<()> {
        let file_path = test_env.path().join(filename);
        std::fs::write(file_path, content)?;

        Command::new("git")
            .args(["add", filename])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", &format!("Add {}", filename)])
            .current_dir(test_env.path())
            .output()?;

        Ok(())
    }

    /// Helper to create a branch
    fn create_branch(test_env: &TestEnv, branch_name: &str) -> Result<()> {
        Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(test_env.path())
            .output()?;

        Ok(())
    }

    /// Test merge conflict error during promote operation
    #[test]
    fn test_promote_merge_conflict_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create a conflicting file on main branch
            create_and_commit_file(test_env, "config.txt", "main config content")?;

            // Create feature branch with conflicting changes
            create_branch(test_env, "feature1")?;
            create_and_commit_file(test_env, "config.txt", "feature config content")?;

            // Try to promote conflicting feature branch - should fail
            let output = run_hitch_command_expect_failure(test_env, &["promote", "feature1", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show merge conflict error
            assert!(stderr.contains("Merge conflict detected") ||
                   stderr.contains("conflict") ||
                   stderr.contains("resolve conflicts"));

            Ok(())
        })
    }

    /// Test rebuild conflict when promoted branches have conflicts
    #[test]
    fn test_rebuild_with_conflicting_promoted_branches() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create first feature branch with config file
            create_and_commit_file(test_env, "config.txt", "feature1 config")?;
            create_branch(test_env, "feature1")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Promote first feature
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create second feature branch with conflicting config
            create_and_commit_file(test_env, "config.txt", "feature2 config")?;
            create_branch(test_env, "feature2")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Promote second feature
            run_hitch_command(test_env, &["promote", "feature2", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to rebuild - should detect conflicts
            let output = run_hitch_command_expect_failure(test_env, &["rebuild", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show conflict error
            assert!(stderr.contains("conflict") ||
                   stderr.contains("Merge conflict") ||
                   stderr.contains("resolve"));

            Ok(())
        })
    }

    /// Test error when environment doesn't exist
    #[test]
    fn test_rebuild_nonexistent_environment_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Try to rebuild non-existent environment
            let output = run_hitch_command_expect_failure(test_env, &["rebuild", "nonexistent"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show environment not found error
            assert!(stderr.contains("not found") ||
                   stderr.contains("does not exist") ||
                   stderr.contains("No such environment"));

            Ok(())
        })
    }

    /// Test error when trying to promote non-existent branch
    #[test]
    fn test_promote_nonexistent_branch_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to promote non-existent branch
            let output = run_hitch_command_expect_failure(test_env, &["promote", "nonexistent-branch", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show branch not found error
            assert!(stderr.contains("not found") ||
                   stderr.contains("does not exist") ||
                   stderr.contains("No such branch"));

            Ok(())
        })
    }

    /// Test error when trying to promote to non-existent environment
    #[test]
    fn test_promote_to_nonexistent_environment_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a feature branch
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature1")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Try to promote to non-existent environment
            let output = run_hitch_command_expect_failure(test_env, &["promote", "feature1", "nonexistent-env"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show environment not found error
            assert!(stderr.contains("not found") ||
                   stderr.contains("does not exist") ||
                   stderr.contains("No such environment"));

            Ok(())
        })
    }

    /// Test error when trying to rebuild locked environment without --force
    #[test]
    fn test_rebuild_locked_environment_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add and lock dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            run_hitch_command(test_env, &["lock", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to rebuild locked environment without --force
            let output = run_hitch_command_expect_failure(test_env, &["rebuild", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show locked environment error
            assert!(stderr.contains("locked") ||
                   stderr.contains("cannot rebuild") ||
                   stderr.contains("use --force"));

            Ok(())
        })
    }

    /// Test error when trying to add duplicate environment
    #[test]
    fn test_add_duplicate_environment_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to add same environment again
            let output = run_hitch_command_expect_failure(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show duplicate environment error
            assert!(stderr.contains("already exists") ||
                   stderr.contains("duplicate") ||
                   stderr.contains("already added"));

            Ok(())
        })
    }

    /// Test error when trying to demote non-promoted branch
    #[test]
    fn test_demote_non_promoted_branch_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create feature branch but don't promote it
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature1")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Try to demote non-promoted branch
            let output = run_hitch_command_expect_failure(test_env, &["demote", "feature1", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show not promoted error
            assert!(stderr.contains("not promoted") ||
                   stderr.contains("not found") ||
                   stderr.contains("cannot demote"));

            Ok(())
        })
    }

    /// Test error handling with corrupted hitch metadata
    #[test]
    fn test_corrupted_metadata_error_handling() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Corrupt the hitch.json file
            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(hitch_json_path, "invalid json content")?;

            // Try to run hitch command - should handle corruption gracefully
            let output = run_hitch_command_expect_failure(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show JSON parsing error or corrupted metadata error
            assert!(stderr.contains("JSON") ||
                   stderr.contains("parse") ||
                   stderr.contains("invalid") ||
                   stderr.contains("corrupted"));

            Ok(())
        })
    }

    /// Test error when hitch is not initialized
    #[test]
    fn test_command_without_hitch_init_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Don't initialize hitch - try to run command directly

            // Try to add environment without hitch init
            let output = run_hitch_command_expect_failure(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show not initialized error
            assert!(stderr.contains("not initialized") ||
                   stderr.contains("initialize") ||
                   stderr.contains("hitch init"));

            Ok(())
        })
    }

    /// Test error handling for invalid git repository state
    #[test]
    fn test_invalid_git_repository_error() -> Result<()> {
        with_test_env(SetupLevel::Basic, |test_env| {
            // Create directory without git repository
            std::env::set_current_dir(test_env.path())?;

            // Try to run hitch init in non-git directory
            let output = run_hitch_command_expect_failure(test_env, &["init"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show git repository error
            assert!(stderr.contains("git") ||
                   stderr.contains("repository") ||
                   stderr.contains("not a git repository"));

            Ok(())
        })
    }

    /// Test graceful error handling during remote operations failure
    #[test]
    fn test_remote_operation_failure_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Break remote configuration
            Command::new("git")
                .args(["remote", "set-url", "origin", "invalid-url-that-will-fail"])
                .current_dir(test_env.path())
                .output()?;

            // Try rebuild with --replace-remote - should fail gracefully
            let output = run_hitch_command(test_env, &["rebuild", "dev", "--replace-remote"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show warning about remote failure but not crash
            assert!(stdout.contains("Failed to force push") ||
                   stdout.contains("warning") ||
                   stderr.contains("Failed to force push") ||
                   stderr.contains("remote"));

            // Should provide manual instructions
            assert!(stdout.contains("manually") ||
                   stdout.contains("git push") ||
                   stderr.contains("manually") ||
                   stderr.contains("git push"));

            Ok(())
        })
    }

    /// Test error when trying to unlock non-locked environment
    #[test]
    fn test_unlock_non_locked_environment_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment (don't lock it)
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to unlock non-locked environment
            let output = run_hitch_command_expect_failure(test_env, &["unlock", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show not locked error
            assert!(stderr.contains("not locked") ||
                   stderr.contains("already unlocked") ||
                   stderr.contains("cannot unlock"));

            Ok(())
        })
    }

    /// Test error handling for invalid branch names
    #[test]
    fn test_invalid_branch_name_error() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Try to promote with invalid branch name (contains invalid characters)
            let output = run_hitch_command_expect_failure(test_env, &["promote", "invalid@branch#name", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show invalid branch name error
            assert!(stderr.contains("invalid") ||
                   stderr.contains("branch") ||
                   stderr.contains("not found") ||
                   stderr.contains("cannot"));

            Ok(())
        })
    }
}