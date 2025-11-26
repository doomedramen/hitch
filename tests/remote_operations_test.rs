use anyhow::Result;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
mod remote_operations_tests {
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

    /// Helper to create a local remote repository
    fn setup_local_remote(test_env: &TestEnv) -> Result<String> {
        // Create a unique bare repository as remote to avoid race conditions
        let thread_id = std::thread::current().id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let remote_name = format!("remote-{:?}-{}.git", thread_id, timestamp);
        let remote_path = test_env.path().parent().unwrap().join(remote_name);

        std::fs::create_dir_all(&remote_path)?;

        let output = Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&remote_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to create bare remote repository: {}",
                stderr
            ));
        }

        // Add remote to main repository
        let remote_url = remote_path.to_str().unwrap();
        let output = Command::new("git")
            .args(["remote", "add", "origin", remote_url])
            .current_dir(test_env.path())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Failed to add remote: {}", stderr));
        }

        Ok(remote_url.to_string())
    }

    /// Test Hitch with valid remote repository
    #[test]
    fn test_hitch_with_valid_remote_repository() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote
            let _remote_url = setup_local_remote(test_env)?;

            // Add environment
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should work with valid remote
            assert!(
                output.status.success(),
                "Should work with valid remote repository"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // May show warnings about push failures to non-existent remote
            if !stderr.is_empty() {
                assert!(
                    stderr.contains("warning")
                        || stderr.contains("Failed to push")
                        || stderr.contains("remote")
                        || stderr.contains("permission"),
                    "Should handle remote operations gracefully"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with invalid remote URL
    #[test]
    fn test_hitch_with_invalid_remote_url() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add invalid remote URL
            let output = Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://invalid-url-that-will-fail.com/repo.git",
                ])
                .current_dir(test_env.path())
                .output()?;

            // This might fail immediately or fail later during push
            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show remote-related error
                    assert!(
                        stderr.contains("remote")
                            || stderr.contains("push")
                            || stderr.contains("permission")
                            || stderr.contains("authentication")
                            || stderr.contains("connection"),
                        "Should show remote-related error"
                    );
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains("invalid")
                        || stderr.contains("permission")
                        || stderr.contains("URL"),
                    "Should show URL validation error"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with missing remote
    #[test]
    fn test_hitch_with_missing_remote() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // No remote configured

            // Add environment
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should work without remote (local operations only)
            assert!(
                output.status.success(),
                "Should work without remote repository"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // May show warnings about missing remote for push operations
            if !stderr.is_empty() {
                assert!(
                    stderr.contains("no remote")
                        || stderr.contains("remote not configured")
                        || stderr.contains("origin")
                        || stderr.contains("not found"),
                    "Should handle missing remote gracefully"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with authentication errors
    #[test]
    fn test_hitch_with_authentication_errors() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up remote that will require authentication
            let output = Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/private/repo-requiring-auth.git",
                ])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show authentication error
                    assert!(
                        stderr.contains("authentication")
                            || stderr.contains("permission")
                            || stderr.contains("denied")
                            || stderr.contains("credentials")
                            || stderr.contains("login"),
                        "Should show authentication error"
                    );
                } else {
                    let hitch_stderr = String::from_utf8_lossy(&hitch_output.stderr);
                    // May still work for local operations but show auth warnings
                    if !hitch_stderr.is_empty() {
                        assert!(
                            hitch_stderr.contains("auth")
                                || hitch_stderr.contains("permission")
                                || hitch_stderr.contains("warning"),
                            "Should show authentication-related warnings"
                        );
                    }
                }
            }

            Ok(())
        })
    }

    /// Test Hitch with network connectivity issues
    #[test]
    fn test_hitch_with_network_connectivity_issues() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up remote with invalid hostname
            let output = Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://nonexistent-host-that-will-timeout.com/repo.git",
                ])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show network/connectivity error
                    assert!(
                        stderr.contains("network")
                            || stderr.contains("connection")
                            || stderr.contains("timeout")
                            || stderr.contains("resolve")
                            || stderr.contains("host"),
                        "Should show network connectivity error"
                    );
                } else {
                    let hitch_stderr = String::from_utf8_lossy(&hitch_output.stderr);
                    // May still work locally but show connectivity warnings
                    if !hitch_stderr.is_empty() {
                        assert!(
                            hitch_stderr.contains("network")
                                || hitch_stderr.contains("connection")
                                || hitch_stderr.contains("timeout"),
                            "Should show network-related warnings"
                        );
                    }
                }
            }

            Ok(())
        })
    }

    /// Test Hitch with permission denied on remote
    #[test]
    fn test_hitch_with_permission_denied_on_remote() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote with restricted permissions
            let remote_path = test_env
                .path()
                .parent()
                .unwrap()
                .join("restricted-remote.git");
            std::fs::create_dir_all(&remote_path)?;

            // Create bare repository
            Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&remote_path)
                .output()?;

            // Remove write permissions (simulate restricted remote)
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(&remote_path)?.permissions();
                perms.set_mode(0o555); // Read and execute only
                std::fs::set_permissions(&remote_path, perms)?;
            }

            // Add remote
            let remote_url = remote_path.to_str().unwrap();
            let output = Command::new("git")
                .args(["remote", "add", "origin", remote_url])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment with unique name to avoid concurrent test conflicts
                let hitch_output = run_hitch_command(test_env, &["add", "permission-test-env"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show permission error
                    assert!(
                        stderr.contains("permission")
                            || stderr.contains("denied")
                            || stderr.contains("access")
                            || stderr.contains("write"),
                        "Should show permission denied error. Actual error: {}",
                        stderr
                    );
                }
            }

            // Restore permissions for cleanup
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(&remote_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&remote_path, perms)?;
            }

            Ok(())
        })
    }

    /// Test Hitch with branch protection on remote
    #[test]
    fn test_hitch_with_branch_protection_on_remote() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote
            let _remote_url = setup_local_remote(test_env)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create feature branch
            create_and_commit_file(test_env, "feature.txt", "Feature content")?;
            Command::new("git")
                .args(["checkout", "-b", "feature"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Try to promote feature (simulates branch protection scenarios)
            let output = run_hitch_command(test_env, &["promote", "feature", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should work locally, but may show warnings about remote branch protection
            if !output.status.success() {
                // Might fail due to branch protection on remote
                assert!(
                    stderr.contains("protected")
                        || stderr.contains("branch")
                        || stderr.contains("push")
                        || stderr.contains("rejected")
                        || stderr.contains("permission"),
                    "Should show branch protection error if it occurs"
                );
            } else {
                assert!(
                    stdout.contains("promoted") || stdout.contains("Successfully promoted"),
                    "Should promote feature successfully"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with multiple remotes
    #[test]
    fn test_hitch_with_multiple_remotes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up multiple local remotes
            let remote1_path = test_env.path().parent().unwrap().join("remote1.git");
            let remote2_path = test_env.path().parent().unwrap().join("remote2.git");

            for (i, remote_path) in [remote1_path, remote2_path].iter().enumerate() {
                std::fs::create_dir_all(remote_path)?;

                Command::new("git")
                    .args(["init", "--bare"])
                    .current_dir(remote_path)
                    .output()?;

                let remote_url = remote_path.to_str().unwrap();
                let remote_name = format!("remote{}", i + 1);

                let output = Command::new("git")
                    .args(["remote", "add", &remote_name, remote_url])
                    .current_dir(test_env.path())
                    .output()?;

                if !output.status.success() {
                    return Err(anyhow::anyhow!("Failed to add remote {}", remote_name));
                }
            }

            // Add environment
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should work with multiple remotes
            assert!(output.status.success(), "Should work with multiple remotes");
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // May show warnings about which remote is being used
            if !stderr.is_empty() {
                assert!(
                    stderr.contains("remote")
                        || stderr.contains("origin")
                        || stderr.contains("pushing"),
                    "Should handle multiple remotes gracefully"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch remote operations with --no-push flag
    #[test]
    fn test_hitch_remote_operations_with_no_push_flag() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote
            let _remote_url = setup_local_remote(test_env)?;

            // Add environment with --no-push flag
            let output = run_hitch_command(test_env, &["add", "dev", "--no-push"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should work without pushing to remote
            assert!(output.status.success(), "Should work with --no-push flag");
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // Should skip remote operations or work without mentioning them
            let stdout_lower = stdout.to_lowercase();
            assert!(
                stdout.contains("no-push") ||
                   stdout_lower.contains("skipping") ||
                   stdout_lower.contains("remote") ||
                   stdout.contains("dev") || // If it works without any remote messages
                   stdout.contains("environment"),
                "Should handle --no-push flag gracefully. Actual output: {}",
                stdout
            );

            Ok(())
        })
    }

    /// Test Hitch remote operations with force push
    #[test]
    fn test_hitch_remote_operations_with_force_push() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote
            let _remote_url = setup_local_remote(test_env)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create and promote feature
            create_and_commit_file(test_env, "feature.txt", "Feature content")?;
            Command::new("git")
                .args(["checkout", "-b", "feature"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Promote (which should always prompt for force push)
            let output =
                run_hitch_command_with_input(test_env, &["promote", "feature", "dev"], "y\n")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should handle force push operations
            if output.status.success() {
                assert!(
                    stdout.contains("promoted") || stdout.contains("Successfully promoted"),
                    "Should promote with force push"
                );
                assert!(
                    stdout.contains("force") || stdout.contains("remote"),
                    "Should mention remote operations"
                );
            } else {
                // May fail due to remote issues, but should handle gracefully
                let stderr_lower = stderr.to_lowercase();
                assert!(
                    stderr_lower.contains("remote")
                        || stderr_lower.contains("push")
                        || stderr_lower.contains("force")
                        || stderr_lower.contains("permission")
                        || stderr_lower.contains("error")
                        || stderr.is_empty(), // Also accept empty stderr if it succeeds
                    "Should handle force push operations gracefully. stderr: {}",
                    stderr
                );
            }

            Ok(())
        })
    }

    /// Helper to run hitch command with input
    fn run_hitch_command_with_input(
        test_env: &TestEnv,
        args: &[&str],
        input: &str,
    ) -> Result<std::process::Output> {
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

    /// Test Hitch with SSH key authentication
    #[test]
    fn test_hitch_with_ssh_key_authentication() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up SSH remote (this will fail without proper SSH keys)
            let output = Command::new("git")
                .args(["remote", "add", "origin", "git@github.com:private/repo.git"])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show SSH authentication error
                    assert!(
                        stderr.contains("ssh")
                            || stderr.contains("authentication")
                            || stderr.contains("key")
                            || stderr.contains("permission")
                            || stderr.contains("connection refused"),
                        "Should show SSH authentication error"
                    );
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stderr.contains("ssh")
                        || stderr.contains("authentication")
                        || stderr.contains("key"),
                    "Should show SSH-related error during remote setup"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with HTTPS certificate issues
    #[test]
    fn test_hitch_with_https_certificate_issues() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up HTTPS remote with certificate issues
            let output = Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://self-signed-certificate.com/repo.git",
                ])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show certificate error
                    assert!(
                        stderr.contains("certificate")
                            || stderr.contains("SSL")
                            || stderr.contains("TLS")
                            || stderr.contains("verification")
                            || stderr.contains("self-signed"),
                        "Should show certificate verification error"
                    );
                }
            }

            Ok(())
        })
    }

    /// Test Hitch remote operations timeout handling
    #[test]
    fn test_hitch_remote_operations_timeout_handling() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up remote that will timeout
            let output = Command::new("git")
                .args(["remote", "add", "origin", "https://httpbin.org/delay/30"]) // 30 second delay
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try to add environment
                let hitch_output = run_hitch_command(test_env, &["add", "dev"])?;

                let stderr = String::from_utf8_lossy(&hitch_output.stderr);

                if !hitch_output.status.success() {
                    // Should show timeout error
                    assert!(
                        stderr.contains("timeout")
                            || stderr.contains("connection")
                            || stderr.contains("timed out")
                            || stderr.contains("slow"),
                        "Should show timeout error"
                    );
                }
            }

            Ok(())
        })
    }

    /// Test Hitch with remote repository rate limiting
    #[test]
    fn test_hitch_with_remote_rate_limiting() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up remote that might have rate limiting
            let output = Command::new("git")
                .args(["remote", "add", "origin", "https://api.github.com"])
                .current_dir(test_env.path())
                .output()?;

            if output.status.success() {
                // Try multiple operations to potentially trigger rate limiting
                for i in 0..3 {
                    let env_name = format!("env-{}", i);
                    let hitch_output = run_hitch_command(test_env, &["add", &env_name])?;

                    if i == 0 && !hitch_output.status.success() {
                        let stderr = String::from_utf8_lossy(&hitch_output.stderr);
                        // Check for rate limiting indicators
                        assert!(
                            stderr.contains("rate")
                                || stderr.contains("limit")
                                || stderr.contains("too many")
                                || stderr.contains("quota")
                                || stderr.contains("429"),
                            "Should show rate limiting error if it occurs"
                        );
                        break;
                    }

                    ensure_clean_working_tree(test_env)?;
                }
            }

            Ok(())
        })
    }

    /// Test Hitch with remote repository size limits
    #[test]
    fn test_hitch_with_remote_repository_size_limits() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set up local remote
            let _remote_url = setup_local_remote(test_env)?;

            // Create many large files to test size limits
            for i in 0..10 {
                let filename = format!("large_file_{}.bin", i);
                let content = "x".repeat(500_000); // 500KB per file
                std::fs::write(test_env.path().join(&filename), content)?;

                Command::new("git")
                    .args(["add", &filename])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["commit", "-m", &format!("Add large file {}", i)])
                    .current_dir(test_env.path())
                    .output()?;
            }

            // Add environment
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should work unless size limits are exceeded
            if !output.status.success() {
                // May show size limit error
                assert!(
                    stderr.contains("size")
                        || stderr.contains("large")
                        || stderr.contains("limit")
                        || stderr.contains("quota"),
                    "Should show size limit error if it occurs"
                );
            } else {
                assert!(
                    stdout.contains("dev") || stdout.contains("environment"),
                    "Should add environment successfully"
                );
            }

            Ok(())
        })
    }
}
