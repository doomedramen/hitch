use anyhow::Result;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

/// Helper extension trait for TestEnv to provide custom methods needed by these tests
trait TestEnvExt {
    #[allow(dead_code)]
    fn run_hitch_init(&self) -> Result<()>;
    fn create_environment_config(
        &self,
        env_name: &str,
        base_branch: &str,
        branches: &[&str],
    ) -> Result<()>;
    fn create_branch_and_commit(&self, branch_name: &str, message: &str) -> Result<()>;
    fn get_current_branch(&self) -> Result<String>;
    fn branch_exists(&self, branch: &str) -> Result<bool>;
    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output>;
    fn create_file(&self, path: &str, content: &str) -> Result<()>;
    #[allow(dead_code)]
    fn run_git_command(&self, args: &[&str]) -> Result<std::process::Output>;
}

impl TestEnvExt for TestEnv {
    #[allow(dead_code)]
    fn run_hitch_init(&self) -> Result<()> {
        let output = Command::new(self.hitch_binary())
            .args(["init"])
            .current_dir(self.path())
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to run hitch init: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    fn create_environment_config(
        &self,
        env_name: &str,
        base_branch: &str,
        branches: &[&str],
    ) -> Result<()> {
        use std::collections::HashMap;

        let mut environments = HashMap::new();
        let mut branches_vec = Vec::new();
        for branch in branches {
            branches_vec.push(branch.to_string());
        }

        environments.insert(
            env_name.to_string(),
            serde_json::json!({
                "base": base_branch,
                "branches": branches_vec,
                "locked": false,
                "lockedBy": null,
                "lockedAt": null,
                "rebuiltAt": null
            }),
        );

        let config = serde_json::json!({
            "version": "1.0.0",
            "environments": environments
        });

        // Use GitOperations for all git operations
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Write to hitch-metadata branch (not orphan - it should already exist from hitch init)
        if git_ops.branch_exists("hitch-metadata")? {
            git_ops.checkout_branch("hitch-metadata")?;
        } else {
            git_ops.create_orphan_branch("hitch-metadata")?;
        }

        // Update hitch.json with the new environment configuration
        fs::write(
            self.path().join("hitch.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        git_ops.add_and_commit(&["hitch.json"], &format!("Add environment '{}'", env_name))?;
        git_ops.checkout_branch("main")?;

        Ok(())
    }

    fn create_branch_and_commit(&self, branch_name: &str, message: &str) -> Result<()> {
        // Use GitOperations for all git operations
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Ensure we're on main branch first to avoid hitch-metadata .gitignore issues
        git_ops.checkout_branch("main")?;

        git_ops.create_branch_from(branch_name, "main")?;

        // Clean any ignored files from previous operations
        let _ = git_ops.run_git_command(&["clean", "-fd"])?;

        // Use unique filename that won't be ignored by hitch-metadata .gitignore
        // Since hitch-metadata .gitignore has "*", our files will be ignored
        // So we force add them
        let filename = format!("{}.txt", branch_name.replace("/", "_"));
        fs::write(self.path().join(&filename), message)?;
        let _ = git_ops.run_git_command(&["add", "-f", &filename])?;
        let _ = git_ops.run_git_command(&["commit", "-m", message])?;
        git_ops.checkout_branch("main")?;
        Ok(())
    }

    fn get_current_branch(&self) -> Result<String> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;
        git_ops.get_current_branch()
    }

    fn branch_exists(&self, branch: &str) -> Result<bool> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;
        git_ops.branch_exists(branch)
    }

    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new(self.hitch_binary())
            .args(args)
            .current_dir(self.path())
            .output()?;

        Ok(output)
    }

    fn create_file(&self, path: &str, content: &str) -> Result<()> {
        fs::write(self.path().join(path), content)?;
        Ok(())
    }

    #[allow(dead_code)]
    fn run_git_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;
        let output = git_ops.run_git_command(args)?;
        Ok(output)
    }
}

/// Test promote command with invalid arguments
#[test]
fn test_promote_invalid_arguments() -> Result<()> {
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

        // Test missing arguments
        let output = test_env.run_hitch_command(&["promote"])?;
        assert!(
            !output.status.success(),
            "Promote should fail with missing arguments"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("the following required arguments were not provided")
                || stderr.contains("missing required arguments")
                || stderr.contains("expected 2 arguments")
        );

        Ok(())
    })
}

/// Test demote command with invalid arguments
#[test]
fn test_demote_invalid_arguments() -> Result<()> {
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

        // Test missing arguments
        let output = test_env.run_hitch_command(&["demote"])?;
        assert!(
            !output.status.success(),
            "Demote should fail with missing arguments"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("the following required arguments were not provided")
                || stderr.contains("missing required arguments")
                || stderr.contains("expected 2 arguments")
        );

        Ok(())
    })
}

