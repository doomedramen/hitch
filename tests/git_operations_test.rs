use anyhow::Result;
use std::fs;
use std::process::Command;

mod common;
use common::{with_test_env, SetupLevel};

/// Test git operations functionality
#[test]
fn test_git_operations_basic_functionality() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        // Test git operations
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test get_current_branch (should always be main)
        let current_branch = git_ops.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should use 'main' as default branch"
        );

        // Test get_user_email
        let email = git_ops.get_user_email()?;
        // Don't assert specific email as it may vary by system, just verify it's a valid email format
        assert!(
            email.contains('@'),
            "Email should contain @ symbol: {}",
            email
        );

        // Test is_working_directory_clean
        let is_clean = git_ops.is_working_directory_clean()?;
        assert!(is_clean);

        // Test branch_exists
        assert!(git_ops.branch_exists("main")?);
        assert!(!git_ops.branch_exists("nonexistent")?);

        // Test checkout_branch
        Command::new("git")
            .args(["checkout", "-b", "test-branch"])
            .current_dir(test_env.path())
            .output()?;
        assert_eq!(git_ops.get_current_branch()?, "test-branch");

        // Test fetch_branch (should fail gracefully with no remote)
        let result = git_ops.fetch_branch("nonexistent-branch");
        assert!(
            result.is_ok(),
            "Fetch should fail gracefully for non-existent branch"
        );

        Ok(())
    })
}

/// Test git operations with untracked files
#[test]
fn test_git_operations_dirty_working_directory() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Initially clean
        assert!(git_ops.is_working_directory_clean()?);

        // Add untracked file
        fs::write(test_env.path().join("untracked.txt"), "untracked content")?;
        assert!(!git_ops.is_working_directory_clean()?);

        // Add staged file
        Command::new("git")
            .args(["add", "untracked.txt"])
            .current_dir(test_env.path())
            .output()?;
        assert!(!git_ops.is_working_directory_clean()?);

        // Commit to make clean again
        Command::new("git")
            .args(["commit", "-m", "Add untracked file"])
            .current_dir(test_env.path())
            .output()?;
        assert!(git_ops.is_working_directory_clean()?);

        Ok(())
    })
}

/// Test git operations with detached HEAD
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Get commit hash
        let commit_hash_output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(test_env.path())
            .output()?;
        let commit_hash_binding = String::from_utf8_lossy(&commit_hash_output.stdout);
        let commit_hash_str = commit_hash_binding.trim();

        // Switch to detached HEAD
        Command::new("git")
            .args(["checkout", commit_hash_str])
            .current_dir(test_env.path())
            .output()?;

        // Test get_current_branch with detached HEAD
        let current_branch = git_ops.get_current_branch()?;
        assert!(current_branch.starts_with("detached-HEAD-"));

        Ok(())
    })
}

/// Test git operations file reading and writing
#[test]
fn test_git_operations_file_operations() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        // Create test branch
        Command::new("git")
            .args(["checkout", "-b", "test-branch"])
            .current_dir(test_env.path())
            .output()?;
        fs::write(test_env.path().join("test.txt"), "test content")?;
        Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "Add test file"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test read_file_from_branch
        let content = git_ops.read_file_from_branch("test-branch", "test.txt")?;
        assert_eq!(content, "test content");

        // Test read_file_from_branch with non-existent file
        let result = git_ops.read_file_from_branch("test-branch", "nonexistent.txt");
        assert!(result.is_err(), "Should fail to read non-existent file");

        Ok(())
    })
}

/// Test git operations with invalid git repository
#[test]
fn test_git_operations_invalid_repository() -> Result<()> {
    // Test with non-git directory using a temporary directory
    with_test_env(SetupLevel::Basic, |test_env| {
        let result = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        );
        assert!(
            result.is_err(),
            "Should fail to create GitOperations in non-git directory"
        );
        Ok(())
    })
}

/// Test git operations push functionality
#[test]
fn test_git_operations_push() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test push_branch (should fail gracefully with no remote)
        let result = git_ops.push_branch("main");
        assert!(
            result.is_err(),
            "Push should fail gracefully with no remote configured"
        );

        Ok(())
    })
}

/// Test git operations merge functionality
#[test]
fn test_git_operations_merge() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Create feature branch
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(test_env.path())
            .output()?;
        fs::write(test_env.path().join("feature.txt"), "feature content")?;
        Command::new("git")
            .args(["add", "feature.txt"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "Add feature"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        // Test squash_merge
        let result = git_ops.squash_merge("feature", "Squash merge feature branch");
        assert!(result.is_ok(), "Squash merge should succeed");

        // Verify merge
        assert!(fs::metadata(test_env.path().join("feature.txt")).is_ok());

        // Test check_merge_conflicts_detailed
        let (has_conflicts, conflicted_files) =
            git_ops.check_merge_conflicts_detailed("feature")?;
        assert!(!has_conflicts, "Should have no conflicts");
        assert!(
            conflicted_files.is_none(),
            "Should have no conflicted files"
        );

        Ok(())
    })
}

