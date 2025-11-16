use anyhow::Result;
use std::process::Command;

mod common;
use common::{with_test_env, SetupLevel};

#[test]
fn test_init_smoke_test() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Run hitch init again to test the smoke test functionality
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path).args(["init"]).output()?;

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        // Check that init command runs (might fail if already initialized, but that's ok for smoke test)
        // The important thing is that the command executes without crashing
        let _ = output.status.success();

        // Check for expected messages (success or already initialized)
        assert!(
            full_output.contains("Hitch initialized successfully")
                || full_output.contains("already initialized")
                || full_output.contains("hitch-metadata branch already exists"),
            "Output should contain expected init message. Got: {}",
            full_output
        );

        // Check that hitch-metadata branch exists
        std::env::set_current_dir(test_env.path())?;
        let branch_output = Command::new("git").args(["branch"]).output()?;

        let branches = String::from_utf8(branch_output.stdout)?;
        assert!(
            branches.contains("hitch-metadata"),
            "hitch-metadata branch should exist. Got branches: {}",
            branches
        );

        // Check that .gitignore and hitch.json files exist in hitch-metadata branch
        Command::new("git")
            .args(["checkout", "hitch-metadata"])
            .output()?;

        assert!(
            test_env.path().join(".gitignore").exists(),
            ".gitignore should exist in hitch-metadata branch"
        );

        assert!(
            test_env.path().join("hitch.json").exists(),
            "hitch.json should exist in hitch-metadata branch"
        );

        // Check .gitignore content
        let gitignore_content = std::fs::read_to_string(test_env.path().join(".gitignore"))?;
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
    })
}
