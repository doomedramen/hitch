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

        // Write to hitch-metadata branch
        if git_ops.branch_exists("hitch-metadata")? {
            git_ops.checkout_branch("hitch-metadata")?;
        } else {
            git_ops.create_orphan_branch("hitch-metadata")?;
        }
        fs::write(
            self.path().join("hitch.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        git_ops.add_and_commit(&["hitch.json"], "Initialize hitch configuration")?;
        git_ops.checkout_branch("main")?;

        Ok(())
    }

    fn create_branch_and_commit(&self, branch_name: &str, message: &str) -> Result<()> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;
        git_ops.create_branch_from(branch_name, "main")?;
        fs::write(self.path().join("test.txt"), message)?;
        git_ops.add_and_commit(&["test.txt"], message)?;
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
}

/// Test actual git hook interference by setting up a pre-commit hook that fails
#[test]
fn test_rebuild_with_actual_git_hooks() -> Result<()> {
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

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        // Set up environments with promoted branches
        test_env.create_environment_config("dev", "main", &["feature/test"])?;
        test_env.create_branch_and_commit("feature/test", "Add test feature")?;

        // Create a failing pre-commit hook
        let hooks_dir = test_env.path().join(".git").join("hooks");
        fs::create_dir_all(&hooks_dir)?;

        let pre_commit_hook = hooks_dir.join("pre-commit");
        fs::write(
            &pre_commit_hook,
            "#!/bin/sh\necho 'Simulated hook failure'\nexit 1",
        )?;

        // Make the hook executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&pre_commit_hook)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&pre_commit_hook, perms)?;
        }

        // Verify the hook exists and will fail
        let output = git_ops.run_git_command(&["commit", "--allow-empty", "-m", "test hook"])?;

        if output.status.success() {
            println!("WARNING: Git hook didn't execute as expected");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("Git hook working as expected: {}", stderr);
        }

        // Now attempt rebuild - this should bypass hooks with --no-verify
        let output = test_env.run_hitch_command(&["rebuild", "dev"])?;

        // Print output for debugging
        println!("=== Rebuild Output ===");
        println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        println!("Exit code: {}", output.status);
        println!("=== End Output ===");

        // The rebuild should succeed despite the failing hook
        assert!(
            output.status.success(),
            "Rebuild should succeed even with failing git hooks"
        );

        // Verify we're back on the original branch
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after rebuild"
        );

        // Verify the environment was actually rebuilt (dev branch should be updated)
        let dev_exists = test_env.branch_exists("dev")?;
        assert!(dev_exists, "Dev branch should exist after rebuild");

        Ok(())
    })
}

/// Test rebuild when there are no changes to commit (branches already up to date)
#[test]
fn test_rebuild_nothing_to_commit() -> Result<()> {
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

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        // Add dev environment
        test_env.create_environment_config("dev", "main", &["feature/same-as-main"])?;

        // Create feature branch but don't add any commits to it
        git_ops.create_branch_from("feature/same-as-main", "main")?;
        git_ops.checkout_branch("main")?;

        // Now attempt rebuild - should handle "nothing to commit" gracefully
        let output = test_env.run_hitch_command(&["rebuild", "dev"])?;

        // Print output for debugging
        println!("=== Rebuild Output (No Changes) ===");
        println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        println!("Exit code: {}", output.status);
        println!("=== End Output ===");

        // The rebuild should succeed even when there's nothing to commit
        assert!(
            output.status.success(),
            "Rebuild should succeed when there's nothing to commit"
        );

        // Verify we're back on the original branch
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after rebuild"
        );

        // Verify the environment branch exists
        let dev_exists = test_env.branch_exists("dev")?;
        assert!(dev_exists, "Dev branch should exist after rebuild");

        Ok(())
    })
}

/// Test rebuild cleanup behavior and verify that temp/backup branches are properly cleaned up
#[test]
fn test_rebuild_cleanup_verification() -> Result<()> {
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

        // Clean up any changes from init
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        if !git_ops.is_working_directory_clean()? {
            git_ops.clean_working_directory("Clean up after hitch init")?;
        }

        // Set up environments with promoted branches
        test_env.create_environment_config("dev", "main", &["feature/test"])?;
        test_env.create_branch_and_commit("feature/test", "Add test feature")?;

        // Get list of branches before rebuild
        let branches_before = git_ops.run_git_command(&["branch", "-a"])?;
        let branches_before_str = String::from_utf8_lossy(&branches_before.stdout);
        println!("Branches before rebuild:\n{}", branches_before_str);

        // Run rebuild
        let output = test_env.run_hitch_command(&["rebuild", "dev"])?;

        // Print output for debugging
        println!("=== Rebuild Output ===");
        println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        println!("Exit code: {}", output.status);
        println!("=== End Output ===");

        // The rebuild should succeed
        assert!(output.status.success(), "Rebuild should succeed");

        // Get list of branches after rebuild
        let branches_after = git_ops.run_git_command(&["branch", "-a"])?;
        let branches_after_str = String::from_utf8_lossy(&branches_after.stdout);
        println!("Branches after rebuild:\n{}", branches_after_str);

        // Verify we're back on the original branch
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after rebuild"
        );

        // Verify no temp or backup branches remain
        let branch_lines: Vec<&str> = branches_after_str.lines().collect();
        for line in branch_lines {
            let clean_line = line.trim().replace("* ", "").replace("  ", "");
            if clean_line.starts_with("hitch-tmp-") || clean_line.starts_with("hitch-backup-") {
                panic!(
                    "Found leftover temporary branch after rebuild: {}",
                    clean_line
                );
            }
        }

        // Verify the environment branch exists
        let dev_exists = test_env.branch_exists("dev")?;
        assert!(dev_exists, "Dev branch should exist after rebuild");

        // Verify no cleanup warnings in output
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let combined_output = format!("{}{}", stderr_str, stdout_str);

        if combined_output.contains("Failed to delete backup branch")
            || combined_output.contains("Failed to delete temp branch")
        {
            panic!("Rebuild output contains cleanup failure warnings, which indicates cleanup problems");
        }

        Ok(())
    })
}
