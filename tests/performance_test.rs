use anyhow::Result;
use std::process::Command;
use std::time::{Duration, Instant};

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
#[allow(dead_code)]
#[allow(clippy::needless_borrows_for_generic_args)]
mod performance_tests {
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

    /// Test performance with many environments
    #[test]
    #[ignore]
        fn test_performance_with_many_environments() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            let start_time = Instant::now();

            // Create many environments
            let environment_count = 20;
            let mut successful_envs = 0;

            for i in 0..environment_count {
                let env_name = format!("env-{:02}", i);
                let add_start = Instant::now();

                let output = run_hitch_command(test_env, &["add", &env_name])?;

                let add_duration = add_start.elapsed();

                if output.status.success() {
                    successful_envs += 1;
                    ensure_clean_working_tree(test_env)?;

                    // Log performance for each environment addition
                    println!("Added environment {} in {:?}", env_name, add_duration);
                } else {
                    println!("Failed to add environment {}: {:?}", env_name, add_duration);
                }
            }

            let total_time = start_time.elapsed();

            println!(
                "Successfully added {}/{} environments in {:?}",
                successful_envs, environment_count, total_time
            );
            println!(
                "Average time per environment: {:?}",
                total_time / environment_count as u32
            );

            // Performance assertions
            assert!(
                successful_envs >= 15,
                "Should successfully add at least 15 environments"
            );
            assert!(
                total_time < Duration::from_secs(30),
                "Should complete within 30 seconds"
            );

            // Test status performance with many environments
            let status_start = Instant::now();
            let status_output = run_hitch_command(test_env, &["status"])?;
            let status_duration = status_start.elapsed();

            assert!(
                status_output.status.success(),
                "Status should work with many environments"
            );
            println!(
                "Status command with {} environments took {:?}",
                successful_envs, status_duration
            );
            assert!(
                status_duration < Duration::from_secs(5),
                "Status should complete within 5 seconds"
            );

            let status_stdout = String::from_utf8_lossy(&status_output.stdout);
            assert!(
                status_stdout.contains("env-"),
                "Status should show environment names"
            );

