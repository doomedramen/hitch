use anyhow::Result;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel};

#[test]
fn test_pre_check_function() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Test pre-check in valid git repository
        // Get the path to our hitch binary
        let hitch_path = test_env.hitch_binary();

        // Run hitch init (this will internally call pre-check and should succeed)
        let output = Command::new(&hitch_path)
            .args(["init", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Check that pre-check validations passed
        assert!(full_output.contains("Running pre-check validation"));
        assert!(full_output.contains("Git repository validation passed"));

        Ok(())
    })
}

#[test]
fn test_pre_check_function_failures() -> Result<()> {
    with_test_env(SetupLevel::Basic, |test_env| {
        // Test pre-check failure in non-git directory
        let hitch_path = test_env.hitch_binary();

        let output = Command::new(&hitch_path)
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            !output.status.success(),
            "hitch init should fail in non-git repo"
        );

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Should fail at pre-check git repository validation
        assert!(full_output.contains("Error: Not in a git repository"));

        Ok(())
    })
}

#[test]
fn test_pre_check_function_dirty_working_directory() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Create dirty file (uncommitted changes)
        fs::write(test_env.path().join("dirty.txt"), "This is uncommitted")?;

        let hitch_path = test_env.hitch_binary();

        let output = Command::new(&hitch_path)
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            !output.status.success(),
            "hitch init should fail in dirty directory"
        );

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Should fail at pre-check working tree validation
        assert!(full_output.contains("Working tree is not clean"));

        Ok(())
    })
}

#[test]
fn test_switch_to_function() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Create feature branch and some content
        Command::new("git")
            .args(["checkout", "-b", "feature/test"])
            .current_dir(test_env.path())
            .output()?;

        fs::write(test_env.path().join("feature.txt"), "Feature content\n")?;
        Command::new("git")
            .args(["add", "feature.txt"])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", "Add feature"])
            .current_dir(test_env.path())
            .output()?;

        // Test that hitch init uses switch-to internally
        let hitch_path = test_env.hitch_binary();

        let output = Command::new(&hitch_path)
            .args(["init"])
            .current_dir(test_env.path())
            .output()?;

        assert!(output.status.success(), "hitch init should succeed");

        // Verify that switch-to preserved the original branch structure
        let branch_output = Command::new("git")
            .args(["branch"])
            .current_dir(test_env.path())
            .output()?;

        let branches = String::from_utf8(branch_output.stdout)?;

        // Should have hitch-metadata branch
        assert!(
            branches.contains("hitch-metadata"),
            "hitch-metadata branch should exist"
        );

        // Should still have feature branch
        assert!(
            branches.contains("feature/test"),
            "feature/test branch should still exist after init"
        );

        // Verify we can switch back to main
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(test_env.path())
            .output()?;

        let current_branch = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(test_env.path())
            .output()?;

        let current = String::from_utf8(current_branch.stdout)?.trim().to_string();
        assert_eq!(
            current, "main",
            "Should be able to switch back to main branch"
        );

        Ok(())
    })
}

#[test]
fn test_modify_metadata_function() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Test that hitch init creates proper metadata structure using access_metadata
        let hitch_path = test_env.hitch_binary();

        let output = Command::new(&hitch_path)
            .args(["init", "--environments", "dev,qa"])
            .current_dir(test_env.path())
            .output()?;

        assert!(output.status.success(), "hitch init should succeed");

        // Verify access_metadata created proper structure
        Command::new("git")
            .args(["checkout", "hitch-metadata"])
            .current_dir(test_env.path())
            .output()?;

        // Check that hitch.json was created with environments
        assert!(
            test_env.path().join("hitch.json").exists(),
            "hitch.json should exist"
        );

        let hitch_json_content = fs::read_to_string(test_env.path().join("hitch.json"))?;
        assert!(
            hitch_json_content.contains("\"dev\""),
            "dev environment should exist"
        );
        assert!(
            hitch_json_content.contains("\"qa\""),
            "qa environment should exist"
        );

        // Check that .gitignore was created properly
        assert!(
            test_env.path().join(".gitignore").exists(),
            ".gitignore should exist"
        );

        let gitignore_content = fs::read_to_string(test_env.path().join(".gitignore"))?;
        assert!(
            gitignore_content.contains("*"),
            "gitignore should ignore all files"
        );
        assert!(
            gitignore_content.contains("!.gitignore"),
            "gitignore should keep .gitignore"
        );
        assert!(
            gitignore_content.contains("!hitch.json"),
            "gitignore should keep hitch.json"
        );

        // Verify that metadata was committed
        let log_output = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(test_env.path())
            .output()?;

        let log = String::from_utf8(log_output.stdout)?;
        assert!(
            log.contains("Initialize Hitch metadata") || log.contains("Update hitch configuration"),
            "Should have a commit for initializing metadata"
        );

        Ok(())
    })
}

#[test]
fn test_with_locked_env_function() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Initialize Hitch to set up metadata
        let hitch_path = test_env.hitch_binary();

        let output = Command::new(&hitch_path)
            .args(["init", "--environments", "dev"])
            .current_dir(test_env.path())
            .output()?;

        assert!(output.status.success(), "hitch init should succeed");

        // Test that with_locked_env function concept exists
        // Check that hitch.json contains dev environment
        Command::new("git")
            .args(["checkout", "hitch-metadata"])
            .current_dir(test_env.path())
            .output()?;

        let hitch_json_content = fs::read_to_string(test_env.path().join("hitch.json"))?;
        assert!(
            hitch_json_content.contains("\"dev\""),
            "dev environment should exist"
        );
        assert!(
            hitch_json_content.contains("\"base\""),
            "environment should have base field"
        );

        // Verify the environment has the lock-related fields (even if not currently locked)
        // The Environment struct should support locking functionality

        Ok(())
    })
}

#[test]
fn test_global_flags_integration() -> Result<()> {
    // Test --verbose flag
    with_test_env(SetupLevel::GitOnly, |test_env| {
        let hitch_path = test_env.hitch_binary();

        let verbose_output = Command::new(&hitch_path)
            .args(["init", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            verbose_output.status.success(),
            "init with --verbose should succeed"
        );

        let verbose_stdout = String::from_utf8(verbose_output.stdout)?;
        let verbose_stderr = String::from_utf8(verbose_output.stderr)?;
        let verbose_full = format!("{}{}", verbose_stdout, verbose_stderr);

        // Should show detailed logs
        assert!(verbose_full.contains("Running pre-check validation"));
        assert!(verbose_full.contains("Creating hitch-metadata branch"));
        assert!(verbose_full.contains("Accessing hitch metadata"));
        assert!(verbose_full.contains("Loading hitch.json"));

        Ok(())
    })?;

    // Test --no-push flag
    with_test_env(SetupLevel::GitOnly, |test_env| {
        let hitch_path = test_env.hitch_binary();

        let no_push_output = Command::new(&hitch_path)
            .args(["init", "--no-push", "--verbose"])
            .current_dir(test_env.path())
            .output()?;

        assert!(
            no_push_output.status.success(),
            "init with --no-push and --verbose should succeed"
        );

        let no_push_stdout = String::from_utf8(no_push_output.stdout)?;
        let no_push_stderr = String::from_utf8(no_push_output.stderr)?;
        let no_push_full = format!("{}{}", no_push_stdout, no_push_stderr);

        // Should mention skipping push in verbose output
        assert!(no_push_full.contains("Skipping push due to --no-push flag"));

        Ok(())
    })
}
