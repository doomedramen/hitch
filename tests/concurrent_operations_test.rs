use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
#[allow(dead_code)]
#[allow(clippy::needless_borrow)]
#[allow(clippy::len_zero)]
#[allow(clippy::useless_vec)]
#[allow(clippy::needless_borrows_for_generic_args)]
mod concurrent_operations_tests {
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

    /// Test multiple rapid environment additions
    #[test]
    fn test_rapid_multiple_environment_additions() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add multiple environments rapidly
            let environments = vec!["dev", "staging", "prod", "qa", "testing"];
            let mut successful_envs = vec![];

            for env_name in environments {
                let output = run_hitch_command(test_env, &["add", env_name])?;

                if output.status.success() {
                    successful_envs.push(env_name);
                    ensure_clean_working_tree(test_env)?;
                }
            }

            // Should have successfully added most environments
            assert!(
                successful_envs.len() >= 4,
                "Expected at least 4 environments to be added successfully"
            );

            // Verify all successful environments exist
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_stdout = String::from_utf8_lossy(&status_output.stdout);

            for env in &successful_envs {
                assert!(
                    status_stdout.contains(env),
                    "Environment {} should exist in status",
                    env
                );
            }

            Ok(())
        })
    }

    /// Test rapid promote/demote operations on same environment
    #[test]
    fn test_rapid_promote_demote_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add dev environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create multiple feature branches
            let feature_branches = vec!["feature1", "feature2", "feature3"];
            let mut successful_branches = vec![];

            for (i, branch_name) in feature_branches.iter().enumerate() {
                create_and_commit_file(
                    test_env,
                    &format!("feature{}.txt", i),
                    &format!("Feature {} content", i),
                )?;

                Command::new("git")
                    .args(["checkout", "-b", branch_name])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["checkout", "main"])
                    .current_dir(test_env.path())
                    .output()?;

                ensure_clean_working_tree(test_env)?;

                // Promote feature
                let promote_output = run_hitch_command(test_env, &["promote", branch_name, "dev"])?;

                if promote_output.status.success() {
                    successful_branches.push(branch_name);
                    ensure_clean_working_tree(test_env)?;

                    // Immediately try to demote (test rapid operations)
                    if i > 0 {
                        let demote_output = run_hitch_command(
                            test_env,
                            &["demote", &successful_branches[i - 1], "dev"],
                        )?;
                        if demote_output.status.success() {
                            ensure_clean_working_tree(test_env)?;
                        }
                    }
                }
            }

            // Should have successfully promoted at least some branches
            assert!(
                successful_branches.len() >= 1,
                "Expected at least 1 promotion to succeed"
            );

            // Verify final state
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Status should work after rapid promote/demote operations"
            );

            Ok(())
        })
    }

    /// Test rapid rebuild operations
    #[test]
    fn test_rapid_rebuild_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment and promote features
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            create_and_commit_file(test_env, "feature1.txt", "Feature 1 content")?;
            Command::new("git")
                .args(["checkout", "-b", "feature1"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            run_hitch_command(test_env, &["promote", "feature1", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Perform multiple rapid rebuilds
            let mut successful_rebuilds = 0;
            for i in 0..5 {
                let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;

                if rebuild_output.status.success() {
                    successful_rebuilds += 1;
                    ensure_clean_working_tree(test_env)?;

                    // Add a small change between rebuilds
                    if i < 3 {
                        create_and_commit_file(
                            test_env,
                            &format!("change{}.txt", i),
                            &format!("Change {}", i),
                        )?;
                        ensure_clean_working_tree(test_env)?;
                    }
                }
            }

            // Should have at least some successful rebuilds
            assert!(
                successful_rebuilds >= 1,
                "Expected at least 1 rebuild to succeed"
            );

            // Environment should still be in valid state
            let final_status = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status.status.success(),
                "Status command should work after rapid rebuilds"
            );

            Ok(())
        })
    }

    /// Test rapid lock/unlock operations
    #[test]
    fn test_rapid_lock_unlock_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Perform rapid lock/unlock cycles
            for i in 0..3 {
                // Lock environment
                let lock_output = run_hitch_command(test_env, &["lock", "dev"])?;
                assert!(lock_output.status.success(), "Lock {} should succeed", i);
                ensure_clean_working_tree(test_env)?;

                // Try normal rebuild (should fail)
                let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;
                assert!(
                    !rebuild_output.status.success(),
                    "Normal rebuild should fail on locked environment"
                );

                // Force rebuild (should succeed)
                let force_rebuild_output =
                    run_hitch_command(test_env, &["rebuild", "dev", "--force"])?;
                assert!(
                    force_rebuild_output.status.success(),
                    "Force rebuild should succeed on locked environment"
                );
                ensure_clean_working_tree(test_env)?;

                // Unlock environment
                let unlock_output = run_hitch_command(test_env, &["unlock", "dev"])?;
                assert!(
                    unlock_output.status.success(),
                    "Unlock {} should succeed",
                    i
                );
                ensure_clean_working_tree(test_env)?;

                // Normal rebuild should work now
                let normal_rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;
                assert!(
                    normal_rebuild_output.status.success(),
                    "Normal rebuild should work after unlock"
                );
                ensure_clean_working_tree(test_env)?;
            }

            // Final state should be consistent
            let final_status = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status.status.success(),
                "Status should work after rapid lock/unlock operations"
            );

            Ok(())
        })
    }

    /// Test rapid status checks
    #[test]
    fn test_rapid_status_checks() -> Result<()> {
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

            // Perform rapid status checks
            let mut successful_checks = 0;
            for _i in 0..10 {
                let status_output = run_hitch_command(test_env, &["status"])?;

                if status_output.status.success() {
                    successful_checks += 1;

                    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
                    // Should always show the environments
                    assert!(
                        status_stdout.contains("dev"),
                        "Status should always show dev environment"
                    );
                    assert!(
                        status_stdout.contains("staging"),
                        "Status should always show staging environment"
                    );
                    assert!(
                        status_stdout.contains("prod"),
                        "Status should always show prod environment"
                    );
                }
            }

            // All status checks should succeed
            assert_eq!(successful_checks, 10, "All status checks should succeed");

            Ok(())
        })
    }

    /// Test rapid mixed operations
    #[test]
    fn test_rapid_mixed_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Perform rapid mixed operations
            let operations = vec![
                ("add", vec!["dev"]),
                ("status", vec![]),
                ("add", vec!["staging"]),
                ("status", vec![]),
                ("add", vec!["prod"]),
                ("status", vec![]),
                ("lock", vec!["dev"]),
                ("status", vec![]),
                ("unlock", vec!["dev"]),
                ("status", vec![]),
            ];

            let mut successful_operations = 0;

            for (op, args) in operations {
                let mut full_args = vec![op];
                full_args.extend(args);

                let output = run_hitch_command(test_env, &full_args)?;

                if output.status.success() {
                    successful_operations += 1;
                }

                ensure_clean_working_tree(test_env)?;
            }

            // Most operations should succeed
            assert!(
                successful_operations >= 8,
                "Expected at least 8 operations to succeed"
            );

            // Final state should be consistent
            let final_status = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status.status.success(),
                "Final status should be consistent"
            );

            let status_stdout = String::from_utf8_lossy(&final_status.stdout);
            assert!(status_stdout.contains("dev"), "Should have dev environment");
            assert!(
                status_stdout.contains("staging"),
                "Should have staging environment"
            );
            assert!(
                status_stdout.contains("prod"),
                "Should have prod environment"
            );

            Ok(())
        })
    }

    /// Test environment creation and promotion race condition simulation
    #[test]
    fn test_environment_creation_promotion_race() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create feature branch
            create_and_commit_file(test_env, "feature.txt", "feature content")?;
            Command::new("git")
                .args(["checkout", "-b", "feature"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Simulate race condition: add environment and immediately promote
            let add_output = run_hitch_command(test_env, &["add", "dev"])?;
            let add_success = add_output.status.success();

            // Immediately try to promote (simulates concurrent access)
            let promote_output = run_hitch_command(test_env, &["promote", "feature", "dev"])?;
            let promote_success = promote_output.status.success();

            // At least one operation should succeed
            assert!(
                add_success || promote_success,
                "At least one operation should succeed"
            );

            // Clean up for final verification
            ensure_clean_working_tree(test_env)?;

            // Final state should be consistent
            let final_status = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status.status.success(),
                "Final status should be consistent"
            );

            Ok(())
        })
    }

    /// Test metadata consistency under rapid operations
    #[test]
    fn test_metadata_consistency_under_rapid_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Perform rapid operations that modify metadata
            for i in 0..5 {
                // Add environment
                let env_name = format!("env-{}", i);
                let add_output = run_hitch_command(test_env, &["add", &env_name])?;

                if add_output.status.success() {
                    ensure_clean_working_tree(test_env)?;

                    // Create and promote a feature to this environment
                    create_and_commit_file(
                        test_env,
                        &format!("feature-{}.txt", i),
                        &format!("Feature {} content", i),
                    )?;

                    let branch_name = format!("feature-{}", i);
                    Command::new("git")
                        .args(["checkout", "-b", &branch_name])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["checkout", "main"])
                        .current_dir(test_env.path())
                        .output()?;

                    ensure_clean_working_tree(test_env)?;

                    let promote_output =
                        run_hitch_command(test_env, &["promote", &branch_name, &env_name])?;

                    if promote_output.status.success() {
                        ensure_clean_working_tree(test_env)?;
                    }
                }
            }

            // Verify metadata consistency
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Status should work after rapid metadata operations"
            );

            let status_stdout = String::from_utf8_lossy(&status_output.stdout);

            // Check that hitch-metadata branch is intact
            let branch_output = Command::new("git")
                .args(["branch", "-a"])
                .current_dir(test_env.path())
                .output()?;

            let branch_stdout = String::from_utf8_lossy(&branch_output.stdout);
            assert!(
                branch_stdout.contains("hitch-metadata"),
                "hitch-metadata branch should exist after rapid operations"
            );

            Ok(())
        })
    }
}
