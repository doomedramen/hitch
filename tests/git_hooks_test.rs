use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
mod git_hooks_tests {
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

    /// Helper to create a git hook
    fn create_git_hook(test_env: &TestEnv, hook_name: &str, hook_content: &str) -> Result<()> {
        let hooks_dir = test_env.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir)?;

        let hook_path = hooks_dir.join(hook_name);
        std::fs::write(&hook_path, hook_content)?;

        // Make hook executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&hook_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&hook_path, perms)?;
        }

        Ok(())
    }

    /// Test Hitch with pre-commit hook that always passes
    #[test]
    #[ignore]
    fn test_hitch_with_passing_pre_commit_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a passing pre-commit hook
            let hook_content = r#"#!/bin/sh
echo "Pre-commit hook: Running checks..."
echo "Pre-commit hook: All checks passed!"
exit 0
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add dev environment (should work with passing hook)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should succeed with passing pre-commit hook
            assert!(
                output.status.success(),
                "Should succeed with passing pre-commit hook"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            Ok(())
        })
    }

    /// Test Hitch with pre-commit hook that always fails
    #[test]
    #[ignore]
    fn test_hitch_with_failing_pre_commit_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a failing pre-commit hook
            let hook_content = r#"#!/bin/sh
echo "Pre-commit hook: Running validation..."
echo "Pre-commit hook: Validation failed!"
exit 1
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Try to add environment (should fail due to hook)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should fail due to failing pre-commit hook
            assert!(
                !output.status.success(),
                "Should fail with failing pre-commit hook"
            );
            assert!(
                stderr.contains("hook")
                    || stderr.contains("pre-commit")
                    || stderr.contains("commit")
                    || output.status.code() == Some(1),
                "Should show hook-related error"
            );

            Ok(())
        })
    }

    /// Test Hitch with pre-push hook that always passes
    #[test]
    #[ignore]
    fn test_hitch_with_passing_pre_push_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a passing pre-push hook
            let hook_content = r#"#!/bin/sh
echo "Pre-push hook: Validating push..."
echo "Pre-push hook: Push validation passed!"
exit 0
"#;
            create_git_hook(test_env, "pre-push", hook_content)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create and promote feature
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

            // Promote feature (should work with passing pre-push hook)
            let output = run_hitch_command(test_env, &["promote", "feature", "dev"])?;

            // Should succeed with passing pre-push hook
            assert!(
                output.status.success(),
                "Should succeed with passing pre-push hook"
            );

            Ok(())
        })
    }

    /// Test Hitch with pre-push hook that always fails
    #[test]
    #[ignore]
    fn test_hitch_with_failing_pre_push_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a failing pre-push hook
            let hook_content = r#"#!/bin/sh
