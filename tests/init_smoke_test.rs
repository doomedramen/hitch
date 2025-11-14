use anyhow::Result;
use std::process::Command;

#[test]
fn test_init_smoke_test() -> Result<()> {
    // Create a temporary directory for our test
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

    // Get the path to our hitch binary
    let hitch_path = format!("{}/target/debug/hitch", std::env::current_dir()?.display());

    // Run hitch init
    let output = Command::new(&hitch_path)
        .args(&["init"])
        .current_dir(&temp_dir)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    // Check that init succeeded
    assert!(output.status.success(), "hitch init should succeed");

    // Check for success message
    assert!(
        full_output.contains("Hitch initialized successfully"),
        "Output should contain success message. Got: {}",
        full_output
    );

    // Check that hitch-metadata branch exists
    let branch_output = Command::new("git")
        .args(&["branch"])
        .current_dir(&temp_dir)
        .output()?;

    let branches = String::from_utf8(branch_output.stdout)?;
    assert!(
        branches.contains("hitch-metadata"),
        "hitch-metadata branch should exist. Got branches: {}",
        branches
    );

    // Check that .gitignore and hitch.json files exist in hitch-metadata branch
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .current_dir(&temp_dir)
        .output()?;

    assert!(
        temp_dir.path().join(".gitignore").exists(),
        ".gitignore should exist in hitch-metadata branch"
    );

    assert!(
        temp_dir.path().join("hitch.json").exists(),
        "hitch.json should exist in hitch-metadata branch"
    );

    // Check .gitignore content
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

    Ok(())
}
