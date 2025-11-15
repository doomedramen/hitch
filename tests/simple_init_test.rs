use anyhow::Result;
use std::process::Command;

mod common;
use common::TestEnv;

#[test]
fn test_basic_init() -> Result<()> {
    // Use the git2-based TestEnv framework for complete isolation
    let test_env = TestEnv::new()?;
    test_env.setup_complete_hitch_env()?;

    // Test that we can verify the setup worked correctly
    std::env::set_current_dir(test_env.path())?;

    // Check that hitch-metadata branch exists (from setup_complete_hitch_env)
    let branch_output = Command::new("git").args(&["branch"]).output()?;

    let branches = String::from_utf8(branch_output.stdout)?;
    assert!(
        branches.contains("hitch-metadata"),
        "hitch-metadata branch should exist. Got branches: {}",
        branches
    );

    // Check that .gitignore and hitch.json files exist in hitch-metadata branch
    Command::new("git")
        .args(&["checkout", "hitch-metadata"])
        .output()?;

    assert!(
        test_env.path().join(".gitignore").exists(),
        ".gitignore should exist in hitch-metadata branch"
    );

    assert!(
        test_env.path().join("hitch.json").exists(),
        "hitch.json should exist in hitch-metadata branch"
    );

    Ok(())
}
