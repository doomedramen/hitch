use anyhow::Result;
use std::process::Command;

mod common;
use common::{TestEnvironment, TestRunner};

fn main() -> Result<()> {
    println!("🚀 Running Hitch init command tests...");

    let runner = TestRunner::new();

    runner.run_suite("Init Command Tests", |runner| {
        // Basic functionality tests
        test_init_basic(runner)?;
        test_init_with_environments(runner)?;
        test_init_with_base_structure(runner)?;

        // Error handling tests
        test_init_error_already_initialized(runner)?;
        test_init_non_git_repo(runner)?;
        test_init_dirty_working_dir(runner)?;

        // Flag tests
        test_init_verbose_output(runner)?;
        test_init_no_push_flag(runner)?;
        test_init_verbose_with_environments(runner)?;

        // Metadata structure tests
        test_init_hitch_metadata_structure(runner)?;
        test_init_gitignore_structure(runner)
    })?;

    println!("\n🎉 All init command tests passed!");
    Ok(())
}

/// Test basic init functionality in a clean repository
fn test_init_basic(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Basic init creates hitch-metadata branch", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "✅ Hitch initialized successfully")?;

        runner.assert_true(env.branch_exists("hitch-metadata"), "hitch-metadata branch should exist");
        runner.assert_true(env.current_branch()? == "hitch-metadata", "Should be on hitch-metadata branch")?;

        Ok(())
    })
}

/// Test init with --environments flag
fn test_init_with_environments(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init with --environments creates specified environments", || {
        let output = env.run_hitch(&["init", "--environments", "dev,qa,staging"])?;
        runner.assert_contains_simple(&output, "Created environments: dev, qa, staging")?;

        // Verify hitch.json was created with the environments
        let config_content = env.read_file("hitch.json")?;
        runner.assert_contains_simple(&config_content, "\"dev\"")?;
        runner.assert_contains_simple(&config_content, "\"qa\"")?;
        runner.assert_contains_simple(&config_content, "\"staging\"")?;

        Ok(())
    })
}

/// Test init in a repository with existing branch structure
fn test_init_with_base_structure(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new_with_base(true)?;

    runner.test("Init works correctly with existing branch structure", || {
        // Verify base structure exists
        runner.assert_true(env.branch_exists("feature/login"), "feature/login branch should exist in base structure");
        runner.assert_true(env.branch_exists("dev"), "dev branch should exist in base structure");
        runner.assert_true(env.branch_exists("staging"), "staging branch should exist in base structure");

        let output = env.run_hitch(&["init", "--environments", "dev,staging"])?;
        runner.assert_contains_simple(&output, "✅ Hitch initialized successfully")?;

        // Verify original branches are still there
        runner.assert_true(env.branch_exists("feature/login"), "feature/login branch should still exist after init");
        runner.assert_true(env.branch_exists("dev"), "dev branch should still exist after init");

        Ok(())
    })
}

/// Test init fails when already initialized
fn test_init_error_already_initialized(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    // First init should succeed
    env.run_hitch(&["init"])?;

    runner.test("Init fails when already initialized", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "Hitch is already initialized")?;
        Ok(())
    })
}

/// Test init fails in non-git repository
fn test_init_non_git_repo(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new_non_git()?;

    runner.test("Init fails in non-git repository", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "Not in a Git repository")?;
        Ok(())
    })
}

/// Test init fails with dirty working directory
fn test_init_dirty_working_dir(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    // Create uncommitted changes
    env.write_file("dirty.txt", "uncommitted changes")?;

    runner.test("Init fails with dirty working directory", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "Working tree is not clean")?;
        Ok(())
    })
}

/// Test verbose output
fn test_init_verbose_output(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init provides verbose output with --verbose flag", || {
        let output = env.run_hitch(&["init", "--verbose"])?;
        runner.assert_contains_simple(&output, "Running pre-check validation")?;
        runner.assert_contains_simple(&output, "Git repository validation passed")?;
        runner.assert_contains_simple(&output, "Working tree is clean")?;
        runner.assert_contains_simple(&output, "✓ hitch-metadata branch does not exist")?;

        Ok(())
    })
}

/// Test --no-push flag functionality
fn test_init_no_push_flag(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init skips push with --no-push flag", || {
        let output = env.run_hitch(&["init", "--no-push"])?;
        runner.assert_contains_simple(&output, "Skipping push due to --no-push flag")?;

        // Should still commit locally
        let log_output = env.log(&["--oneline", "hitch-metadata"])?;
        runner.assert_contains_simple(&log_output, "Initialize Hitch metadata")?;

        Ok(())
    })
}

/// Test verbose output with environments
fn test_init_verbose_with_environments(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init verbose output with environments shows creation steps", || {
        let output = env.run_hitch(&["init", "--verbose", "--environments", "dev,qa"])?;
        runner.assert_contains_simple(&output, "Creating 2 environment(s): dev, qa")?;
        runner.assert_contains_simple(&output, "✓ Configuration skeleton created")?;

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
        runner.assert_contains_simple(&gitignore, "*")?;
        runner.assert_contains_simple(&gitignore, "!.gitignore")?;
        runner.assert_contains_simple(&gitignore, "!hitch.json")?;

        Ok(())
    })
}

/// Test .gitignore structure specifically
fn test_init_gitignore_structure(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init creates proper .gitignore content", || {
        env.run_hitch(&["init"])?;

        env.checkout_branch("hitch-metadata")?;
        let gitignore = env.read_file(".gitignore")?;

        // Verify exact .gitignore content
        let expected_lines = vec!["*", "!.gitignore", "!hitch.json"];
        for line in expected_lines {
            runner.assert_contains_simple(&gitignore, line)?;
        }

        Ok(())
    })
}