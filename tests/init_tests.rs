use anyhow::Result;
use std::process::Command;

mod common;
use common::{TestEnvironment, TestRunner};

fn main() -> Result<()> {
    println!("🚀 Running Hitch init command tests...");

    let runner = TestRunner::new();

    runner.run_suite("Init Command Tests", |runner| {
        test_init_basic(runner)?;
        test_init_with_environments(runner)?;
        test_init_error_already_initialized(runner)?;
        test_init_verbose_output(runner)?;
        test_init_no_push_flag(runner)
    })?;

    println!("\n🎉 All init command tests passed!");
    Ok(())
}

/// Test basic init functionality
fn test_init_basic(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Basic init creates hitch-metadata branch", || {
        let output = env.run_hitch(&["init"])?;
        runner.assert_contains_simple(&output, "✅ Hitch initialized successfully")?;

        runner.assert_true(env.branch_exists("hitch-metadata"), "hitch-metadata branch should exist");
        runner.assert_true(env.current_branch()? == "hitch-metadata", "Should be on hitch-metadata branch");

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
        runner.assert_contains_simple(&config_content, "\"staging")?;

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

/// Test verbose output
fn test_init_verbose_output(runner: &TestRunner) -> Result<()> {
    let env = TestEnvironment::new()?;

    runner.test("Init provides verbose output with --verbose flag", || {
        let output = env.run_hitch(&["init", "--verbose"])?;
        runner.assert_contains_simple(&output, "Running pre-check validation")?;
        runner.assert_contains_simple(&output, "Git repository validation passed")?;
        runner.assert_contains_simple(&output, "Working tree is clean")?;

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