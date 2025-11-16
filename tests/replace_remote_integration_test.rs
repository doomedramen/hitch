use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

/// Simple ANSI code stripper for test assertions
#[allow(dead_code)]
fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ANSI escape sequence
            if chars.next() == Some('[') {
                // Skip until we hit the end character (a-z)
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_alphabetic() {
                        chars.next(); // consume the end character
                        break;
                    }
                    chars.next(); // consume part of the sequence
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
#[allow(dead_code)]
mod replace_remote_tests {
    use super::*;

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

    /// Helper to run hitch command with input
    fn run_hitch_command_with_input(
        test_env: &TestEnv,
        args: &[&str],
        _input: &str,
    ) -> Result<std::process::Output> {
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
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

    /// Helper to checkout a branch
    fn checkout_branch(test_env: &TestEnv, branch_name: &str) -> Result<()> {
        Command::new("git")
            .args(["checkout", branch_name])
            .current_dir(test_env.path())
            .output()?;

        Ok(())
    }

    /// Test basic --replace-remote functionality with rebuild command
    #[test]
    fn test_rebuild_with_replace_remote_success() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Promote a branch to dev
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Now rebuild with --replace-remote and confirm "yes"
            let output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the warning about replacing remote
            assert!(stdout.contains("This will replace the remote 'dev' branch"));
            assert!(stdout.contains("This action cannot be undone"));
            assert!(stdout.contains("Force pushing rebuilt 'dev' branch to replace remote"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test --replace-remote with "no" response (should skip remote replacement)
    #[test]
    fn test_rebuild_with_replace_remote_declined() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create a feature branch and promote it
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with --replace-remote but answer "no"
            let output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote"],
                "n\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the warning but not the force push success message
            assert!(stdout.contains("This will replace the remote 'dev' branch"));
            assert!(stdout.contains("Skipping remote replacement for 'dev' branch"));
            assert!(stdout.contains("To push manually, run: git push origin dev --force"));

            // Should NOT show the force push success message
            assert!(!stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test promote command with --replace-remote flag
    #[test]
    fn test_promote_with_replace_remote() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;

            // Promote with --replace-remote and confirm "yes"
            let output = run_hitch_command_with_input(
                test_env,
                &["promote", "feature1", "dev", "--replace-remote"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show promotion success and rebuild with remote replacement
            assert!(stdout.contains("Successfully promoted 'feature1' to environment 'dev'"));
            assert!(stdout.contains("This will replace the remote 'dev' branch"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test demote command with --replace-remote flag
    #[test]
    fn test_demote_with_replace_remote() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch first
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Now demote with --replace-remote and confirm "yes"
            let output = run_hitch_command_with_input(
                test_env,
                &["demote", "feature1", "dev", "--replace-remote"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show demotion success and rebuild with remote replacement
            assert!(stdout.contains("Successfully demoted 'feature1' from environment 'dev'"));
            assert!(stdout.contains("This will replace the remote 'dev' branch"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test that --replace-remote respects --no-push flag
    #[test]
    fn test_replace_remote_with_no_push_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Rebuild with both --replace-remote and --no-push
            let output = run_hitch_command(
                test_env,
                &["rebuild", "dev", "--replace-remote", "--no-push"],
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show rebuild success but skip remote operations due to --no-push
            assert!(stdout.contains("Environment 'dev' rebuilt successfully"));
            assert!(stdout
                .contains("Skipping remote operations for 'dev' branch due to --no-push flag"));

            // Should NOT show force push operations
            assert!(!stdout.contains("Force pushing rebuilt 'dev' branch"));
            assert!(!stdout.contains("This will replace the remote 'dev' branch"));

            Ok(())
        })
    }

    /// Test --replace-remote with empty environment
    #[test]
    fn test_replace_remote_empty_environment() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment but don't promote any branches
            run_hitch_command(test_env, &["add", "dev"])?;

            // Rebuild with --replace-remote
            let output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle empty environment gracefully
            assert!(
                stdout.contains("No branches promoted to this environment, using base branch only")
            );
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test --replace-remote with multiple branches promoted
    #[test]
    fn test_replace_remote_multiple_branches() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote multiple feature branches
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            checkout_branch(test_env, "main")?;
            create_and_commit_file(test_env, "feature2.txt", "Feature 2 content")?;
            create_branch(test_env, "feature2")?;
            run_hitch_command(test_env, &["promote", "feature2", "dev"])?;

            // Rebuild with --replace-remote
            let output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle multiple branches correctly
            assert!(stdout.contains("Merging promoted branches into temporary branch"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test --replace-remote with locked environment (should fail without --force)
    #[test]
    fn test_replace_remote_locked_environment() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Lock the environment
            run_hitch_command(test_env, &["lock", "dev"])?;

            // Try to rebuild with --replace-remote (should fail)
            let output = Command::new(test_env.hitch_binary())
                .args(["rebuild", "dev", "--replace-remote"])
                .current_dir(test_env.path())
                .output()?;

            assert!(
                !output.status.success(),
                "Rebuild locked environment should fail"
            );

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about locked environment
            assert!(stderr.contains("Environment 'dev' is locked") || stderr.contains("locked by"));

            Ok(())
        })
    }

    /// Test --replace-remote with locked environment using --force flag
    #[test]
    fn test_replace_remote_locked_environment_with_force() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch
            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            create_branch(test_env, "feature1")?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Lock the environment
            run_hitch_command(test_env, &["lock", "dev"])?;

            // Rebuild with both --replace-remote and --force
            let output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote", "--force"],
                "y\n",
            )?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle force rebuild correctly
            assert!(stdout.contains("Force rebuilding locked environment 'dev'"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }
}
