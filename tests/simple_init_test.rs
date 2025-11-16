use anyhow::Result;

mod common;
use common::{with_test_env, SetupLevel};

#[test]
fn test_basic_init() -> Result<()> {
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

        // Test that we can verify the setup worked correctly
        std::env::set_current_dir(test_env.path())?;

        // Check that hitch-metadata branch exists (from setup_complete_hitch_env)
        assert!(
            git_ops.branch_exists("hitch-metadata")?,
            "hitch-metadata branch should exist"
        );

        // Check that .gitignore and hitch.json files exist in hitch-metadata branch
        git_ops.checkout_branch("hitch-metadata")?;

        assert!(
            test_env.path().join(".gitignore").exists(),
            ".gitignore should exist in hitch-metadata branch"
        );

        assert!(
            test_env.path().join("hitch.json").exists(),
            "hitch.json should exist in hitch-metadata branch"
        );

        Ok(())
    })
}
