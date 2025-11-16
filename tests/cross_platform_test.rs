use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
#[allow(dead_code)]
mod cross_platform_tests {
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

    /// Test Hitch with platform-specific file paths
    #[test]
    fn test_platform_specific_file_paths() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Test files with platform-specific naming patterns
            let test_files = vec![
                "normal-file.txt",
                #[cfg(windows)]
                "file\\with\\backslashes.txt",
                #[cfg(not(windows))]
                "file/with/slashes.txt",
                "file-with-dashes.txt",
                "file_with_underscores.txt",
                "file.with.dots.txt",
                "123numeric.txt",
                "UPPERCASE.TXT",
                "MixedCase.txt",
            ];

            for filename in test_files {
                // Create file with platform-specific content
                let content = if cfg!(windows) {
                    format!("Windows content for {}", filename)
                } else if cfg!(target_os = "macos") {
                    format!("macOS content for {}", filename)
                } else {
                    format!("Linux content for {}", filename)
                };

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
            }

            // Add environment and promote one of the files
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create branch with platform-specific name
            let branch_name = if cfg!(windows) {
                "platform\\feature\\windows"
            } else {
                "platform-feature-unix"
            };

            Command::new("git")
                .args(["checkout", "-b", branch_name])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Promote the branch
            let output = run_hitch_command(test_env, &["promote", branch_name, "dev"])?;

            // Should work with platform-specific paths
            assert!(
                output.status.success(),
                "Should work with platform-specific file paths"
            );