            Ok(())
        })
    }

    /// Test performance with large repository (many files and commits)
    #[test]
    #[ignore]
        fn test_performance_with_large_repository() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            let start_time = Instant::now();

            // Create a large repository with many files and commits
            let file_count = 50;
            let commit_count = 20;

            for commit in 0..commit_count {
                // Add multiple files per commit
                for file in 0..(file_count / commit_count + 1) {
                    let filename = format!("file_{}_{}.txt", commit, file);
                    let content = format!("Content for file {} in commit {}\n", file, commit);

                    std::fs::write(test_env.path().join(&filename), content)?;

                    Command::new("git")
                        .args(["add", &filename])
                        .current_dir(test_env.path())
                        .output()?;
                }

                Command::new("git")
                    .args(["commit", "-m", &format!("Commit {}", commit)])
                    .current_dir(test_env.path())
                    .output()?;
            }

            let repo_creation_time = start_time.elapsed();
            println!(
                "Created large repository ({} files, {} commits) in {:?}",
                file_count, commit_count, repo_creation_time
            );

            // Test Hitch performance with large repository
            let hitch_start = Instant::now();

            // Add environment
            let add_start = Instant::now();
            run_hitch_command(test_env, &["add", "dev"])?;
            let add_duration = add_start.elapsed();
            println!(
                "Hitch add environment on large repo took {:?}",
                add_duration
            );

            ensure_clean_working_tree(test_env)?;

            // Create feature branch
            Command::new("git")
                .args(["checkout", "-b", "large-feature"])
                .current_dir(test_env.path())
                .output()?;

            std::fs::write(
                test_env.path().join("large-feature.txt"),
                "Large feature content",
            )?;
            Command::new("git")
                .args(["add", "large-feature.txt"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["commit", "-m", "Add large feature"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Promote feature
            let promote_start = Instant::now();
            let promote_output = run_hitch_command(test_env, &["promote", "large-feature", "dev"])?;
            let promote_duration = promote_start.elapsed();
            println!("Hitch promote on large repo took {:?}", promote_duration);

            assert!(
                promote_output.status.success(),
                "Promote should work on large repository"
            );

            // Rebuild environment
            let rebuild_start = Instant::now();
            let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;
            let rebuild_duration = rebuild_start.elapsed();
            println!("Hitch rebuild on large repo took {:?}", rebuild_duration);

            assert!(
                rebuild_output.status.success(),
                "Rebuild should work on large repository"
            );

            let total_hitch_time = hitch_start.elapsed();
            println!(
                "Total Hitch operations on large repo took {:?}",
                total_hitch_time
            );

            // Performance assertions
            assert!(
                add_duration < Duration::from_secs(10),
                "Add should complete within 10 seconds on large repo"
            );
            assert!(
                promote_duration < Duration::from_secs(15),
                "Promote should complete within 15 seconds on large repo"
            );
            assert!(
                rebuild_duration < Duration::from_secs(20),
                "Rebuild should complete within 20 seconds on large repo"
            );

            Ok(())
        })
    }

    /// Test performance with many promoted branches
    #[test]
    #[ignore]
        fn test_performance_with_many_promoted_branches() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            let start_time = Instant::now();

            // Create and promote many branches
            let branch_count = 15;
            let mut successful_promotions = 0;

            for i in 0..branch_count {
                let branch_name = format!("feature-{:02}", i);

                // Create feature branch
                std::fs::write(
                    test_env.path().join(&format!("{}.txt", branch_name)),
                    format!("Content for {}", branch_name),
                )?;

                Command::new("git")
                    .args(["add", &format!("{}.txt", branch_name)])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["commit", "-m", &format!("Add {}", branch_name)])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["checkout", "-b", &branch_name])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["checkout", "main"])
                    .current_dir(test_env.path())
                    .output()?;

                ensure_clean_working_tree(test_env)?;

                // Promote branch
                let promote_start = Instant::now();
                let output = run_hitch_command(test_env, &["promote", &branch_name, "dev"])?;
                let promote_duration = promote_start.elapsed();

                if output.status.success() {
                    successful_promotions += 1;
                    ensure_clean_working_tree(test_env)?;
                    println!("Promoted {} in {:?}", branch_name, promote_duration);
                } else {
                    println!(
                        "Failed to promote {} in {:?}",
                        branch_name, promote_duration
                    );
                }
            }

            let total_promotion_time = start_time.elapsed();
            println!(
                "Successfully promoted {}/{} branches in {:?}",
                successful_promotions, branch_count, total_promotion_time
            );
            println!(
                "Average promotion time: {:?}",
                total_promotion_time / branch_count as u32
            );

            // Performance assertions
            assert!(
                successful_promotions >= 10,
                "Should successfully promote at least 10 branches"
            );
            assert!(
                total_promotion_time < Duration::from_secs(60),
                "Should complete promotions within 60 seconds"
            );

            // Test rebuild performance with many promoted branches
            let rebuild_start = Instant::now();
            let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;
            let rebuild_duration = rebuild_start.elapsed();

            assert!(
                rebuild_output.status.success(),
                "Rebuild should work with many promoted branches"
            );
            println!(
                "Rebuild with {} promoted branches took {:?}",
                successful_promotions, rebuild_duration
            );
            assert!(
                rebuild_duration < Duration::from_secs(30),
                "Rebuild should complete within 30 seconds"
            );

            Ok(())
        })
    }

    /// Test performance of status command with complex environments
    #[test]
    #[ignore]
        fn test_status_performance_with_complex_environments() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create multiple environments with different numbers of promoted branches
            let environments = vec![("dev", 5), ("staging", 8), ("prod", 3), ("qa", 4)];

            for (env_name, branch_count) in environments {
                run_hitch_command(test_env, &["add", env_name])?;
                ensure_clean_working_tree(test_env)?;

                // Add promoted branches to this environment
                for i in 0..branch_count {
                    let branch_name = format!("{}-feature-{}", env_name, i);

                    std::fs::write(
                        test_env.path().join(&format!("{}.txt", branch_name)),
                        format!("Content for {}", branch_name),
                    )?;

                    Command::new("git")
                        .args(["add", &format!("{}.txt", branch_name)])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["commit", "-m", &format!("Add {}", branch_name)])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["checkout", "-b", &branch_name])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["checkout", "main"])
                        .current_dir(test_env.path())
                        .output()?;

                    ensure_clean_working_tree(test_env)?;

                    run_hitch_command(test_env, &["promote", &branch_name, env_name])?;
                    ensure_clean_working_tree(test_env)?;
                }
            }

            // Test status performance multiple times
            let status_iterations = 10;
            let mut total_status_time = Duration::ZERO;

            for i in 0..status_iterations {
                let status_start = Instant::now();
                let status_output = run_hitch_command(test_env, &["status"])?;
                let status_duration = status_start.elapsed();

                assert!(
                    status_output.status.success(),
                    "Status should work in iteration {}",
                    i
                );
                total_status_time += status_duration;

                println!("Status iteration {} took {:?}", i, status_duration);
            }

            let average_status_time = total_status_time / status_iterations as u32;
            println!(
                "Average status time over {} iterations: {:?}",
                status_iterations, average_status_time
            );

            // Performance assertions
            assert!(
                average_status_time < Duration::from_millis(500),
                "Average status should complete within 500ms"
            );

            // Run one more status to get output for verification
            let final_status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status_output.status.success(),
                "Final status should work"
            );

            let status_stdout = String::from_utf8_lossy(&final_status_output.stdout);
            assert!(
                status_stdout.contains("dev"),
                "Status should show dev environment"
            );
            assert!(
                status_stdout.contains("staging"),
                "Status should show staging environment"
            );
            assert!(
                status_stdout.contains("prod"),
                "Status should show prod environment"
            );
            assert!(
                status_stdout.contains("qa"),
                "Status should show qa environment"
            );

            Ok(())
        })
    }

    /// Test memory performance with rapid operations
    #[test]
    #[ignore]
        fn test_memory_performance_with_rapid_operations() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Perform rapid operations to test memory stability
            let operation_count = 20;
            let start_time = Instant::now();

            for i in 0..operation_count {
                // Create feature branch
                let branch_name = format!("rapid-feature-{}", i);

                std::fs::write(
                    test_env.path().join(&format!("{}.txt", branch_name)),
                    format!("Rapid content {}", i),
                )?;

                Command::new("git")
                    .args(["add", &format!("{}.txt", branch_name)])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["commit", "-m", &format!("Add {}", branch_name)])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["checkout", "-b", &branch_name])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["checkout", "main"])
                    .current_dir(test_env.path())
                    .output()?;

                ensure_clean_working_tree(test_env)?;

                // Promote
                run_hitch_command(test_env, &["promote", &branch_name, "dev"])?;
                ensure_clean_working_tree(test_env)?;

                // Rebuild
                run_hitch_command(test_env, &["rebuild", "dev"])?;
                ensure_clean_working_tree(test_env)?;

                // Demote (cleanup)
                run_hitch_command(test_env, &["demote", &branch_name, "dev"])?;
                ensure_clean_working_tree(test_env)?;

                if (i + 1) % 5 == 0 {
                    println!("Completed {}/{} rapid operations", i + 1, operation_count);
                }
            }

            let total_time = start_time.elapsed();
            println!(
                "Completed {} rapid operations in {:?}",
                operation_count, total_time
            );
            println!(
                "Average time per operation: {:?}",
                total_time / operation_count as u32
            );

            // Performance assertions
            assert!(
                total_time < Duration::from_secs(120),
                "Should complete rapid operations within 2 minutes"
            );

            // Final status should work (no memory corruption)
            let final_status = run_hitch_command(test_env, &["status"])?;
            assert!(
                final_status.status.success(),
                "Final status should work after rapid operations"
            );

            Ok(())
        })
    }

    /// Test performance scaling with environment complexity
    #[test]
    #[ignore]
        fn test_performance_scaling_with_complexity() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            let complexity_levels = vec![
                ("simple", 1, 1),  // 1 environment, 1 branch
                ("medium", 3, 3),  // 3 environments, 3 branches each
                ("complex", 5, 5), // 5 environments, 5 branches each
            ];

            for (level_name, env_count, branch_count) in complexity_levels {
                println!(
                    "Testing {} complexity: {} environments, {} branches each",
                    level_name, env_count, branch_count
                );

                let level_start = Instant::now();

                // Create environments
                for env_i in 0..env_count {
                    let env_name = format!("{}-env-{}", level_name, env_i);
                    run_hitch_command(test_env, &["add", &env_name])?;
                    ensure_clean_working_tree(test_env)?;

                    // Create and promote branches for this environment
                    for branch_j in 0..branch_count {
                        let branch_name = format!("{}-{}-branch-{}", level_name, env_i, branch_j);

                        std::fs::write(
                            test_env.path().join(&format!("{}.txt", branch_name)),
                            format!("Content for {}", branch_name),
                        )?;

                        Command::new("git")
                            .args(["add", &format!("{}.txt", branch_name)])
                            .current_dir(test_env.path())
                            .output()?;

                        Command::new("git")
                            .args(["commit", "-m", &format!("Add {}", branch_name)])
                            .current_dir(test_env.path())
                            .output()?;

                        Command::new("git")
                            .args(["checkout", "-b", &branch_name])
                            .current_dir(test_env.path())
                            .output()?;

                        Command::new("git")
                            .args(["checkout", "main"])
                            .current_dir(test_env.path())
                            .output()?;

                        ensure_clean_working_tree(test_env)?;

                        run_hitch_command(test_env, &["promote", &branch_name, &env_name])?;
                        ensure_clean_working_tree(test_env)?;
                    }
                }

                let setup_time = level_start.elapsed();
                println!("{} setup completed in {:?}", level_name, setup_time);

                // Test rebuild performance at this complexity level
                let rebuild_start = Instant::now();
                run_hitch_command(test_env, &["rebuild", &format!("{}-env-0", level_name)])?;
                let rebuild_time = rebuild_start.elapsed();

                println!("{} rebuild took {:?}", level_name, rebuild_time);

                // Test status performance at this complexity level
                let status_start = Instant::now();
                run_hitch_command(test_env, &["status"])?;
                let status_time = status_start.elapsed();

                println!("{} status took {:?}", level_name, status_time);

                // Reasonable performance expectations
                assert!(
                    rebuild_time < Duration::from_secs(30),
                    "Rebuild should complete within 30 seconds for {} complexity",
                    level_name
                );
                assert!(
                    status_time < Duration::from_secs(10),
                    "Status should complete within 10 seconds for {} complexity",
                    level_name
                );
            }

            Ok(())
        })
    }
}
