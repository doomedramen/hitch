use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

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
mod remote_replacement_tests {
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

    /// Helper function to create test files consistently
    fn create_test_file(test_env: &TestEnv, filename: &str, content: &str) -> Result<()> {
        use std::fs;
        let file_path = test_env.path().join(filename);
        fs::write(file_path, content)?;
        Ok(())
    }

    /// Helper function to create and commit a feature branch following the working pattern
    fn create_feature_branch(
        test_env: &TestEnv,
        branch_name: &str,
        filename: &str,
        content: &str,
    ) -> Result<()> {
        // Create and checkout feature branch FIRST, then create files ON the branch
        let checkout_output = Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(test_env.path())
            .output()?;
        if !checkout_output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create branch '{}': {}",
                branch_name,
                String::from_utf8_lossy(&checkout_output.stderr)
            ));
        }

        create_test_file(test_env, filename, content)?;

        let add_output = Command::new("git")
            .args(["add", "-f", filename])
            .current_dir(test_env.path())
            .output()?;
        if !add_output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to add file '{}': {}",
                filename,
                String::from_utf8_lossy(&add_output.stderr)
            ));
        }

        let commit_output = Command::new("git")
            .args(["commit", "-m", &format!("Add {}", filename)])
            .current_dir(test_env.path())
            .output()?;
        if !commit_output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to commit: {}",
                String::from_utf8_lossy(&commit_output.stderr)
            ));
        }

        let main_checkout_output = Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;
        if !main_checkout_output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to checkout main: {}",
                String::from_utf8_lossy(&main_checkout_output.stderr)
            ));
        }

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

    /// Test basic remote replacement functionality with rebuild command
    #[test]
    fn test_rebuild_with_remote_replacement_success() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Use the exact working pattern from cli_workflows_test.rs
            create_test_file(
                test_env,
                "rebuild-success-feature.txt",
                "Rebuild success feature",
            )?;
            Command::new("git")
                .args(["add", "-f", "rebuild-success-feature.txt"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["commit", "-m", "Add rebuild-success-feature.txt"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["checkout", "-b", "rebuild-success-remote-replacement-test"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &["promote", "rebuild-success-remote-replacement-test", "dev"],
            )?;

            // Now rebuild and confirm "yes" (should always prompt)
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show the warning about replacing remote
            assert!(stdout.contains("This will replace the remote 'dev' branch"));
            assert!(stdout.contains("This action cannot be undone"));
            assert!(stdout.contains("Force pushing rebuilt 'dev' branch to replace remote"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test remote replacement with "no" response (should skip remote replacement)
    #[test]
    fn test_rebuild_with_remote_replacement_declined() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create a commit on main, then create and checkout feature branch
            create_test_file(
                test_env,
                "remote-replacement-declined-feature.txt",
                "Feature for declined test",
            )?;
            Command::new("git")
                .args(["checkout", "-b", "remote-replacement-declined-test"])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &["promote", "remote-replacement-declined-test", "dev"],
            )?;

            // Rebuild and answer "no" (should always prompt)
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "n\n")?;

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

    /// Test promote command with remote replacement
    #[test]
    fn test_promote_with_remote_replacement() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create a commit on main, then create and checkout feature branch
            create_test_file(
                test_env,
                "promote-remote-replacement-feature.txt",
                "Feature for promote test",
            )?;
            Command::new("git")
                .args(["checkout", "-b", "promote-remote-replacement-test"])
                .current_dir(test_env.path())
                .output()?;

            // Promote and confirm "yes" (should always prompt)
            let output = run_hitch_command_with_input(
                test_env,
                &["promote", "promote-remote-replacement-test", "dev"],
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

    /// Test demote command with remote replacement
    #[test]
    fn test_demote_with_remote_replacement() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch first
            create_test_file(
                test_env,
                "demote-remote-replacement-feature.txt",
                "Feature for demote test",
            )?;
            Command::new("git")
                .args(["checkout", "-b", "demote-remote-replacement-test"])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &["promote", "demote-remote-replacement-test", "dev"],
            )?;

            // Now demote and confirm "yes" (should always prompt)
            let output = run_hitch_command_with_input(
                test_env,
                &["demote", "demote-remote-replacement-test", "dev"],
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

    /// Test that remote replacement respects --no-push flag
    #[test]
    fn test_remote_replacement_with_no_push_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch
            create_test_file(
                test_env,
                "no-push-remote-replacement-feature.txt",
                "Feature for no-push test",
            )?;
            Command::new("git")
                .args(["checkout", "-b", "no-push-remote-replacement-test"])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &["promote", "no-push-remote-replacement-test", "dev"],
            )?;

            // Rebuild with --no-push (should skip all remote operations)
            let output = run_hitch_command(test_env, &["rebuild", "dev", "--no-push"])?;

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

    /// Test remote replacement with empty environment
    #[test]
    fn test_remote_replacement_empty_environment() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment but don't promote any branches
            run_hitch_command(test_env, &["add", "dev"])?;

            // Rebuild and confirm "yes" (should always prompt)
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle empty environment gracefully
            assert!(
                stdout.contains("No branches promoted to this environment, using base branch only")
            );
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test remote replacement with multiple branches promoted
    #[test]
    fn test_remote_replacement_multiple_branches() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote multiple feature branches
            create_test_file(
                test_env,
                "multiple-branches-remote-replacement-feature1.txt",
                "Feature 1 for multiple branches test",
            )?;
            Command::new("git")
                .args([
                    "checkout",
                    "-b",
                    "multiple-branches-remote-replacement-test1",
                ])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &[
                    "promote",
                    "multiple-branches-remote-replacement-test1",
                    "dev",
                ],
            )?;

            checkout_branch(test_env, "main")?;
            create_test_file(
                test_env,
                "multiple-branches-remote-replacement-feature2.txt",
                "Feature 2 for multiple branches test",
            )?;
            Command::new("git")
                .args([
                    "checkout",
                    "-b",
                    "multiple-branches-remote-replacement-test2",
                ])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &[
                    "promote",
                    "multiple-branches-remote-replacement-test2",
                    "dev",
                ],
            )?;

            // Rebuild and confirm "yes" (should always prompt)
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle multiple branches correctly
            assert!(stdout.contains("Merging promoted branches into temporary branch"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }

    /// Test remote replacement with locked environment (should fail without --force)
    #[test]
    fn test_remote_replacement_locked_environment() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Lock the environment
            run_hitch_command(test_env, &["lock", "dev"])?;

            // Try to rebuild (should fail due to lock)
            let output = Command::new(test_env.hitch_binary())
                .args(["rebuild", "dev"])
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

    /// Test remote replacement with locked environment using --force flag
    #[test]
    fn test_remote_replacement_locked_environment_with_force() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch
            create_test_file(
                test_env,
                "force-remote-replacement-feature.txt",
                "Feature for force test",
            )?;
            Command::new("git")
                .args(["checkout", "-b", "force-remote-replacement-test"])
                .current_dir(test_env.path())
                .output()?;
            run_hitch_command(
                test_env,
                &["promote", "force-remote-replacement-test", "dev"],
            )?;

            // Lock the environment
            run_hitch_command(test_env, &["lock", "dev"])?;

            // Rebuild with --force and confirm "yes" (should always prompt)
            let output =
                run_hitch_command_with_input(test_env, &["rebuild", "dev", "--force"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should handle force rebuild correctly
            assert!(stdout.contains("Force rebuilding locked environment 'dev'"));
            assert!(stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"));

            Ok(())
        })
    }
}