/// Test git operations error handling
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Test checkout to non-existent branch
        let result = git_ops.checkout_branch("nonexistent");
        assert!(
            result.is_err(),
            "Should fail to checkout non-existent branch"
        );

        // Test squash_merge with non-existent branch
        let result = git_ops.squash_merge("nonexistent", "Should fail");
        assert!(
            result.is_err(),
            "Should fail to squash merge non-existent branch"
        );

        // Test check_merge_conflicts_detailed with non-existent branch
        let result = git_ops.check_merge_conflicts_detailed("nonexistent");
        assert!(
            result.is_err(),
            "Should fail to check conflicts for non-existent branch"
        );

        Ok(())
    })
}

#[test]
fn test_debug_merge_conflicts_scenario() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        // Recreate the scenario from the failing rebuild test exactly
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Create feature branch with content (exactly like rebuild test)
        git_ops.create_branch_from("feature/login", "main")?;
        fs::write(test_env.path().join("login.md"), "# Login feature\n")?;
        git_ops.add_and_commit(&["login.md"], "Add login feature")?;
        git_ops.checkout_branch("main")?;

        // Create hitch config manually (simplified version of what rebuild test does)
        use std::collections::HashMap;
        let mut environments = HashMap::new();
        let branches_vec: Vec<String> = vec!["feature/login".to_string()];

        environments.insert(
            "dev".to_string(),
            serde_json::json!({
                "base": "main",
                "branches": branches_vec,
                "locked": false
            }),
        );

        let config = serde_json::json!({ "environments": environments });

        // Create hitch-metadata branch and write config (only if it doesn't already exist)
        if !git_ops.branch_exists("hitch-metadata")? {
            git_ops.create_orphan_branch("hitch-metadata")?;
        }
        std::fs::write(
            test_env.path().join("hitch.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        std::fs::write(
            test_env.path().join(".gitignore"),
            "*\n!.gitignore\n!hitch.json\n",
        )?;
        git_ops.add_and_commit(&["hitch.json", ".gitignore"], "Add hitch configuration")?;
        git_ops.checkout_branch("main")?;

        // Check for merge conflicts
        let (has_conflicts, conflicted_files) =
            git_ops.check_merge_conflicts_detailed("feature/login")?;

        println!("Current branch: {}", git_ops.get_current_branch()?);
        println!(
            "feature/login exists: {}",
            git_ops.branch_exists("feature/login")?
        );
        println!("Merge conflicts detected: {}", has_conflicts);
        if let Some(files) = conflicted_files {
            println!("Conflicted files: {:?}", files);
        }

        // This might detect conflicts if hitch init changed files
        println!("Files in main:");
        let output = std::process::Command::new("git")
            .args(["ls-tree", "--name-only", "HEAD"])
            .current_dir(test_env.path())
            .output()?;
        println!("{}", String::from_utf8_lossy(&output.stdout));

        Ok(())
    })
}

/// Test git operations branch synchronization functionality
#[test]
fn test_git_operations_branch_synchronization() -> Result<()> {
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

        // Return to main branch for this test since we're testing GitOperations, not hitch
        git_ops.checkout_branch("main")?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;

        // Create a couple of test branches
        Command::new("git")
            .args(["checkout", "-b", "feature1"])
            .current_dir(test_env.path())
            .output()?;
        fs::write(test_env.path().join("feature1.txt"), "feature1 content")?;
        Command::new("git")
            .args(["add", "feature1.txt"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "Add feature1"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["checkout", "-b", "feature2"])
            .current_dir(test_env.path())
            .output()?;
        fs::write(test_env.path().join("feature2.txt"), "feature2 content")?;
        Command::new("git")
            .args(["add", "feature2.txt"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "Add feature2"])
            .current_dir(test_env.path())
            .output()?;
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        // Test synchronize_branches with existing local branches
        let branches = vec!["feature1".to_string(), "feature2".to_string()];
        let result = git_ops.synchronize_branches(&branches);
        assert!(
            result.is_ok(),
            "Synchronize branches should succeed with existing local branches"
        );

        // Test fetch_all_remotes (should succeed gracefully even with no remote)
        let result = git_ops.fetch_all_remotes();
        assert!(
            result.is_ok(),
            "Fetch all remotes should succeed gracefully"
        );

        // Test create_local_branch_from_remote with a branch that already exists locally
        let result = git_ops.create_local_branch_from_remote("feature1");
        assert!(
            result.is_ok(),
            "Should succeed when branch already exists locally"
        );

        // Test synchronize_branches with non-existent branch (should skip gracefully)
        let branches = vec!["feature1".to_string(), "nonexistent".to_string()];
        let result = git_ops.synchronize_branches(&branches);
        assert!(
            result.is_ok(),
            "Synchronize branches should succeed even if some branches don't exist"
        );

        Ok(())
    })
}