/// Test promote when not initialized
#[test]
fn test_promote_not_initialized() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // NOTE: Do NOT initialize hitch - this test checks what happens when hitch is not initialized

        // Try to promote without hitch init
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail when hitch not initialized"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("not found") || stderr.contains("Failed to read hitch.json"));

        Ok(())
    })
}

/// Test demote when not initialized
#[test]
fn test_demote_not_initialized() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // NOTE: Do NOT initialize hitch - this test checks what happens when hitch is not initialized

        // Try to demote without hitch init
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Demote should fail when hitch not initialized"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("not found") || stderr.contains("Failed to read hitch.json"));

        Ok(())
    })
}

/// Test promote with non-existent environment
#[test]
fn test_promote_nonexistent_environment() -> Result<()> {
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

        // Try to promote to non-existent environment
        let output = test_env.run_hitch_command(&["promote", "feature/test", "nonexistent"])?;
        assert!(
            !output.status.success(),
            "Promote should fail with non-existent environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist"));

        Ok(())
    })
}

/// Test demote with non-existent environment
#[test]
fn test_demote_nonexistent_environment() -> Result<()> {
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

        // Try to demote from non-existent environment
        let output = test_env.run_hitch_command(&["demote", "feature/test", "nonexistent"])?;
        assert!(
            !output.status.success(),
            "Demote should fail with non-existent environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist"));

        Ok(())
    })
}

/// Test promote with non-existent branch
#[test]
fn test_promote_nonexistent_branch() -> Result<()> {
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

        test_env.create_environment_config("dev", "main", &[])?;

        // Ensure we're on main branch with clean working directory
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("main")?;
        let _ = git_ops.run_git_command(&["clean", "-fd"])?;

        // Try to promote non-existent branch
        let output = test_env.run_hitch_command(&["promote", "nonexistent", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail with non-existent branch"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("does not exist"),
            "Expected error about non-existent branch, got: {}",
            stderr
        );

        Ok(())
    })
}

/// Test promote branch that's already promoted
#[test]
fn test_promote_already_promoted_branch() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &["feature/test"])?;

        // Try to promote already promoted branch
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail with already promoted branch"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("already promoted"),
            "Expected error message about already promoted branch, got: {}",
            stderr
        );

        Ok(())
    })
}

/// Test demote branch that's not promoted
#[test]
fn test_demote_not_promoted_branch() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &[])?;

        // Try to demote non-promoted branch
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Demote should fail with non-promoted branch"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("not promoted"));

        Ok(())
    })
}

/// Test promote with dirty working directory
#[test]
fn test_promote_dirty_working_directory() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &[])?;

        // Ensure we're on main branch and clean any gitignore effects
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("main")?;
        let _ = git_ops.run_git_command(&["clean", "-fd"])?;

        // Create untracked file that won't be ignored
        test_env.create_file("untracked.txt", "This should cause pre-check to fail")?;

        // Try to promote
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail with dirty working directory"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Working tree is not clean")
                || stderr.contains("not clean")
                || stderr.contains("unclean")
                || stderr.contains("clean"),
            "Expected error about working directory not being clean, got: {}",
            stderr
        );

        Ok(())
    })
}

/// Test demote with dirty working directory
#[test]
fn test_demote_dirty_working_directory() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &["feature/test"])?;

        // Ensure we're on main branch and clean any gitignore effects
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("main")?;
        let _ = git_ops.run_git_command(&["clean", "-fd"])?;

        // Create untracked file that won't be ignored
        test_env.create_file("untracked.txt", "This should cause pre-check to fail")?;

        // Try to demote
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Demote should fail with dirty working directory"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Working tree is not clean")
                || stderr.contains("not clean")
                || stderr.contains("unclean")
                || stderr.contains("clean"),
            "Expected error about working directory not being clean, got: {}",
            stderr
        );

        Ok(())
    })
}

/// Test promote to locked environment without force
#[test]
fn test_promote_locked_environment() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &[])?;

        // Lock the environment manually
        #[allow(unused_assignments)]
        let mut config_content = String::new();
        {
            use std::collections::HashMap;
            let mut environments = HashMap::new();
            environments.insert(
                "dev".to_string(),
                serde_json::json!({
                    "base": "main",
                    "branches": [],
                    "locked": true,
                    "lockedBy": "test@example.com",
                    "lockedAt": "2024-01-01T00:00:00Z",
                    "rebuiltAt": null
                }),
            );

            let config = serde_json::json!({
                "version": "1.0.0",
                "environments": environments
            });

            config_content = serde_json::to_string_pretty(&config)?;
        }

        // Update hitch.json with locked environment
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if git_ops.branch_exists("hitch-metadata")? {
            git_ops.checkout_branch("hitch-metadata")?;
        } else {
            git_ops.create_orphan_branch("hitch-metadata")?;
        }
        fs::write(test_env.path().join("hitch.json"), config_content)?;
        git_ops.add_and_commit(&["hitch.json"], "Lock environment")?;
        git_ops.checkout_branch("main")?;

        // Try to promote to locked environment
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail to locked environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("locked") || stderr.contains("currently locked"));

        Ok(())
    })
}

