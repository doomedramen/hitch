//! Integration tests for hitch tree command

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_tree_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to get tree without initializing hitch
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_empty_configuration() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Get tree with empty configuration
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No environments configured")
                .assert_stdout_contains(
                    "Use 'hitch add <environment>' to create your first environment",
                );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_single_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add a single environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("main")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("base:")
                .assert_stdout_contains("[env]");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_multiple_environments_same_base() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add multiple environments with same base
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("main")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("qa")
                .assert_stdout_contains("production")
                .assert_stdout_contains("[env]");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_nested_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create nested environment structure
            // dev based on main
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create the dev branch in git (needed for it to be used as base)
            env.git.run(&["checkout", "-b", "dev"])?;

            // b2b-dev based on dev
            env.hitch
                .run()
                .args(&["add", "b2b-dev", "--base", "dev"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("main")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("b2b-dev")
                .assert_stdout_contains("base:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_complex_hierarchy() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create a complex hierarchy like the user's scenario:
            // main
            // ├── dev
            // │   ├── b2b-dev
            // │   └── backoffice-dev
            // ├── b2b-prod
            // └── backoffice-prod

            // First level environments based on main
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "b2b-prod"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "backoffice-prod"])
                .execute()?
                .assert_success();

            // Create the dev branch in git (needed for it to be used as base)
            env.git.run(&["checkout", "-b", "dev"])?;

            // Second level environments based on dev
            env.hitch
                .run()
                .args(&["add", "b2b-dev", "--base", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "backoffice-dev", "--base", "dev"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("main")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("b2b-dev")
                .assert_stdout_contains("backoffice-dev")
                .assert_stdout_contains("b2b-prod")
                .assert_stdout_contains("backoffice-prod");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_with_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup: create environment with promoted branches
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote a feature branch
            env.git.run(&["checkout", "-b", "feature-auth"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-auth", "dev"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("promoted")
                .assert_stdout_contains("feature-auth");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_with_locked_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup: create and lock environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "production"])
                .execute()?
                .assert_success();

            // Get tree
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("production")
                .assert_stdout_contains("[LOCKED]");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_with_base_branch_indicator() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup: create nested environments where dev is both an environment
            // and a base for other environments
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create the dev branch in git (needed for it to be used as base)
            env.git.run(&["checkout", "-b", "dev"])?;

            env.hitch
                .run()
                .args(&["add", "staging", "--base", "dev"])
                .execute()?
                .assert_success();

            // Promote a branch to dev
            env.git.run(&["checkout", "-b", "feature-api"])?;
            env.hitch
                .run()
                .args(&["promote", "feature-api", "dev"])
                .execute()?
                .assert_success();

            // Get tree - staging should appear under dev showing the hierarchy
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("staging")
                .assert_stdout_contains("base:");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_verbose_mode() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Get tree with verbose flag
            let result = env.hitch.run().args(&["tree", "--verbose"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("Starting tree command")
                .assert_stdout_contains("Tree command completed");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_tree_multiple_root_bases() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create the develop branch first (needed for it to be used as base)
            env.git.run(&["checkout", "-b", "develop"])?;

            // Create environments with different base branches
            env.hitch
                .run()
                .args(&["add", "dev", "--base", "develop"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "production", "--base", "main"])
                .execute()?
                .assert_success();

            // Get tree - should show both main and develop as roots
            let result = env.hitch.run().args(&["tree"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("Branch Hierarchy")
                .assert_stdout_contains("develop")
                .assert_stdout_contains("main")
                .assert_stdout_contains("dev")
                .assert_stdout_contains("production");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