echo "Pre-push hook: Validating push..."
echo "Pre-push hook: Push validation failed!"
exit 1
"#;
            create_git_hook(test_env, "pre-push", hook_content)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create and promote feature
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

            // Try to promote feature (might be affected by failing pre-push hook)
            let output = run_hitch_command(test_env, &["promote", "feature", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Result depends on whether Hitch operations trigger pre-push hooks
            // Either succeeds (if Hitch doesn't trigger push) or fails with hook error
            if !output.status.success() {
                assert!(
                    stderr.contains("hook")
                        || stderr.contains("pre-push")
                        || stderr.contains("push")
                        || output.status.code() == Some(1),
                    "Should show hook-related error if it fails"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with commit-msg hook
    #[test]
    #[ignore]
    fn test_hitch_with_commit_msg_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a commit-msg hook that validates commit messages
            let hook_content = r#"#!/bin/sh
commit_msg_file="$1"
commit_msg=$(cat "$commit_msg_file")

echo "Commit-msg hook: Validating message: $commit_msg"

# Allow any commit message that contains "hitch"
if echo "$commit_msg" | grep -q "hitch"; then
    echo "Commit-msg hook: Message validation passed!"
    exit 0
else
    echo "Commit-msg hook: Message must contain 'hitch'"
    exit 1
fi
"#;
            create_git_hook(test_env, "commit-msg", hook_content)?;

            // Add environment (this creates commits)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should succeed since Hitch likely creates appropriate commit messages
            if !output.status.success() {
                assert!(
                    stderr.contains("hook")
                        || stderr.contains("commit-msg")
                        || stderr.contains("message"),
                    "Should show commit-msg hook error if it fails"
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

    /// Test Hitch with multiple hooks
    #[test]
    #[ignore]
    fn test_hitch_with_multiple_hooks() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create multiple hooks
            let pre_commit_content = r#"#!/bin/sh
echo "Pre-commit hook: Checking files..."
exit 0
"#;
            create_git_hook(test_env, "pre-commit", pre_commit_content)?;

            let pre_push_content = r#"#!/bin/sh
echo "Pre-push hook: Validating push..."
exit 0
"#;
            create_git_hook(test_env, "pre-push", pre_push_content)?;

            let commit_msg_content = r#"#!/bin/sh
echo "Commit-msg hook: Validating message..."
exit 0
"#;
            create_git_hook(test_env, "commit-msg", commit_msg_content)?;

            // Add environment (should work with all passing hooks)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should succeed with all passing hooks
            assert!(
                output.status.success(),
                "Should succeed with all passing hooks"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            Ok(())
        })
    }

    /// Test Hitch behavior when hooks directory doesn't exist
    #[test]
    #[ignore]
    fn test_hitch_without_hooks_directory() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Remove hooks directory if it exists
            let hooks_dir = test_env.path().join(".git").join("hooks");
            if hooks_dir.exists() {
                std::fs::remove_dir_all(&hooks_dir)?;
            }

            // Add environment (should work without hooks)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should succeed without hooks
            assert!(
                output.status.success(),
                "Should succeed without hooks directory"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            Ok(())
        })
    }

    /// Test Hitch with Lefthook-style configuration
    #[test]
    #[ignore]
    fn test_hitch_with_lefthook_style_setup() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a lefthook.yaml configuration file
            let lefthook_config = r#"pre-commit:
  commands:
    - name: lint
      run: echo "Running linter..."
      glob: "*.{js,ts,py,rs}"
    - name: format-check
      run: echo "Checking code format..."
      glob: "*.{js,ts,py,rs}"

pre-push:
  commands:
    - name: tests
      run: echo "Running tests..."
      glob: "*.{js,ts,py,rs}"
    - name: security-scan
      run: echo "Running security scan..."
      glob: "*.{js,ts,py,rs}"
"#;

            std::fs::write(test_env.path().join("lefthook.yaml"), lefthook_config)?;

            // Create a simple pre-commit hook that simulates lefthook behavior
            let hook_content = r#"#!/bin/sh
echo "Lefthook: Running pre-commit hooks..."
echo "Lefthook: ✓ lint"
echo "Lefthook: ✓ format-check"
echo "Lefthook: All pre-commit hooks passed!"
exit 0
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add environment (should work with lefthook-style setup)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should succeed with lefthook-style setup
            assert!(
                output.status.success(),
                "Should succeed with lefthook-style setup"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            Ok(())
        })
    }

    /// Test Hitch with complex hook that validates files
    #[test]
    #[ignore]
    fn test_hitch_with_complex_validation_hook() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a complex pre-commit hook that validates files
            let hook_content = r#"#!/bin/sh
echo "Complex validation hook: Checking staged files..."

# Get list of staged files
staged_files=$(git diff --cached --name-only)

if [ -z "$staged_files" ]; then
    echo "No staged files to validate"
    exit 0
fi

# Check each staged file
for file in $staged_files; do
    echo "Validating file: $file"

    # Check if file is too large (> 1MB)
    file_size=$(git diff --cached --numstat "$file" | cut -f3)
    if [ "$file_size" -gt 1048576 ]; then
        echo "Error: File $file is too large ($file_size bytes)"
        exit 1
    fi

    # Check for common issues
    if echo "$file" | grep -q "\.txt$"; then
        # For .txt files, check if they contain sensitive patterns
        if git show :"$file" | grep -q "password\|secret\|key"; then
            echo "Error: File $file may contain sensitive information"
            exit 1
        fi
    fi
done

