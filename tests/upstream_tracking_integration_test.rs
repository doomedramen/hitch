use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
mod upstream_tracking_tests {
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

    /// Helper to run git command
    fn run_git_command(test_env: &TestEnv, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new("git")
            .args(args)
            .current_dir(test_env.path())
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Git command failed: git {} - {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(output)
    }

    /// Helper to check if branch has upstream tracking configured
    fn has_upstream_tracking(test_env: &TestEnv, branch_name: &str) -> Result<bool> {
        let output = run_git_command(test_env, &["branch", "-vv", branch_name])?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check if the branch line contains upstream tracking info
        // Format should be something like: "* dev 1234567 [origin/dev] ..."
        Ok(stdout.contains(&format!("[origin/{}]", branch_name)))
    }

    /// Helper to get upstream tracking info
    fn get_upstream_tracking(test_env: &TestEnv, branch_name: &str) -> Result<String> {
        let output = run_git_command(test_env, &["branch", "-vv", branch_name])?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Extract the upstream tracking part
        for line in stdout.lines() {
            if line.contains(branch_name) {
                if let Some(start) = line.find('[') {
                    if let Some(end) = line.find(']') {
                        return Ok(line[start..end + 1].to_string());
                    }
                }
            }
        }

        Ok("No upstream tracking".to_string())
    }

    /// Test that force push operations set up upstream tracking correctly
    #[test]
    fn test_force_push_sets_upstream_tracking() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Verify that the dev branch doesn't have upstream tracking initially
            assert!(
                !has_upstream_tracking(test_env, "dev")?,
                "Dev branch should not have upstream tracking initially"
            );

            // Now rebuild with to trigger force push with --set-upstream
            let _output = run_hitch_command_with_input(
                test_env,
                &["rebuild", "dev", "--replace-remote"],
                "y\n",
            )?;

            // Verify that upstream tracking is now set up
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after --replace-remote"
            );

            let tracking_info = get_upstream_tracking(test_env, "dev")?;
            assert!(
                tracking_info.contains("[origin/dev]"),
                "Upstream tracking should point to origin/dev"
            );