/// Test demote from locked environment without force
#[test]
fn test_demote_locked_environment() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &["feature/test"])?;

        // Lock the environment manually
        #[allow(unused_assignments)]
        let mut config_content = String::new();
        {
            use std::collections::HashMap;
            let mut environments = HashMap::new();
            environments.insert(
                "dev".to_string(),
                serde_json::json!({
                    "base": "main",
                    "branches": ["feature/test"],
                    "locked": true,
                    "lockedBy": "test@example.com",
                    "lockedAt": "2024-01-01T00:00:00Z",
                    "rebuiltAt": null
                }),
            );

            let config = serde_json::json!({
                "version": "1.0.0",
                "environments": environments
            });

            config_content = serde_json::to_string_pretty(&config)?;
        }

        // Update hitch.json with locked environment
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if git_ops.branch_exists("hitch-metadata")? {
            git_ops.checkout_branch("hitch-metadata")?;
        } else {
            git_ops.create_orphan_branch("hitch-metadata")?;
        }
        fs::write(test_env.path().join("hitch.json"), config_content)?;
        git_ops.add_and_commit(&["hitch.json"], "Lock environment")?;
        git_ops.checkout_branch("main")?;

        // Try to demote from locked environment
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Demote should fail to locked environment"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("locked") || stderr.contains("currently locked"));

        Ok(())
    })
}

/// Test promote command in non-git repository
#[test]
fn test_promote_non_git_repository() -> Result<()> {
    with_test_env(SetupLevel::Basic, |test_env| {
        // Don't initialize git, just try to promote
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail in non-git repository"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Not in a Git repository") || stderr.contains("git repository"));

        Ok(())
    })
}

/// Test demote command in non-git repository
#[test]
fn test_demote_non_git_repository() -> Result<()> {
    with_test_env(SetupLevel::Basic, |test_env| {
        // Don't initialize git, just try to demote
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Demote should fail in non-git repository"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Not in a Git repository") || stderr.contains("git repository"));

        Ok(())
    })
}

/// Test promote and demote workflow integration
#[test]
fn test_promote_demote_integration_workflow() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &[])?;

        // 1. Promote the branch
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        if !output.status.success() {
            println!("Promote failed in integration workflow:");
            println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
            println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
        assert!(output.status.success(), "Promote should succeed");

        // Verify we're back on main branch
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after promote"
        );

        // Verify dev branch was rebuilt
        assert!(
            test_env.branch_exists("dev")?,
            "Dev branch should exist after promote"
        );

        // 2. Demote the branch
        let output = test_env.run_hitch_command(&["demote", "feature/test", "dev"])?;
        assert!(output.status.success(), "Demote should succeed");

        // Verify we're back on main branch
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after demote"
        );

        // Verify dev branch was rebuilt again
        assert!(
            test_env.branch_exists("dev")?,
            "Dev branch should still exist after demote"
        );

        Ok(())
    })
}

/// Test promote with environment that has missing base branch
#[test]
fn test_promote_environment_missing_base_branch() -> Result<()> {
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

        // Create environment with non-existent base branch
        test_env.create_environment_config("dev", "nonexistent-base", &[])?;

        // Try to promote
        let output = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output.status.success(),
            "Promote should fail when environment has missing base branch"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("does not exist") || stderr.contains("Base branch"));

        Ok(())
    })
}

/// Test concurrent operations simulation
#[test]
fn test_concurrent_operations_simulation() -> Result<()> {
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
        test_env.create_environment_config("dev", "main", &[])?;

        // Simulate concurrent access by running promote twice in sequence quickly
        let output1 = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(output1.status.success(), "First promote should succeed");

        let current_branch = test_env.get_current_branch()?;
        assert_eq!(current_branch, "main", "Should be back on main branch");

        // Second promote should fail because branch is already promoted
        let output2 = test_env.run_hitch_command(&["promote", "feature/test", "dev"])?;
        assert!(
            !output2.status.success(),
            "Second promote should fail because branch is already promoted"
        );

        Ok(())
    })
}
