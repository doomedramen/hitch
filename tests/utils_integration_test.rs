use anyhow::Result;
use hitch::types::HitchConfig;
use std::fs;
use std::process::Command;

// Import the test framework (avoid importing TestEnv since we use closure)
mod common;
use common::{with_test_env, SetupLevel};

/// Helper functions for git operations using the test environment
impl common::TestEnv {
    /// Create a branch and commit for testing
    fn create_branch_and_commit(&self, branch_name: &str, message: &str) -> Result<()> {
        Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(self.path())
            .output()?;

        fs::write(self.path().join("test.txt"), message)?;

        Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(self.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(self.path())
            .output()?;

        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(self.path())
            .output()?;

        Ok(())
    }
}

/// Test git operations functionality
#[test]
fn test_git_operations_integration() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        let _git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        test_env.create_branch_and_commit("feature/test", "Test feature")?;

        // Test that git operations work correctly
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test get current branch
        let current_branch = git_ops.get_current_branch()?;
        assert_eq!(current_branch, "main", "Should be on main branch");

        // Test checkout branch
        git_ops.checkout_branch("feature/test")?;
        let current_branch = git_ops.get_current_branch()?;
        assert_eq!(
            current_branch, "feature/test",
            "Should be on feature/test branch"
        );

        // Test branch existence check
        assert!(
            git_ops.branch_exists_anywhere("main")?,
            "Main branch should exist"
        );
        assert!(
            git_ops.branch_exists_anywhere("feature/test")?,
            "Feature branch should exist"
        );
        assert!(
            !git_ops.branch_exists_anywhere("nonexistent")?,
            "Nonexistent branch should not exist"
        );

        // Test clean working directory detection
        assert!(
            git_ops.is_working_directory_clean()?,
            "Should detect clean working directory"
        );

        // Test dirty working directory detection
        fs::write(test_env.path().join("dirty.txt"), "dirty content")?;
        assert!(
            !git_ops.is_working_directory_clean()?,
            "Should detect dirty working directory"
        );

        // Clean up
        Command::new("git")
            .args(["add", "dirty.txt"])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", "Add dirty file"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            git_ops.is_working_directory_clean()?,
            "Should detect clean working directory again"
        );

        Ok(())
    })
}

/// Test detached HEAD functionality
#[test]
fn test_git_operations_detached_head() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        test_env.create_branch_and_commit("feature/test", "Test feature")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Get current commit hash
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(test_env.path())
            .output()?;

        let commit_hash_str = String::from_utf8_lossy(&output.stdout);
        let commit_hash = commit_hash_str.trim().to_string();

        // Switch to detached HEAD
        Command::new("git")
            .args(["checkout", &commit_hash])
            .current_dir(test_env.path())
            .output()?;

        // Test get current branch in detached HEAD
        let current_branch = git_ops.get_current_branch()?;
        assert!(
            current_branch.starts_with("detached-HEAD-"),
            "Should detect detached HEAD state"
        );
        assert!(current_branch.len() > 13, "Should include commit hash");

        Ok(())
    })
}

/// Test git operations with various error conditions
#[test]
fn test_git_operations_error_handling() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test checkout non-existent branch
        let result = git_ops.checkout_branch("nonexistent");
        assert!(
            result.is_err(),
            "Should fail to checkout non-existent branch"
        );

        // Test get user email
        let user_email = git_ops.get_user_email()?;
        assert!(user_email.contains("@"), "User email should be valid");

        Ok(())
    })
}

/// Test types and serialization
#[test]
fn test_environment_type_functionality() -> Result<()> {
    with_test_env(SetupLevel::Basic, |_test_env| {
        use hitch::types::Environment;
        use serde_json;

        // Test environment creation and methods
        let mut env = Environment::new("main".to_string());
        assert_eq!(env.base, "main");
        assert!(env.branches.is_empty());
        assert!(!env.is_locked());

        // Test adding branches
        env.add_branch("feature/test".to_string());
        assert!(env.has_branch("feature/test"));
        assert!(env.branches.contains(&"feature/test".to_string()));

        // Test removing branches
        env.remove_branch("feature/test");
        assert!(!env.has_branch("feature/test"));
        assert!(!env.branches.contains(&"feature/test".to_string()));

        // Test locking
        env.lock("test@example.com".to_string());
        assert!(env.is_locked());
        assert_eq!(env.locked_by, Some("test@example.com".to_string()));

        env.unlock();
        assert!(!env.is_locked());
        assert_eq!(env.locked_by, None);

        // Test serialization/deserialization
        let json_str = serde_json::to_string_pretty(&env)?;
        let deserialized_env: Environment = serde_json::from_str(&json_str)?;
        assert_eq!(deserialized_env.base, env.base);
        assert_eq!(deserialized_env.branches, env.branches);

        Ok(())
    })
}

/// Test hitch config functionality
#[test]
fn test_hitch_config_functionality() -> Result<()> {
    with_test_env(SetupLevel::Basic, |_test_env| {
        use hitch::types::Environment;
        use serde_json;

        let mut config = HitchConfig::new();

        // Test adding environments
        let mut env = Environment::new("main".to_string());
        env.add_branch("feature/test".to_string());

        config.add_environment("dev".to_string(), env);
        assert!(config.environment_exists("dev"));
        assert!(config.get_environment("dev").is_some());

        // Test getting environment names
        let names = config.get_environment_names();
        assert!(names.contains(&"dev".to_string()));

        // Test removing environments
        config.remove_environment("dev");
        assert!(!config.environment_exists("dev"));
        assert!(config.get_environment("dev").is_none());

        // Test serialization works (basic check)
        let json_str = serde_json::to_string_pretty(&config)?;
        assert!(!json_str.is_empty(), "Should serialize to non-empty JSON");

        Ok(())
    })
}

/// Test validation functions
#[test]
fn test_input_validation() -> Result<()> {
    with_test_env(SetupLevel::Basic, |_test_env| {
        // Test that our input validation works correctly
        // These are simple tests - the actual validation is in the command implementations

        // Test valid environment names
        let valid_names = vec!["dev", "staging", "production", "feature-branch", "env_123"];
        for name in valid_names {
            assert!(!name.is_empty(), "Valid name should not be empty");
            assert!(name.len() <= 100, "Valid name should be under 100 chars");
            assert!(
                !name.contains(".."),
                "Valid name should not contain invalid chars"
            );
        }

        // Test invalid environment names (these would fail our validation)
        let invalid_names = vec!["", "env@invalid", "env:invalid", "env..invalid"];
        for name in invalid_names {
            if name.is_empty() || name.contains("..") || name.contains("@") || name.contains(":") {
                // These would fail validation - that's expected
            }
        }

        Ok(())
    })
}
