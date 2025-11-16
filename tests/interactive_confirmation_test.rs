use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
mod interactive_confirmation_tests {
    use super::*;

    /// Helper to ensure working tree is clean before and after hitch init
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

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Hitch command failed: hitch {} - {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(output)
    }

    /// Helper to run hitch command with specific input
    fn run_hitch_command_with_input(test_env: &TestEnv, args: &[&str], input: &str) -> Result<std::process::Output> {
        use std::io::Write;
        use std::process::Stdio;

        let binary_path = test_env.hitch_binary();
        let mut child = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write input to stdin
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input.as_bytes())?;
            stdin.flush()?;
        }

        let output = child.wait_with_output()?;
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

    /// Test interactive confirmation with 'y' response for rebuild --replace-remote
    #[test]
    fn test_rebuild_replace_remote_confirm_yes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote and confirm 'y'
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show success message (user confirmed)
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            // Should NOT show the skipped message
            assert!(!stdout.contains("Skipping remote replacement"));

            Ok(())
        })
    }

    /// Test interactive confirmation with 'yes' response for promote --replace-remote
    #[test]
    fn test_promote_replace_remote_confirm_yes_full() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Promote with --replace-remote and confirm 'yes'
            let output = run_hitch_command_with_input(test_env, &["promote", "feature1", "dev", "--replace-remote"], "yes\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show promotion success
            assert!(stdout.contains("Successfully promoted 'feature1' to environment 'dev'"));

            // Should show force push success
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test interactive confirmation with 'n' response for rebuild --replace-remote
    #[test]
    fn test_rebuild_replace_remote_decline_no() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote but decline 'n'
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "n\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show the skipped message (user declined)
            assert!(stdout.contains("Skipping remote replacement for 'dev' branch"));
            assert!(stdout.contains("To push manually, run: git push origin dev --force"));

            // Should NOT show the force push success message
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test interactive confirmation with 'N' response for demote --replace-remote
    #[test]
    fn test_demote_replace_remote_decline_uppercase() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch first
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Demote with --replace-remote but decline 'N'
            let output = run_hitch_command_with_input(test_env, &["demote", "feature1", "dev", "--replace-remote"], "N\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show the skipped message (user declined)
            assert!(stdout.contains("Skipping remote replacement for 'dev' branch"));

            // Should NOT show the force push success message
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test interactive confirmation with empty input (should decline)
    #[test]
    fn test_replace_remote_empty_input_declines() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote and provide empty input
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show the skipped message (empty input = decline)
            assert!(stdout.contains("Skipping remote replacement for 'dev' branch"));

            // Should NOT show the force push success message
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test interactive confirmation with invalid input (should decline)
    #[test]
    fn test_replace_remote_invalid_input_declines() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote and provide invalid input
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "maybe\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show the skipped message (invalid input = decline)
            assert!(stdout.contains("Skipping remote replacement for 'dev' branch"));

            // Should NOT show the force push success message
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test interactive confirmation with case-insensitive 'Y' response
    #[test]
    fn test_replace_remote_case_insensitive_yes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote and confirm 'Y'
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "Y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show success message (user confirmed with uppercase Y)
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            // Should NOT show the skipped message
            assert!(!stdout.contains("Skipping remote replacement"));

            Ok(())
        })
    }

    /// Test that confirmation prompt shows proper warning messages
    #[test]
    fn test_confirmation_prompt_shows_warnings() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote but decline to see warnings
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "n\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the warning about replacing remote branch
            assert!(stdout.contains("This will replace the remote 'dev' branch with the rebuilt version."));

            // Should show the warning about inability to undo
            assert!(stdout.contains("This action cannot be undone and will overwrite the remote branch."));

            // Should show the confirmation prompt
            assert!(stdout.contains("Do you want to proceed? [y/N]:"));

            Ok(())
        })
    }

    /// Test confirmation prompt with multiple force push operations
    #[test]
    fn test_multiple_confirmations_in_sequence() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;

            // Add dev and staging environments
            run_hitch_command(test_env, &["add", "dev"])?;
            run_hitch_command(test_env, &["add", "staging"])?;

            // Create and promote a feature branch to dev
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Promote to dev with --replace-remote and confirm
            let output1 = run_hitch_command_with_input(test_env, &["promote", "feature1", "dev", "--replace-remote"], "y\n")?;
            let stdout1 = String::from_utf8_lossy(&output1.stdout);
            assert!(stdout1.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            // Promote to staging with --replace-remote and confirm
            let output2 = run_hitch_command_with_input(test_env, &["promote", "feature1", "staging", "--replace-remote"], "y\n")?;
            let stdout2 = String::from_utf8_lossy(&output2.stdout);
            assert!(stdout2.contains("✓ Force pushed rebuilt 'staging' branch to remote"));

            // Both should show confirmation prompts
            assert!(stdout1.contains("Do you want to proceed? [y/N]:"));
            assert!(stdout2.contains("Do you want to proceed? [y/N]:"));

            Ok(())
        })
    }

    /// Test that --no-push flag bypasses confirmation prompt
    #[test]
    fn test_no_push_bypasses_confirmation() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create a feature branch with clean history (no conflicts)
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with both --replace-remote and --no-push
            let output = run_hitch_command(test_env, &["rebuild", "dev", "--replace-remote", "--no-push"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should NOT show confirmation prompt (bypassed by --no-push)
            assert!(!stdout.contains("Do you want to proceed? [y/N]:"));

            // Should show that remote operations are skipped
            assert!(stdout.contains("Skipping remote operations for 'dev' branch due to --no-push flag"));

            // Should NOT show force push operations
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test confirmation with whitespace in input
    #[test]
    fn test_confirmation_with_whitespace_input() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Clean up after adding dev environment
            ensure_clean_working_tree(test_env)?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Switch back to main to avoid conflict when promoting
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Clean up any working tree changes after branch switching
            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Test with whitespace before 'y'
            let output1 = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "  y  \n")?;
            let stdout1 = String::from_utf8_lossy(&output1.stdout);
            assert!(stdout1.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            // Test with whitespace before 'n'
            let output2 = run_hitch_command_with_input(test_env, &["rebuild", "dev", "--replace-remote"], "  n  \n")?;
            let stdout2 = String::from_utf8_lossy(&output2.stdout);
            assert!(stdout2.contains("Skipping remote replacement for 'dev' branch"));

            Ok(())
        })
    }
}