use anyhow::Result;
use serde_json;
use super::{TestEnvironment, TestRunner};
use hitch::types::{HitchConfig, Environment};

/// Comprehensive tests for the init command
pub fn run_init_tests() -> Result<()> {
    let runner = TestRunner::new();

    runner.run_suite("Init Command Tests", |runner| {
        test_init_basic(runner)?;
        test_init_with_environments(runner)?;
        test_init_error_already_initialized(runner)?;
        test_init_non_git_repo(runner)?;
        test_init_dirty_working_dir(runner)?;
        test_init_hitch_metadata_structure(runner)?;
        test_init_environment_creation(runner)?;
        test_init_default_source_branch(runner)?;
        test_init_commit_and_push(runner)?;
        test_init_verbose_output(runner)?;
        test_init_no_push_flag(runner)
    })
}

/// Test basic init functionality
fn test_init_basic(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Basic init creates hitch-metadata branch", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains(&output, "✅ Hitch initialized successfully")?;

        runner.assert_true(env.branch_exists("hitch-metadata"), "hitch-metadata branch should exist");
        runner.assert_true(env.current_branch() == "hitch-metadata", "Should be on hitch-metadata branch");

        Ok(())
    })
}

/// Test init with --environments flag
fn test_init_with_environments(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init with --environments creates specified environments", || {
        let output = env.run_hitch(&["init", "--environments", "dev,qa,staging"])?;
        runner.assert_contains(&output, "Created environments: dev, qa, staging")?;

        // Verify hitch.json was created with the environments
        let config_content = env.read_file("hitch.json")?;
        let config: HitchConfig = serde_json::from_str(&config_content)?;

        runner.assert_eq(config.environments.len(), 3, "Should have 3 environments");
        runner.assert_true(config.environment_exists("dev"), "dev environment should exist");
        runner.assert_true(config.environment_exists("qa"), "qa environment should exist");
        runner.assert_true(config.environment_exists("staging"), "staging environment should exist");

        // Verify default source branch
        for env_name in ["dev", "qa", "staging"] {
            let env = config.get_environment(env_name).unwrap();
            runner.assert_eq(&env.source, "main", "Source should default to 'main'");
        }

        Ok(())
    })
}

/// Test init fails when already initialized
fn test_init_error_already_initialized(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    // First init should succeed
    env.run_hitch(&["init"])?;

    runner.test("Init fails when already initialized", || {
        let output = env.run_hitch(&["init"]);
        runner.assert_contains(&output, "Hitch is already initialized")?;
        Ok(())
    })
}

/// Test init fails in non-git repository
fn test_init_non_git_repo(runner: &TestRunner) -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let non_git_path = temp_dir.path();

    runner.test("Init fails in non-git repository", || {
        // Change to non-git directory and run hitch
        let hitch_path = std::env::current_dir()?
            .join("target")
            .join("debug")
            .join("hitch");

        let output = std::process::Command::new(&hitch_path.display().to_string())
            .args(&["init"])
            .current_dir(non_git_path)
            .output();

        let stderr = String::from_utf8_lossy(&output.unwrap_err().stderr);
        runner.assert_contains(&stderr, "Not in a Git repository")?;
        Ok(())
    })
}

/// Test init fails with dirty working directory
fn test_init_dirty_working_dir(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    // Create uncommitted changes
    env.write_file("dirty.txt", "uncommitted changes")?;

    runner.test("Init fails with dirty working directory", || {
        let output = env.run_hitch(&["init"]);
        runner.assert_contains(&output, "Working tree is not clean")?;
        Ok(())
    })
}

/// Test hitch-metadata branch structure
fn test_init_hitch_metadata_structure(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init creates proper hitch-metadata structure", || {
        env.run_hitch(&["init"])?;

        // Switch to hitch-metadata branch to check structure
        env.checkout_branch("hitch-metadata")?;

        runner.assert_true(env.file_exists("hitch.json"), "hitch.json should exist");
        runner.assert_true(env.file_exists(".gitignore"), ".gitignore should exist");

        let gitignore = env.read_file(".gitignore")?;
        runner.assert_contains(&gitignore, "*", "Should ignore all files");
        runner.assert_contains(&gitignore, "!.gitignore", "Should keep .gitignore");
        runner.assert_contains(&gitignore, "!hitch.json", "Should keep hitch.json");

        Ok(())
    })
}

/// Test environment creation with proper metadata
fn test_init_environment_creation(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init creates environments with proper metadata fields", || {
        env.run_hitch(&["init", "--environments", "dev"])?;

        let config_content = env.read_file("hitch.json")?;
        let config: HitchConfig = serde_json::from_str(&config_content)?;
        let dev_env = config.get_environment("dev").unwrap();

        runner.assert_eq(&dev_env.name, "dev", "Environment name should be correct");
        runner.assert_eq(&dev_env.source, "main", "Source should default to main");
        runner.assert_eq!(dev_env.branches.len(), 0, "Should start with no branches");
        runner.assert!(!dev_env.locked, "Should start unlocked");
        runner.assert!(dev_env.locked_by.is_none(), "Should have no locked_by");
        runner.assert!(dev_env.locked_at.is_none(), "Should have no locked_at");
        runner.assert!(dev_env.rebuilt_at.is_none(), "Should have no rebuilt_at");

        Ok(())
    })
}

/// Test default source branch behavior
fn test_init_default_source_branch(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init defaults source to main when not specified", || {
        env.run_hitch(&["init", "--environments", "dev"])?;

        let config_content = env.read_file("hitch.json")?;
        let config: HitchConfig = serde_json::from_str(&config_content)?;
        let dev_env = config.get_environment("dev").unwrap();

        runner.assert_eq(&dev_env.source, "main", "Source should default to 'main'");
        Ok(())
    })
}

/// Test commit and push functionality
fn test_init_commit_and_push(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init commits changes to hitch-metadata", || {
        env.run_hitch(&["init"])?;

        // Check if commit was created
        let log_output = env.log(&["--oneline", "hitch-metadata"]);
        runner.assert_contains(&log_output, "Initialize Hitch metadata")?;

        Ok(())
    })
}

/// Test verbose output
fn test_init_verbose_output(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init provides verbose output with --verbose flag", || {
        let output = env.run_hitch(&["init", "--verbose"])?;
        runner.assert_contains(&output, "Running pre-check validation")?;
        runner.assert_contains(&output, "Git repository validation passed")?;
        runner.assert_contains(&output, "Working tree is clean")?;
        runner.assert_contains(&output, "✓ hitch-metadata branch does not exist")?;

        Ok(())
    })
}

/// Test --no-push flag functionality
fn test_init_no_push_flag(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init skips push with --no-push flag", || {
        let output = env.run_hitch(&["init", "--no-push"])?;
        runner.assert_contains(&output, "Skipping push due to --no-push flag")?;

        // Should still commit locally
        let log_output = env.log(&["--oneline", "hitch-metadata"]);
        runner.assert_contains(&log_output, "Initialize Hitch metadata")?;

        Ok(())
    })
}