            Ok(())
        })
    }

    /// Test that promote with --replace-remote sets up upstream tracking
    #[test]
    fn test_promote_remote_replacement_sets_upstream_tracking() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Verify dev branch has no upstream tracking initially
            assert!(
                !has_upstream_tracking(test_env, "dev")?,
                "Dev branch should not have upstream tracking initially"
            );

            // Create a feature branch
            run_git_command(test_env, &["checkout", "-b", "feature1"])?;
            run_git_command(test_env, &["touch", "test.txt"])?;
            run_git_command(test_env, &["add", "test.txt"])?;
            run_git_command(test_env, &["commit", "-m", "Add test file"])?;

            // Promote
            let _output =
                run_hitch_command_with_input(test_env, &["promote", "feature1", "dev"], "y\n")?;

            // Verify that upstream tracking is set up for dev
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after promote"
            );

            Ok(())
        })
    }

    /// Test that demote with --replace-remote sets up upstream tracking
    #[test]
    fn test_demote_remote_replacement_sets_upstream_tracking() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Create and promote a feature branch first
            run_git_command(test_env, &["checkout", "-b", "feature1"])?;
            run_git_command(test_env, &["touch", "test.txt"])?;
            run_git_command(test_env, &["add", "test.txt"])?;
            run_git_command(test_env, &["commit", "-m", "Add test file"])?;
            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;

            // Verify dev branch has upstream tracking from initial promote
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after initial promote"
            );

            // Now demote
            let _output =
                run_hitch_command_with_input(test_env, &["demote", "feature1", "dev"], "y\n")?;

            // Verify that upstream tracking is still set up after demote
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should still have upstream tracking after demote"
            );

            Ok(())
        })
    }

    /// Test that upstream tracking is preserved when no remote replacement occurs
    #[test]
    fn test_upstream_tracking_preservation_without_remote_replacement() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Manually set up upstream tracking for dev branch
            run_git_command(test_env, &["checkout", "-b", "dev"])?;
            run_git_command(test_env, &["push", "origin", "dev", "--set-upstream"])?;
            run_git_command(test_env, &["checkout", "main"])?;

            // Verify upstream tracking exists
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after manual setup"
            );

            // Rebuild without --replace-remote (should preserve existing tracking)
            run_hitch_command(test_env, &["rebuild", "dev"])?;

            // Verify that upstream tracking is still there
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should still have upstream tracking after rebuild"
            );

            Ok(())
        })
    }

    /// Test that force push uses --set-upstream flag by checking git command behavior
    #[test]
    fn test_force_push_uses_set_upstream_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Manually create a dev branch without upstream tracking
            run_git_command(test_env, &["checkout", "-b", "dev"])?;
            run_git_command(test_env, &["checkout", "main"])?;

            // Verify no upstream tracking exists
            assert!(
                !has_upstream_tracking(test_env, "dev")?,
                "Dev branch should not have upstream tracking before rebuild"
            );

            // Rebuild
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            // The rebuild should succeed and set up upstream tracking
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("✓ Force pushed rebuilt 'dev' branch to remote"),
                "Should show successful force push message"
            );

            // Verify upstream tracking is now set up
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after force push with --set-upstream"
            );

            Ok(())
        })
    }

    /// Test multiple environments each get upstream tracking
    #[test]
    fn test_multiple_environments_upstream_tracking() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add multiple environments
            run_hitch_command(test_env, &["add", "dev"])?;
            run_hitch_command(test_env, &["add", "staging"])?;
            run_hitch_command(test_env, &["add", "prod"])?;

            // Verify no upstream tracking exists for any of them
            assert!(
                !has_upstream_tracking(test_env, "dev")?,
                "Dev should not have upstream tracking"
            );
            assert!(
                !has_upstream_tracking(test_env, "staging")?,
                "Staging should not have upstream tracking"
            );
            assert!(
                !has_upstream_tracking(test_env, "prod")?,
                "Prod should not have upstream tracking"
            );

            // Rebuild each environment
            run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;
            run_hitch_command_with_input(test_env, &["rebuild", "staging"], "y\n")?;
            run_hitch_command_with_input(test_env, &["rebuild", "prod"], "y\n")?;

            // Verify all environments now have upstream tracking
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev should have upstream tracking"
            );
            assert!(
                has_upstream_tracking(test_env, "staging")?,
                "Staging should have upstream tracking"
            );
            assert!(
                has_upstream_tracking(test_env, "prod")?,
                "Prod should have upstream tracking"
            );

            Ok(())
        })
    }

    /// Test that upstream tracking works correctly after multiple rebuilds
    #[test]
    fn test_upstream_tracking_persistence_across_rebuilds() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // First rebuild to set up tracking
            run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            // Verify upstream tracking is set up
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after first rebuild"
            );

            // Make a change and commit it
            run_git_command(test_env, &["checkout", "dev"])?;
            run_git_command(test_env, &["touch", "another-file.txt"])?;
            run_git_command(test_env, &["add", "another-file.txt"])?;
            run_git_command(test_env, &["commit", "-m", "Add another file"])?;

            // Second rebuild
            run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            // Verify upstream tracking is still there
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should still have upstream tracking after second rebuild"
            );

            // Third rebuild
            run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            // Verify upstream tracking persists
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should still have upstream tracking after third rebuild"
            );

            Ok(())
        })
    }

    /// Test that --no-push flag doesn't interfere with existing upstream tracking
    #[test]
    fn test_no_push_preserves_existing_upstream_tracking() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Manually set up upstream tracking
            run_git_command(test_env, &["checkout", "-b", "dev"])?;
            run_git_command(test_env, &["push", "origin", "dev", "--set-upstream"])?;
            run_git_command(test_env, &["checkout", "main"])?;

            // Verify upstream tracking exists
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should have upstream tracking after manual setup"
            );

            // Rebuild with --no-push (should preserve existing tracking)
            run_hitch_command(test_env, &["rebuild", "dev", "--no-push"])?;

            // Verify upstream tracking is still there
            assert!(
                has_upstream_tracking(test_env, "dev")?,
                "Dev branch should still have upstream tracking after rebuild with --no-push"
            );

            Ok(())
        })
    }

    /// Test that force push failure doesn't corrupt upstream tracking
    #[test]
    fn test_force_push_failure_handling() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Initialize Hitch
            run_hitch_command(test_env, &["init"])?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;

            // Break the remote URL to simulate push failure
            run_git_command(
                test_env,
                &["remote", "set-url", "origin", "invalid-url-that-will-fail"],
            )?;

            // Try to rebuild (should fail gracefully)
            let output = run_hitch_command_with_input(test_env, &["rebuild", "dev"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should show warning about force push failure
            assert!(
                stdout.contains("Failed to force push")
                    || stdout.contains("You may need to manually run"),
                "Should show warning about force push failure"
            );

            // Restore a valid remote URL
            run_git_command(
                test_env,
                &[
                    "remote",
                    "set-url",
                    "origin",
                    test_env.path().to_str().unwrap(),
                ],
            )?;

            Ok(())
        })
    }
}