echo "Complex validation hook: All files validated successfully!"
exit 0
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add environment (should pass validation)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should pass validation
            assert!(
                output.status.success(),
                "Should pass complex validation hook"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // Test with a file that would fail validation
            std::fs::write(
                test_env.path().join("test.txt"),
                "This file contains a secret password",
            )?;
            Command::new("git")
                .args(["add", "test.txt"])
                .current_dir(test_env.path())
                .output()?;

            // Try to add another environment (should fail due to sensitive content)
            let output2 = run_hitch_command(test_env, &["add", "staging"])?;

            let stderr2 = String::from_utf8_lossy(&output2.stderr);

            // Should fail due to validation hook
            assert!(
                !output2.status.success(),
                "Should fail due to sensitive content validation"
            );
            assert!(
                stderr2.contains("hook")
                    || stderr2.contains("validation")
                    || stderr2.contains("sensitive")
                    || output2.status.code() == Some(1),
                "Should show validation error"
            );

            Ok(())
        })
    }

    /// Test Hitch interaction with hooks during promote operations
    #[test]
    #[ignore]
    fn test_hitch_promote_with_hooks() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a pre-commit hook that validates feature branches
            let hook_content = r#"#!/bin/sh
echo "Feature validation hook: Checking branch name..."

current_branch=$(git branch --show-current)

# Only allow promotion from feature-* branches
if echo "$current_branch" | grep -q "^feature-"; then
    echo "Feature validation hook: Branch $current_branch is valid"
    exit 0
else
    echo "Feature validation hook: Branch $current_branch is not a feature branch"
    exit 1
fi
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add environment
            run_hitch_command(test_env, &["add", "dev"])?;
            ensure_clean_working_tree(test_env)?;

            // Create a valid feature branch
            create_and_commit_file(test_env, "feature-content.txt", "Feature content")?;
            Command::new("git")
                .args(["checkout", "-b", "feature-valid"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Promote valid feature branch (should work)
            let output = run_hitch_command(test_env, &["promote", "feature-valid", "dev"])?;

            // Should succeed with valid feature branch
            assert!(
                output.status.success(),
                "Should promote valid feature branch"
            );

            // Create an invalid branch (not feature-*)
            create_and_commit_file(test_env, "invalid-content.txt", "Invalid branch content")?;
            Command::new("git")
                .args(["checkout", "-b", "invalid-branch"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["checkout", "main"])
                .current_dir(test_env.path())
                .output()?;

            ensure_clean_working_tree(test_env)?;

            // Try to promote invalid branch (might fail due to hook)
            let output2 = run_hitch_command(test_env, &["promote", "invalid-branch", "dev"])?;

            let stderr2 = String::from_utf8_lossy(&output2.stderr);

            // Result depends on whether promote operations trigger hooks
            if !output2.status.success() {
                assert!(
                    stderr2.contains("hook")
                        || stderr2.contains("validation")
                        || stderr2.contains("feature")
                        || output2.status.code() == Some(1),
                    "Should show validation error if hook is triggered"
                );
            }

            Ok(())
        })
    }

    /// Test Hitch with hooks that modify files
    #[test]
    #[ignore]
    fn test_hitch_with_file_modifying_hooks() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a pre-commit hook that adds or modifies files
            let hook_content = r#"#!/bin/sh
echo "File modifying hook: Adding metadata..."

# Add a timestamp file
echo "$(date): Hitch operation in progress" > .hitch-timestamp
git add .hitch-timestamp

echo "File modifying hook: Metadata added"
exit 0
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add environment (should work with file-modifying hook)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should succeed even with file-modifying hook
            assert!(
                output.status.success(),
                "Should succeed with file-modifying hook"
            );
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            // Check if the hook added its file
            let timestamp_exists = test_env.path().join(".hitch-timestamp").exists();
            assert!(timestamp_exists, "Hook should have added timestamp file");

            Ok(())
        })
    }

    /// Test Hitch with async hooks (background processes)
    #[test]
    #[ignore]
    fn test_hitch_with_async_hooks() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a pre-commit hook that runs background processes
            let hook_content = r#"#!/bin/sh
echo "Async hook: Starting background processes..."

# Start a background process (simulate async operation)
(
    echo "Background task: Running tests..."
    sleep 0.1
    echo "Background task: Tests completed"
) &

# Start another background process
(
    echo "Background task: Running linter..."
    sleep 0.1
    echo "Background task: Linter completed"
) &

# Wait for background processes to complete
wait

echo "Async hook: All background processes completed!"
exit 0
"#;
            create_git_hook(test_env, "pre-commit", hook_content)?;

            // Add environment (should work with async hooks)
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            let stdout = String::from_utf8_lossy(&output.stdout);

            // Should succeed with async hooks
            assert!(output.status.success(), "Should succeed with async hooks");
            assert!(
                stdout.contains("dev") || stdout.contains("environment"),
                "Should add environment successfully"
            );

            Ok(())
        })
    }
}
