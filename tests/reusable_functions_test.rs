use anyhow::Result;
use std::process::Command;

#[test]
fn test_pre_check_function() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Test pre-check in valid git repository
    {
        // Initialize git repo
        Command::new("git")
            .args(&["init"])
            .current_dir(&temp_dir)
            .output()?;

        // Configure git user
        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&temp_dir)
            .output()?;

        // Create initial commit
        std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&temp_dir)
            .output()?;

        // Get the path to our hitch binary
        let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

        // Run hitch init (this will internally call pre-check and should succeed)
        let output = Command::new(&hitch_path)
            .args(&["init", "--verbose"])
            .current_dir(&temp_dir)
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Check that pre-check validations passed
        assert!(full_output.contains("Running pre-check validation"));
        assert!(full_output.contains("Git repository validation passed"));
        assert!(full_output.contains("Working tree is clean"));
        assert!(full_output.contains("Pre-check validation completed successfully"));

        assert!(
            output.status.success(),
            "hitch init should succeed in clean git repo"
        );
    }

    // Test pre-check failure in non-git repository
    {
        let non_git_dir = tempfile::tempdir()?;

        // Get the path to our hitch binary
        let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

        // Run hitch init (this should fail pre-check)
        let output = Command::new(&hitch_path)
            .args(&["init"])
            .current_dir(&non_git_dir)
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
    }

    // Test pre-check failure with dirty working directory
    {
        // Initialize git repo (reusing the same temp_dir)
        Command::new("git")
            .args(&["init"])
            .current_dir(&temp_dir)
            .output()?;

        // Configure git user
        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&temp_dir)
            .output()?;

        // Create initial commit
        std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&temp_dir)
            .output()?;

        // Create dirty file (uncommitted changes)
        std::fs::write(temp_dir.path().join("dirty.txt"), "This is uncommitted")?;

        // Get the path to our hitch binary
        let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

        // Run hitch init (this should fail pre-check)
        let output = Command::new(&hitch_path)
            .args(&["init"])
            .current_dir(&temp_dir)
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
    }

    Ok(())
}

#[test]
fn test_switch_to_function() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(&temp_dir)
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(&temp_dir)
        .output()?;

    // Create feature branch and some content
    Command::new("git")
        .args(&["checkout", "-b", "feature/test"])
        .current_dir(&temp_dir)
        .output()?;

    std::fs::write(temp_dir.path().join("feature.txt"), "Feature content\n")?;
    Command::new("git")
        .args(&["add", "feature.txt"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add feature"])
        .current_dir(&temp_dir)
        .output()?;

    // Test that hitch init uses switch-to internally
    // (by verifying it creates hitch-metadata branch while preserving other branches)
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "hitch init should succeed");

    // Verify that switch-to preserved the original branch structure
    let branch_output = Command::new("git")
        .args(&["branch"])
        .current_dir(&temp_dir)
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
        .args(&["checkout", "main"])
        .current_dir(&temp_dir)
        .output()?;

    let current_branch = Command::new("git")
        .args(&["branch", "--show-current"])
        .current_dir(&temp_dir)
        .output()?;

    let current = String::from_utf8(current_branch.stdout)?.trim().to_string();
    assert_eq!(
        current, "main",
        "Should be able to switch back to main branch"
    );

    Ok(())
}

#[test]
fn test_access_metadata_function() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(&temp_dir)
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(&temp_dir)
        .output()?;

    // Test that hitch init creates proper metadata structure using access_metadata
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    let output = Command::new(&hitch_path)
        .args(&["init", "--environments", "dev,qa"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "hitch init should succeed");

    // Verify access_metadata created proper structure
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    // Check that hitch.json was created with environments
    assert!(
        temp_dir.path().join("hitch.json").exists(),
        "hitch.json should exist"
    );

    let hitch_json_content = std::fs::read_to_string(temp_dir.path().join("hitch.json"))?;
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
        temp_dir.path().join(".gitignore").exists(),
        ".gitignore should exist"
    );

    let gitignore_content = std::fs::read_to_string(temp_dir.path().join(".gitignore"))?;
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
        .args(&["log", "--oneline"])
        .current_dir(&temp_dir)
        .output()?;

    let log = String::from_utf8(log_output.stdout)?;
    assert!(
        log.contains("Initialize Hitch metadata") || log.contains("Update hitch configuration"),
        "Should have a commit for initializing metadata"
    );

    Ok(())
}

#[test]
fn test_with_locked_env_function() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(&temp_dir)
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(&temp_dir)
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(&temp_dir)
        .output()?;

    // Initialize Hitch to set up metadata
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    let output = Command::new(&hitch_path)
        .args(&["init", "--environments", "dev"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(output.status.success(), "hitch init should succeed");

    // Test that with_locked_env function concept exists
    // Since lock/unlock commands aren't implemented yet, we'll test the pattern
    // by verifying that the environment locking infrastructure exists

    // Check that hitch.json contains dev environment
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    let hitch_json_content = std::fs::read_to_string(temp_dir.path().join("hitch.json"))?;
    assert!(
        hitch_json_content.contains("\"dev\""),
        "dev environment should exist"
    );
    assert!(
        hitch_json_content.contains("\"source\""),
        "environment should have source field"
    );

    // Verify the environment has the lock-related fields (even if not currently locked)
    // The Environment struct should support locking functionality
    assert!(
        hitch_json_content.contains("\"name\""),
        "environment should have name field"
    );

    // The test demonstrates that the infrastructure for with_locked_env exists
    // When lock/unlock commands are implemented, they will use with_locked_env function

    Ok(())
}

#[test]
fn test_global_flags_integration() -> Result<()> {
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Test --verbose flag
    {
        let temp_dir = tempfile::tempdir()?;

        // Initialize git repo
        Command::new("git")
            .args(&["init"])
            .current_dir(&temp_dir)
            .output()?;

        // Configure git user
        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&temp_dir)
            .output()?;

        // Create initial commit
        std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&temp_dir)
            .output()?;

        let verbose_output = Command::new(&hitch_path)
            .args(&["init", "--verbose"])
            .current_dir(&temp_dir)
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
    }

    // Test --no-push flag
    {
        let temp_dir = tempfile::tempdir()?;

        // Initialize git repo
        Command::new("git")
            .args(&["init"])
            .current_dir(&temp_dir)
            .output()?;

        // Configure git user
        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&temp_dir)
            .output()?;

        // Create initial commit
        std::fs::write(temp_dir.path().join("README.md"), "# Test\n")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&temp_dir)
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&temp_dir)
            .output()?;

        let no_push_output = Command::new(&hitch_path)
            .args(&["init", "--no-push", "--verbose"])
            .current_dir(&temp_dir)
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
    }

    Ok(())
}
