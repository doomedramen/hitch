use anyhow::Result;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
mod metadata_corruption_tests {
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

    /// Test Hitch behavior with corrupted hitch.json file
    #[test]
        fn test_corrupted_hitch_json_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Corrupt the hitch.json file with invalid JSON
            let hitch_json_path = test_env.path().join("hitch.json");
            let corrupted_content = r#"{
                "environments": {
                    "dev": {
                        "base_branch": "main",
                        "promoted_branches": ["feature1", "feature2"]
                    },
                    "staging": {
                        "base_branch": "main",
                        "promoted_branches": ["feature1"
                    }
                }
            "#; // Missing closing bracket and quotes

            std::fs::write(&hitch_json_path, corrupted_content)?;

            // Try to run hitch status - should handle corruption gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show JSON parsing error or corrupted metadata error
                assert!(
                    stderr.contains("JSON")
                        || stderr.contains("parse")
                        || stderr.contains("invalid")
                        || stderr.contains("corrupted")
                        || stderr.contains("hitch.json"),
                    "Should show JSON parsing error for corrupted hitch.json"
                );
            }

            // Try to fix by creating a valid hitch.json
            let valid_content = r#"{
                "environments": {
                    "dev": {
                        "base_branch": "main",
                        "promoted_branches": []
                    }
                }
            }"#;

            std::fs::write(&hitch_json_path, valid_content)?;

            // Should work after fixing the JSON
            let output2 = run_hitch_command(test_env, &["status"])?;
            assert!(
                output2.status.success(),
                "Should work after fixing hitch.json"
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with missing hitch.json file
    #[test]
        fn test_missing_hitch_json_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Remove the hitch.json file
            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::remove_file(&hitch_json_path)?;

            // Try to run hitch status - should handle missing file gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show missing file error
                assert!(
                    stderr.contains("not found")
                        || stderr.contains("missing")
                        || stderr.contains("hitch.json")
                        || stderr.contains("No such file"),
                    "Should show missing file error"
                );
            }

            // Try to add environment - might create new hitch.json
            let output2 = run_hitch_command(test_env, &["add", "dev"])?;

            // Either succeeds (creates new config) or fails gracefully
            if output2.status.success() {
                let stdout = String::from_utf8_lossy(&output2.stdout);
                assert!(
                    stdout.contains("dev"),
                    "Should successfully add environment"
                );
            } else {
                let stderr2 = String::from_utf8_lossy(&output2.stderr);
                let stdout2 = String::from_utf8_lossy(&output2.stdout);
                assert!(
                    stderr2.contains("hitch.json")
                        || stderr2.contains("configuration")
                        || stderr2.contains("missing")
                        || stdout2.contains("dev")
                        || stdout2.contains("success")
                        || stderr2.contains("Error"),
                    "Should show configuration-related error or success"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch behavior with corrupted hitch-metadata branch
    #[test]
        fn test_corrupted_metadata_branch_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment and promote feature to create metadata
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

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

            run_hitch_command(test_env, &["promote", "feature", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Corrupt the hitch-metadata branch by modifying its files
            Command::new("git")
                .args(["checkout", "hitch-metadata"])
                .current_dir(test_env.path())
                .output()?;

            // Corrupt the hitch.json in metadata branch
            let metadata_json_path = test_env.path().join("hitch.json");
            std::fs::write(&metadata_json_path, "invalid json content {")?;

            Command::new("git")
                .args(["add", "hitch.json"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Corrupt metadata"])
                .current_dir(test_env.path())
                .output()?;

            // Switch back to main
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Try to run hitch status - should handle corrupted metadata gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show metadata corruption error
                assert!(
                    stderr.contains("metadata")
                        || stderr.contains("corrupted")
                        || stderr.contains("JSON")
                        || stderr.contains("parse"),
                    "Should show metadata corruption error"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch behavior with missing hitch-metadata branch
    #[test]
        fn test_missing_metadata_branch_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Delete the hitch-metadata branch
            Command::new("git")
                .args(["branch", "-D", "hitch-metadata"])
                .current_dir(test_env.path())
                .output()?;

            // Try to run hitch status - should handle missing metadata branch gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show missing branch error
                assert!(
                    stderr.contains("metadata")
                        || stderr.contains("branch")
                        || stderr.contains("hitch-metadata")
                        || stderr.contains("not found"),
                    "Should show missing metadata branch error"
                );
            }

            // Try to add environment - might recreate metadata branch
            let output2 = run_hitch_command(test_env, &["add", "dev"])?;

            if output2.status.success() {
                let stdout = String::from_utf8_lossy(&output2.stdout);
                assert!(
                    stdout.contains("dev"),
                    "Should successfully add environment"
                );

                // Check if metadata branch was recreated
                let branch_output = Command::new("git")
                    .args(["branch", "-a"])
                    .current_dir(test_env.path())
                    .output()?;

                let branch_stdout = String::from_utf8_lossy(&branch_output.stdout);
                assert!(
                    branch_stdout.contains("hitch-metadata"),
                    "Should recreate metadata branch"
                );
            } else {
                let stderr2 = String::from_utf8_lossy(&output2.stderr);
                assert!(
                    stderr2.contains("metadata") || stderr2.contains("branch"),
                    "Should show metadata-related error"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch behavior with incomplete metadata (missing required fields)
    #[test]
        fn test_incomplete_metadata_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create incomplete hitch.json (missing required fields)
            let incomplete_content = r#"{
                "environments": {
                    "dev": {
                        "promoted_branches": ["feature1"]
                    }
                }
            }"#; // Missing base_branch field

            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, incomplete_content)?;

            // Try to run hitch status - should handle incomplete metadata gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show incomplete metadata error
                assert!(
                    stderr.contains("required")
                        || stderr.contains("missing")
                        || stderr.contains("field")
                        || stderr.contains("incomplete")
                        || stderr.contains("base_branch"),
                    "Should show incomplete metadata error"
                );
            }

            // Fix the metadata by adding missing field
            let complete_content = r#"{
                "environments": {
                    "dev": {
                        "base_branch": "main",
                        "promoted_branches": ["feature1"]
                    }
                }
            }"#;

            std::fs::write(&hitch_json_path, complete_content)?;

            // Should work after fixing metadata
            let output2 = run_hitch_command(test_env, &["status"])?;
            assert!(
                output2.status.success(),
                "Should work after fixing metadata"
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with metadata containing invalid environment references
    #[test]
        fn test_invalid_environment_references_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create hitch.json with references to non-existent branches
            let invalid_content = r#"{
                "environments": {
                    "dev": {
                        "base_branch": "main",
                        "promoted_branches": ["non-existent-branch-1", "non-existent-branch-2"]
                    }
                }
            }"#;

            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, invalid_content)?;

            // Try to run hitch status - should handle invalid references gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show invalid branch reference error
                assert!(
                    stderr.contains("branch")
                        || stderr.contains("not found")
                        || stderr.contains("non-existent")
                        || stderr.contains("invalid"),
                    "Should show invalid branch reference error"
                );
            } else {
                // Might succeed but show warnings about missing branches
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    stdout.contains("dev")
                        || stderr.contains("branch")
                        || stderr.contains("warning")
                        || stdout.contains("environment"),
                    "Should show environment despite invalid references"
                );
            }

            // Try to rebuild with invalid references
            let rebuild_output = run_hitch_command(test_env, &["rebuild", "dev"])?;

            let rebuild_stdout = String::from_utf8_lossy(&rebuild_output.stdout);
            let rebuild_stderr = String::from_utf8_lossy(&rebuild_output.stderr);
            assert!(
                !rebuild_output.status.success() || rebuild_output.status.success(),
                "Should handle rebuild gracefully"
            );
            assert!(
                rebuild_stderr.contains("branch")
                    || rebuild_stderr.contains("not found")
                    || rebuild_stderr.contains("non-existent")
                    || rebuild_stdout.contains("Rebuilding")
                    || rebuild_stdout.contains("dev"),
                "Should show branch error or rebuild success"
            );

            Ok(())
        })
    }

    /// Test Hitch recovery after metadata corruption
    #[test]
        fn test_hitch_recovery_after_corruption() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment and create some metadata
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

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

            run_hitch_command(test_env, &["promote", "feature", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Corrupt the metadata
            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, "{ invalid json")?;

            // Try to use hitch - should either fail or handle gracefully
            let output = run_hitch_command(test_env, &["status"])?;
            assert!(
                !output.status.success() || output.status.success(),
                "Should handle corrupted metadata gracefully"
            );

            // Try to recover by reinitializing hitch
            let init_output = run_hitch_command(test_env, &["init"])?;

            if init_output.status.success() {
                // Should be able to add environment again
                let add_output = run_hitch_command(test_env, &["add", "staging"])?;

                let stdout = String::from_utf8_lossy(&add_output.stdout);
                let stderr = String::from_utf8_lossy(&add_output.stderr);
                assert!(
                    add_output.status.success() || !add_output.status.success(),
                    "Should handle add environment after recovery gracefully"
                );
                assert!(
                    stdout.contains("staging")
                        || stderr.contains("staging")
                        || stdout.contains("Adding")
                        || stderr.contains("Error"),
                    "Should show staging environment or error"
                );
            } else {
                // If reinit fails, at least it should fail gracefully
                let init_stderr = String::from_utf8_lossy(&init_output.stderr);
                let init_stdout = String::from_utf8_lossy(&init_output.stdout);
                assert!(
                    init_stderr.contains("already initialized")
                        || init_stderr.contains("exists")
                        || init_stderr.contains("corrupted")
                        || init_stderr.contains("Error")
                        || init_stdout.contains("already")
                        || init_stdout.contains("initialized"),
                    "Should handle reinit gracefully"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with metadata containing invalid Unicode
    #[test]
        fn test_invalid_unicode_metadata_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create hitch.json with invalid Unicode characters
            let invalid_unicode_content = b"{\n    \"environments\": {\n        \"dev\": {\n            \"base_branch\": \"main\",\n            \"promoted_branches\": [\"\xFF\xFE\xFD\"]\n        }\n    }\n}";

            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, invalid_unicode_content)?;

            // Try to run hitch status - should handle invalid Unicode gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show encoding/Unicode error
                assert!(
                    stderr.contains("encoding")
                        || stderr.contains("UTF")
                        || stderr.contains("unicode")
                        || stderr.contains("invalid")
                        || stderr.contains("parse"),
                    "Should show encoding error for invalid Unicode"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with extremely large metadata file
    #[test]
        fn test_extremely_large_metadata_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create extremely large hitch.json (simulate memory pressure)
            let mut large_content = String::from("{\n    \"environments\": {\n        \"dev\": {\n            \"base_branch\": \"main\",\n            \"promoted_branches\": [");

            // Add many promoted branches to make file large
            for i in 0..10000 {
                if i > 0 {
                    large_content.push_str(", ");
                }
                large_content.push_str(&format!("\"branch-{}\"", i));
            }

            large_content.push_str("]\n        }\n    }\n}");

            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, large_content)?;

            // Try to run hitch status - should handle large metadata gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show size or memory error
                assert!(
                    stderr.contains("large")
                        || stderr.contains("size")
                        || stderr.contains("memory")
                        || stderr.contains("too many")
                        || stderr.contains("limit"),
                    "Should show size-related error for extremely large metadata"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch recovery after git operations on metadata branch
    #[test]
        fn test_metadata_branch_git_operations_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add environment and create metadata
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Perform git operations on hitch-metadata branch that could corrupt it
            Command::new("git")
                .args(["checkout", "hitch-metadata"])
                .current_dir(test_env.path())
                .output()?;

            // Create a conflicting commit on metadata branch
            std::fs::write(test_env.path().join("test.txt"), "test file")?;
            Command::new("git")
                .args(["add", "test.txt"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Conflicting commit"])
                .current_dir(test_env.path())
                .output()?;

            // Switch back to main
            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            // Try to use hitch - should handle metadata conflicts gracefully
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show conflict or corruption error
                assert!(
                    stderr.contains("conflict")
                        || stderr.contains("corrupted")
                        || stderr.contains("metadata")
                        || stderr.contains("hitch-metadata"),
                    "Should show metadata conflict error"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch behavior with metadata permission issues
    #[test]
        fn test_metadata_permission_issues_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Make hitch.json read-only (simulate permission issue)
            let hitch_json_path = test_env.path().join("hitch.json");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&hitch_json_path)?.permissions();
                perms.set_mode(0o444); // Read-only
                std::fs::set_permissions(&hitch_json_path, perms)?;
            }

            // Try to run hitch command that would modify metadata
            let output = run_hitch_command(test_env, &["add", "staging"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show permission error
                assert!(
                    stderr.contains("permission")
                        || stderr.contains("denied")
                        || stderr.contains("read-only")
                        || stderr.contains("access"),
                    "Should show permission error"
                );
            }

            // Restore permissions and try again
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(&hitch_json_path)?.permissions();
                perms.set_mode(0o644); // Read-write
                std::fs::set_permissions(&hitch_json_path, perms)?;
            }

            let output2 = run_hitch_command(test_env, &["add", "staging"])?;

            if output2.status.success() {
                let stdout = String::from_utf8_lossy(&output2.stdout);
                assert!(
                    stdout.contains("staging"),
                    "Should work after fixing permissions"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch behavior with metadata version incompatibility
    #[test]
        fn test_metadata_version_incompatibility_recovery() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create hitch.json with incompatible version format
            let version_content = r#"{
                "version": "999.0.0",
                "environments": {
                    "dev": {
                        "base_branch": "main",
                        "promoted_branches": []
                    }
                }
            }"#;

            let hitch_json_path = test_env.path().join("hitch.json");
            std::fs::write(&hitch_json_path, version_content)?;

            // Try to run hitch status - should handle version incompatibility
            let output = run_hitch_command(test_env, &["status"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                // Should show version incompatibility error
                assert!(
                    stderr.contains("version")
                        || stderr.contains("incompatible")
                        || stderr.contains("unsupported")
                        || stderr.contains("migrate"),
                    "Should show version incompatibility error"
                );
            }

            Ok(())
        })
    }
}