            Ok(())
        })
    }

    /// Test Hitch with different line endings
    #[test]
    fn test_different_line_endings() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Configure git for different line endings
            if cfg!(windows) {
                // Windows-style line endings
                Command::new("git")
                    .args(["config", "core.autocrlf", "true"])
                    .current_dir(test_env.path())
                    .output()?;
            } else {
                // Unix-style line endings
                Command::new("git")
                    .args(["config", "core.autocrlf", "false"])
                    .current_dir(test_env.path())
                    .output()?;
            }

            // Create files with different line endings
            let crlf_content = "Line 1\r\nLine 2\r\nLine 3\r\n";
            let lf_content = "Line 1\nLine 2\nLine 3\n";
            let mixed_content = "Line 1\r\nLine 2\nLine 3\r\n";

            std::fs::write(test_env.path().join("crlf.txt"), crlf_content)?;
            std::fs::write(test_env.path().join("lf.txt"), lf_content)?;
            std::fs::write(test_env.path().join("mixed.txt"), mixed_content)?;

            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add files with different line endings"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment and test operations
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with different line endings
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with different line endings"
            );

            Ok(())
        })
    }

    /// Test Hitch with file permissions and attributes
    #[test]
    fn test_file_permissions_and_attributes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create files with different permissions (Unix systems)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                // Create executable file
                let exec_file = test_env.path().join("script.sh");
                std::fs::write(&exec_file, "#!/bin/sh\necho 'Hello World'\n")?;

                let mut perms = std::fs::metadata(&exec_file)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&exec_file, perms)?;

                // Create read-only file
                let readonly_file = test_env.path().join("readonly.txt");
                std::fs::write(&readonly_file, "Read only content")?;

                let mut perms = std::fs::metadata(&readonly_file)?.permissions();
                perms.set_mode(0o444);
                std::fs::set_permissions(&readonly_file, perms)?;
            }

            // Create hidden file (Unix/Mac)
            #[cfg(unix)]
            {
                std::fs::write(test_env.path().join(".hidden"), "Hidden file content")?;
            }

            // Create system file (Windows-style)
            #[cfg(windows)]
            {
                std::fs::write(test_env.path().join("system.dat"), "System file content")?;
            }

            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add files with different permissions"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with different file permissions
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with different file permissions"
            );

            Ok(())
        })
    }

    /// Test Hitch with special characters in file and branch names
    #[test]
    fn test_special_characters_in_names() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Test files with special characters (avoid problematic ones)
            let special_files = vec![
                "file-with-accents-áéíóú.txt",
                "file-with-german-äöüß.txt",
                "file-with-symbols-@#$%.txt",
                "file-with-spaces and tabs\t.txt",
                "file-with-unicode-测试.txt",
                "файл-с-кириллицей.txt",
            ];

            for filename in special_files {
                let content = format!("Content for file: {}", filename);
                std::fs::write(test_env.path().join(filename), content)?;

                Command::new("git")
                    .args(["add", filename])
                    .current_dir(test_env.path())
                    .output()?;

                Command::new("git")
                    .args(["commit", "-m", &format!("Add {}", filename)])
                    .current_dir(test_env.path())
                    .output()?;
            }

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with special characters
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with special characters in names"
            );

            Ok(())
        })
    }

    /// Test Hitch with different character encodings
    #[test]
    fn test_different_character_encodings() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set git to handle different encodings
            Command::new("git")
                .args(["config", "core.quotepath", "off"])
                .current_dir(test_env.path())
                .output()?;

            // Create files with different encodings
            let utf8_content = "UTF-8 content: 中文 русский العربية";
            std::fs::write(test_env.path().join("utf8.txt"), utf8_content)?;

            // Latin-1 content (if supported)
            let latin1_bytes = b"Latin-1 content: caf\xe9 na\xefve";
            std::fs::write(test_env.path().join("latin1.txt"), latin1_bytes)?;

            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add files with different encodings"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with different encodings
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with different character encodings"
            );

            Ok(())
        })
    }

    /// Test Hitch with case-sensitive file systems
    #[test]
    fn test_case_sensitive_file_systems() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Configure git for case sensitivity
            Command::new("git")
                .args(["config", "core.ignorecase", "false"])
                .current_dir(test_env.path())
                .output()?;

            // Create files with different cases
            std::fs::write(test_env.path().join("CaseSensitive.txt"), "Uppercase")?;
            std::fs::write(test_env.path().join("casesensitive.txt"), "Lowercase")?;
            std::fs::write(test_env.path().join("CASESENSITIVE.txt"), "All uppercase")?;

            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add case-sensitive files"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with case-sensitive files
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with case-sensitive file systems"
            );

            Ok(())
        })
    }

    /// Test Hitch with very long file and directory names
    #[test]
    fn test_long_file_and_directory_names() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create very long filename (approaching OS limits)
            let long_name = "a".repeat(200); // 200 characters
            let long_filename = format!("{}.txt", long_name);

            std::fs::write(
                test_env.path().join(&long_filename),
                "Long filename content",
            )?;

            // Create nested directories with long names
            let mut current_path = test_env.path().to_path_buf();
            for i in 0..5 {
                let dir_name = format!("directory_with_very_long_name_{}", i);
                current_path.push(dir_name);
                std::fs::create_dir_all(&current_path)?;
            }

            let deep_file_path = current_path.join("deep_file.txt");
            std::fs::write(deep_file_path, "Deep file content")?;

            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add files with long names"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with long file names
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with long file and directory names"
            );

            Ok(())
        })
    }

    /// Test Hitch with platform-specific commands
    #[test]
    fn test_platform_specific_commands() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Run platform-specific git commands
            if cfg!(windows) {
                // Windows-specific commands
                let output = Command::new("git")
                    .args(["config", "core.symlinks", "false"])
                    .current_dir(test_env.path())
                    .output()?;
                assert!(
                    output.status.success(),
                    "Should configure Windows-specific settings"
                );
            } else {
                // Unix-specific commands
                let output = Command::new("git")
                    .args(["config", "core.symlinks", "true"])
                    .current_dir(test_env.path())
                    .output()?;
                assert!(
                    output.status.success(),
                    "Should configure Unix-specific settings"
                );

                // Create symbolic link (if supported)
                let link_target = test_env.path().join("target.txt");
                std::fs::write(&link_target, "Target content")?;

                let link_path = test_env.path().join("symlink.txt");
                let _symlink_result = std::os::unix::fs::symlink(&link_target, &link_path);
            }

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with platform-specific configurations
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with platform-specific commands"
            );

            Ok(())
        })
    }

    /// Test Hitch environment variable handling
    #[test]
    fn test_environment_variable_handling() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Set environment variables that Hitch might use
            std::env::set_var("HITCH_LOG_LEVEL", "debug");
            std::env::set_var("HITCH_CONFIG_DIR", test_env.path().join(".hitch"));
            std::env::set_var("GIT_AUTHOR_NAME", "Test User");
            std::env::set_var("GIT_AUTHOR_EMAIL", "test@example.com");

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with custom environment variables
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with custom environment variables"
            );

            // Clean up environment variables
            std::env::remove_var("HITCH_LOG_LEVEL");
            std::env::remove_var("HITCH_CONFIG_DIR");
            std::env::remove_var("GIT_AUTHOR_NAME");
            std::env::remove_var("GIT_AUTHOR_EMAIL");

            Ok(())
        })
    }

    /// Test Hitch with different terminal types
    #[test]
    fn test_different_terminal_types() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Test with different terminal configurations
            std::env::set_var("TERM", "xterm-256color");
            std::env::set_var("COLORTERM", "truecolor");

            // Add environment
            let output1 = run_hitch_command(test_env, &["add", "dev"])?;
            assert!(
                output1.status.success(),
                "Should work with xterm-256color terminal"
            );

            std::env::set_var("TERM", "dumb");
            std::env::remove_var("COLORTERM");

            ensure_clean_working_tree(test_env)?;

            // Add another environment
            let output2 = run_hitch_command(test_env, &["add", "staging"])?;
            assert!(output2.status.success(), "Should work with dumb terminal");

            // Clean up environment variables
            std::env::remove_var("TERM");

            Ok(())
        })
    }

    /// Test Hitch with filesystem edge cases
    #[test]
    fn test_filesystem_edge_cases() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create files with filesystem edge cases
            let edge_cases = vec![
                ("", "empty_filename.txt"),
                (" ", "space_filename.txt"),
                (".", "dot_filename.txt"),
                ("..", "dotdot_filename.txt"),
            ];

            for (prefix, suffix) in edge_cases {
                let filename = format!("{}{}", prefix, suffix);
                if !filename.is_empty() && filename != "." && filename != ".." {
                    let content = format!("Edge case: {}", filename);
                    std::fs::write(test_env.path().join(&filename), content)?;

                    Command::new("git")
                        .args(["add", &filename])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["commit", "-m", &format!("Add edge case: {}", filename)])
                        .current_dir(test_env.path())
                        .output()?;
                }
            }

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with filesystem edge cases
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work with filesystem edge cases"
            );

            Ok(())
        })
    }

    /// Test Hitch resource limits and constraints
    #[test]
    fn test_resource_limits_and_constraints() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Test with many small files
            for i in 0..100 {
                let filename = format!("small_file_{:03}.txt", i);
                let content = format!("Small content for file {}", i);
                std::fs::write(test_env.path().join(&filename), content)?;

                // Batch commits every 10 files
                if (i + 1) % 10 == 0 {
                    Command::new("git")
                        .args(["add", "."])
                        .current_dir(test_env.path())
                        .output()?;

                    Command::new("git")
                        .args(["commit", "-m", &format!("Batch commit for files 0-{}", i)])
                        .current_dir(test_env.path())
                        .output()?;
                }
            }

            // Add remaining files and commit
            Command::new("git")
                .args(["add", "."])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Final batch commit"])
                .current_dir(test_env.path())
                .output()?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Should work with resource constraints
            let status_output = run_hitch_command(test_env, &["status"])?;
            assert!(
                status_output.status.success(),
                "Should work within resource limits"
            );

            Ok(())
        })
    }
}
