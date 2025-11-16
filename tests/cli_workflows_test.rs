use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
mod cli_workflow_tests {
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

    /// Test complete hitch init workflow
    #[test]
    fn test_complete_hitch_init_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean
            ensure_clean_working_tree(test_env)?;

            // Initialize Hitch
            let output = run_hitch_command(test_env, &["init"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show initialization success messages
            assert!(stdout.contains("Initializing Hitch") || stdout.contains("✅"));
            assert!(stdout.contains("hitch-metadata") || stdout.contains("metadata"));
            assert!(stdout.contains("successfully") || stdout.contains("✅"));

            // Verify hitch.json was created
            assert!(test_env.path().join("hitch.json").exists());

            // Verify hitch-metadata branch exists
            let branch_output = Command::new("git")
                .args(["branch", "-a"])
                .current_dir(test_env.path())
                .output()?;
            let branch_stdout = String::from_utf8_lossy(&branch_output.stdout);
            assert!(branch_stdout.contains("hitch-metadata"));

            Ok(())
        })
    }

    /// Test add environment workflow
    #[test]
    fn test_add_environment_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show environment addition success
            assert!(stdout.contains("Adding environment") || stdout.contains("dev"));
            assert!(stdout.contains("successfully") || stdout.contains("✅"));

            // Verify environment branch was created
            let branch_output = Command::new("git")
                .args(["branch", "-a"])
                .current_dir(test_env.path())
                .output()?;
            let branch_stdout = String::from_utf8_lossy(&branch_output.stdout);
            assert!(branch_stdout.contains("dev"));

            Ok(())
        })
    }

    /// Test status command workflow
    #[test]
    fn test_status_command_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add multiple environments
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "staging"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "prod"])?;
            ensure_clean_working_tree(test_env)?;

            // Check status
            let output = run_hitch_command(test_env, &["status"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show all environments
            assert!(stdout.contains("dev"));
            assert!(stdout.contains("staging"));
            assert!(stdout.contains("prod"));

            // Should show environment status information
            assert!(stdout.contains("Environment") || stdout.contains("Status"));

            Ok(())
        })
    }

    /// Test complete promote workflow
    #[test]
    fn test_complete_promote_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create feature branch with changes
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature-branch")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Promote feature to dev
            let output = run_hitch_command(test_env, &["promote", "feature-branch", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show promotion success
            assert!(stdout.contains("Successfully promoted") || stdout.contains("✅"));
            assert!(stdout.contains("feature-branch"));
            assert!(stdout.contains("dev"));

            // Check status to verify promotion
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_stdout = String::from_utf8_lossy(&status_output.stdout);
            assert!(status_stdout.contains("feature-branch"));

            Ok(())
        })
    }

    /// Test complete rebuild workflow
    #[test]
    fn test_complete_rebuild_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create and promote feature branch
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature-branch")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature-branch", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Rebuild dev environment
            let output = run_hitch_command(test_env, &["rebuild", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show rebuild success
            assert!(stdout.contains("rebuilt") || stdout.contains("✅"));
            assert!(stdout.contains("dev"));
            assert!(stdout.contains("successfully") || stdout.contains("✅"));

            Ok(())
        })
    }

    /// Test complete demote workflow
    #[test]
    fn test_complete_demote_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create and promote feature branch
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature-branch")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature-branch", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Demote feature from dev
            let output = run_hitch_command(test_env, &["demote", "feature-branch", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show demotion success
            assert!(stdout.contains("Successfully demoted") || stdout.contains("✅"));
            assert!(stdout.contains("feature-branch"));
            assert!(stdout.contains("dev"));

            Ok(())
        })
    }

    /// Test lock and unlock workflow
    #[test]
    fn test_lock_unlock_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Lock environment
            let lock_output = run_hitch_command(test_env, &["lock", "dev"])?;
            let lock_stdout = String::from_utf8_lossy(&lock_output.stdout);
            assert!(lock_stdout.contains("locked") || lock_stdout.contains("✅"));

            // Try to rebuild locked environment (should fail)
            let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"]);
            assert!(!rebuild_output?.status.success());

            // Unlock environment
            let unlock_output = run_hitch_command(test_env, &["unlock", "dev"])?;
            let unlock_stdout = String::from_utf8_lossy(&unlock_output.stdout);
            assert!(unlock_stdout.contains("unlocked") || unlock_stdout.contains("✅"));

            // Should be able to rebuild now
            let rebuild_output2 = run_hitch_command(test_env, &["rebuild", "dev"])?;
            assert!(rebuild_output2.status.success());

            Ok(())
        })
    }

    /// Test multi-environment workflow
    #[test]
    fn test_multi_environment_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add multiple environments
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "staging"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "prod"])?;
            ensure_clean_working_tree(test_env)?;

            // Create feature branch
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            create_branch(test_env, "feature-branch")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Promote through environments
            run_hitch_command(test_env, &["promote", "feature-branch", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature-branch", "staging"])?;
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature-branch", "prod"])?;
            ensure_clean_working_tree(test_env)?;

            // Check final status
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_stdout = String::from_utf8_lossy(&status_output.stdout);

            // Should show feature in all environments
            assert!(status_stdout.contains("feature-branch"));

            Ok(())
        })
    }

    /// Test workflow with --no-push flag
    #[test]
    fn test_workflow_with_no_push_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment with --no-push
            let output = run_hitch_command(test_env, &["add", "dev", "--no-push"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should mention skipping remote operations
            assert!(stdout.contains("no-push") || stdout.contains("Skipping remote"));

            // Rebuild with --no-push
            let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev", "--no-push"])?;
            let rebuild_stdout = String::from_utf8_lossy(&rebuild_output.stdout);
            assert!(rebuild_stdout.contains("no-push") || rebuild_stdout.contains("Skipping remote"));

            Ok(())
        })
    }

    /// Test workflow with --force flag
    #[test]
    fn test_workflow_with_force_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Lock environment
            run_hitch_command(test_env, &["lock", "dev"])?;

            // Force rebuild locked environment
            let output = run_hitch_command(test_env, &["rebuild", "dev", "--force"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should mention force operation
            assert!(stdout.contains("force") || stdout.contains("Force"));

            Ok(())
        })
    }

    /// Test realistic development workflow
    #[test]
    fn test_realistic_development_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Setup development environments
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "staging"])?;
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["add", "prod"])?;
            ensure_clean_working_tree(test_env)?;

            // Developer creates feature branch
            create_and_commit_file(test_env, "user-auth.txt", "User authentication feature")?;
            create_branch(test_env, "feature/user-auth")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Promote to dev for testing
            let promote_output = run_hitch_command(test_env, &["promote", "feature/user-auth", "dev"])?;
            let promote_stdout = String::from_utf8_lossy(&promote_output.stdout);
            assert!(promote_stdout.contains("dev"));

            ensure_clean_working_tree(test_env)?;

            // Add more changes to the feature
            Command::new("git")
                .args(["checkout", "feature/user-auth"])
                .current_dir(test_env.path())
                .output()?;
            create_and_commit_file(test_env, "auth-tests.txt", "Authentication tests")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            // Rebuild dev with latest changes
            run_hitch_command(test_env, &["rebuild", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Promote to staging for QA
            run_hitch_command(test_env, &["promote", "feature/user-auth", "staging"])?;
            ensure_clean_working_tree(test_env)?;

            // Promote to production for release
            run_hitch_command(test_env, &["promote", "feature/user-auth", "prod"])?;
            ensure_clean_working_tree(test_env)?;

            // Final status check
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_stdout = String::from_utf8_lossy(&status_output.stdout);

            // Should show feature in all environments
            assert!(status_stdout.contains("feature/user-auth"));
            assert!(status_stdout.contains("dev"));
            assert!(status_stdout.contains("staging"));
            assert!(status_stdout.contains("prod"));

            Ok(())
        })
    }

    /// Test cleanup workflow
    #[test]
    fn test_cleanup_workflow() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add and work with environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            create_and_commit_file(test_env, "temp.txt", "temporary feature")?;
            create_branch(test_env, "temp-feature")?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "temp-feature", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Demote feature to clean up
            run_hitch_command(test_env, &["demote", "temp-feature", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Verify cleanup
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_stdout = String::from_utf8_lossy(&status_output.stdout);

            // Feature should no longer be in environment
            assert!(!status_stdout.contains("temp-feature"));

            Ok(())
        })
    }
}