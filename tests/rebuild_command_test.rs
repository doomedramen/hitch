use anyhow::Result;
use chrono::Utc;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

/// Simple ANSI code stripper for test assertions
fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ANSI escape sequence
            if chars.next() == Some('[') {
                // Skip until we hit the end character (a-z)
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_alphabetic() {
                        chars.next(); // consume the end character
                        break;
                    }
                    chars.next(); // consume part of the sequence
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Helper extension trait for TestEnv to provide custom methods needed by rebuild tests
trait TestEnvExt {
    fn create_hitch_config_with_environment(
        &self,
        env_name: &str,
        base_branch: &str,
        branches: &[&str],
        locked: bool,
    ) -> Result<()>;
    fn create_branch_with_content(
        &self,
        branch_name: &str,
        filename: &str,
        content: &str,
    ) -> Result<()>;
    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output>;
    fn get_current_branch(&self) -> Result<String>;
    fn has_temp_branches(&self) -> Result<bool>;
}

impl TestEnvExt for TestEnv {
    fn create_hitch_config_with_environment(
        &self,
        env_name: &str,
        base_branch: &str,
        branches: &[&str],
        locked: bool,
    ) -> Result<()> {
        use std::collections::HashMap;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        let mut environments = HashMap::new();
        let branches_vec: Vec<String> = branches.iter().map(|s| s.to_string()).collect();

        environments.insert(env_name.to_string(), serde_json::json!({
            "base": base_branch,
            "branches": branches_vec,
            "locked": locked,
            "locked_by": if locked { Some("admin@example.com".to_string()) } else { None },
            "locked_at": if locked { Some(Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()) } else { None },
            "rebuilt_at": None::<String>
        }));

        let config = serde_json::json!({
            "version": "1.0",
            "environments": environments
        });

        // Create hitch-metadata branch if it doesn't exist (orphan branch)
        let create_result = git_ops.create_orphan_branch("hitch-metadata");
        if create_result.is_err() {
            // Branch might already exist, try to checkout it
            git_ops.checkout_branch("hitch-metadata")?;
        }

        git_ops.write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        git_ops.write_file(".gitignore", "*\n!.gitignore\n!hitch.json\n")?;

        git_ops.add_and_commit(&["hitch.json", ".gitignore"], "Add hitch configuration")?;
        git_ops.checkout_branch("main")?;

        Ok(())
    }

    fn create_branch_with_content(
        &self,
        branch_name: &str,
        filename: &str,
        content: &str,
    ) -> Result<()> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Create branch
        git_ops.create_branch_from(branch_name, "main")?;

        // Write content and commit
        git_ops.write_file(filename, content)?;
        git_ops.add_and_commit(&[filename], &format!("Add {}", filename))?;

        // Return to main
        git_ops.checkout_branch("main")?;

        Ok(())
    }

    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new(self.hitch_binary())
            .args(args)
            .current_dir(self.path())
            .output()?;

        Ok(output)
    }

    fn get_current_branch(&self) -> Result<String> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;
        git_ops.get_current_branch()
    }

    fn has_temp_branches(&self) -> Result<bool> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Check for common temp branch patterns
        let temp_patterns = ["hitch-tmp-main-", "hitch-tmp-dev-", "hitch-tmp-staging-"];
        for pattern in &temp_patterns {
            if git_ops.branch_exists_anywhere(pattern)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[test]
fn test_rebuild_basic_success() -> Result<()> {
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

        // Create feature branch with content
        test_env.create_branch_with_content("feature/login", "login.md", "# Login feature\n")?;

        // Initialize hitch with dev environment
        test_env.create_hitch_config_with_environment("dev", "main", &["feature/login"], false)?;

        // Run rebuild command
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(
            output.status.success(),
            "rebuild command should succeed. Output: {}",
            full_output
        );

        // Verify rebuild process steps
        assert!(full_output.contains("Rebuilding environment 'dev'"));
        assert!(full_output.contains("Creating temporary branch"));
        assert!(full_output.contains("Merging promoted branches"));
        assert!(full_output.contains("Replacing 'dev' branch"));
        assert!(full_output.contains("Environment 'dev' rebuilt successfully"));

        // Verify dev branch exists and has the expected content
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("dev")?;

        assert!(
            test_env.path().join("login.md").exists(),
            "dev branch should have login.md from feature branch"
        );
        assert!(
            test_env.path().join("README.md").exists(),
            "dev branch should have README.md from base branch"
        );

        // Verify rebuiltAt timestamp was updated
        git_ops.checkout_branch("hitch-metadata")?;

        let updated_config = std::fs::read_to_string(test_env.path().join("hitch.json"))?;
        assert!(
            updated_config.contains("\"rebuilt_at\""),
            "rebuilt_at should be set after successful rebuild"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_empty_environment() -> Result<()> {
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

        // Initialize hitch with empty environment
        test_env.create_hitch_config_with_environment("staging", "main", &[], false)?;

        // Run rebuild command
        let output = test_env.run_hitch_command(&["rebuild", "staging", "--verbose"])?;

        assert!(
            output.status.success(),
            "rebuild should succeed with empty environment"
        );

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(full_output
            .contains("No branches promoted to this environment, using base branch only"));

        // Verify staging branch exists and matches main branch
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("staging")?;

        assert!(
            test_env.path().join("README.md").exists(),
            "staging branch should have base branch content"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_nonexistent_environment() -> Result<()> {
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

        // Initialize hitch with dev environment only
        test_env.create_hitch_config_with_environment("dev", "main", &[], false)?;

        // Try to rebuild nonexistent environment
        let output = test_env.run_hitch_command(&["rebuild", "production"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail for nonexistent environment"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Environment 'production' does not exist"));

        Ok(())
    })
}

#[test]
fn test_rebuild_locked_environment() -> Result<()> {
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

        // Initialize hitch with locked environment
        test_env.create_hitch_config_with_environment("production", "main", &[], true)?;

        // Try to rebuild locked environment (should fail)
        let output = test_env.run_hitch_command(&["rebuild", "production"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail for locked environment"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Environment 'production' is locked") || stderr.contains("locked"));
        assert!(stderr.contains("Use --force to override") || stderr.contains("--force"));

        // Try to rebuild with --force flag (should succeed)
        let force_output = test_env.run_hitch_command(&["rebuild", "production", "--force"])?;

        assert!(
            force_output.status.success(),
            "rebuild with --force should succeed for locked environment"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_missing_branch() -> Result<()> {
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

        // Initialize hitch with environment that references non-existent branch
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["nonexistent/feature"],
            false,
        )?;

        // Try to rebuild environment with missing branch
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail for missing branch"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Branch 'nonexistent/feature' does not exist"));

        Ok(())
    })
}

#[test]
fn test_rebuild_missing_base_branch() -> Result<()> {
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

        // Initialize hitch with environment that references non-existent base branch
        test_env.create_hitch_config_with_environment("dev", "nonexistent-base", &[], false)?;

        // Try to rebuild environment with missing base branch
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail for missing base branch"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Base branch 'nonexistent-base' does not exist"));

        Ok(())
    })
}

#[test]
fn test_rebuild_multiple_branches() -> Result<()> {
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

        // Create multiple feature branches
        let branches = vec!["feature/login", "feature/ui", "feature/api"];

        for branch in &branches {
            test_env.create_branch_with_content(
                branch,
                &format!("{}.md", branch.replace("/", "_")),
                &format!("# {} feature\n", branch),
            )?;
        }

        // Initialize hitch with environment that has multiple promoted branches
        let mut environments = std::collections::HashMap::new();
        environments.insert(
            "dev".to_string(),
            serde_json::json!({
                "base": "main",
                "branches": branches.iter().map(|b| b.to_string()).collect::<Vec<_>>(),
                "locked": false,
                "locked_by": None::<String>,
                "locked_at": None::<String>,
                "rebuilt_at": None::<String>
            }),
        );

        // Use the helper method to create hitch configuration
        test_env.create_hitch_config_with_environment("dev", "main", &branches, false)?;

        // Run rebuild command
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(
            output.status.success(),
            "rebuild should succeed with multiple branches. Output: {}",
            full_output
        );
        let clean_output = strip_ansi_codes(&full_output);

        // Verify all branches were processed
        for branch in &branches {
            let processing_msg = format!("Processing branch '{}'", branch);
            let merged_msg = format!("Squash merged '{}' into temp branch", branch);

            assert!(
                clean_output.contains(&processing_msg),
                "Should process branch '{}'",
                branch
            );
            assert!(
                clean_output.contains(&merged_msg),
                "Should merge branch '{}'",
                branch
            );
        }

        // Verify dev branch exists and has content from all feature branches
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("dev")?;

        assert!(
            test_env.path().join("README.md").exists(),
            "dev branch should have base branch content"
        );

        for branch in &branches {
            let filename = format!("{}.md", branch.replace("/", "_"));
            assert!(
                test_env.path().join(&filename).exists(),
                "dev branch should have content from {}",
                branch
            );
        }

        Ok(())
    })
}

#[test]
fn test_rebuild_with_git_hooks() -> Result<()> {
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

        // Create feature branch
        test_env.create_branch_with_content("feature/test", "feature.txt", "# Feature\n")?;

        // Initialize hitch with dev environment
        test_env.create_hitch_config_with_environment("dev", "main", &["feature/test"], false)?;

        // Set up a problematic pre-commit hook that would normally fail
        let hooks_dir = test_env.path().join(".git").join("hooks");
        fs::create_dir_all(&hooks_dir)?;

        let pre_commit_hook = r#"#!/bin/sh
# This hook simulates a problematic hook that might interfere with automated operations
echo "Simulating hook that could interfere with automated operations"
# We'll make this hook succeed, but test that it doesn't interfere
exit 0
"#;

        fs::write(hooks_dir.join("pre-commit"), pre_commit_hook)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(hooks_dir.join("pre-commit"))?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(hooks_dir.join("pre-commit"), perms)?;
        }

        // Run rebuild command - should succeed despite git hooks
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(
            output.status.success(),
            "rebuild should succeed with git hooks"
        );

        // Verify rebuild process completed successfully
        assert!(full_output.contains("Environment 'dev' rebuilt successfully"));

        // Verify we returned to the original branch (not stuck on temp branch)
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(current_branch, "main", "Should be back on main branch");

        // Verify dev branch exists and has the expected content
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("dev")?;

        assert!(
            test_env.path().join("README.md").exists(),
            "dev branch should have base branch content"
        );
        assert!(
            test_env.path().join("feature.txt").exists(),
            "dev branch should have feature branch content"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_not_initialized() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // NOTE: Do NOT initialize hitch - this test checks what happens when hitch is not initialized

        // Try to rebuild without hitch being initialized
        let output = test_env.run_hitch_command(&["rebuild", "dev"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail when hitch not initialized"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(
            stderr.contains("Failed to read hitch.json")
                || stderr.contains("Failed to access hitch metadata")
                || stderr.contains("Failed to checkout branch")
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_simple_merge_conflict() -> Result<()> {
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

        // Create a base file with content
        let base_content = r#"# Configuration File
version = "1.0"
database_url = "localhost:5432"
debug = false
"#;
        git_ops.write_file("config.yaml", base_content)?;
        git_ops.add_and_commit(&["config.yaml"], "Add base configuration")?;

        // Create first feature branch that modifies the file
        git_ops.create_branch_from("feature/db-settings", "main")?;
        let feature1_content = r#"# Configuration File
version = "1.1"
database_url = "prod-db.example.com:5432"
debug = false
max_connections = 100
"#;
        git_ops.write_file("config.yaml", feature1_content)?;
        git_ops.add_and_commit(&["config.yaml"], "Update database settings")?;
        git_ops.checkout_branch("main")?;

        // Create second feature branch that modifies the same file with conflicting changes
        git_ops.create_branch_from("feature/local-settings", "main")?;
        let feature2_content = r#"# Configuration File
version = "2.0"
database_url = "localhost:5432"
debug = true
cache_enabled = true
"#;
        git_ops.write_file("config.yaml", feature2_content)?;
        git_ops.add_and_commit(&["config.yaml"], "Enable debug and cache")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has both conflicting branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["feature/db-settings", "feature/local-settings"],
            false,
        )?;

        // Run rebuild command - should fail due to merge conflicts
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail when merge conflicts are detected"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Merge conflict detected"));
        assert!(stderr.contains("config.yaml"));

        // Verify helpful error message with resolution steps
        assert!(stderr.contains("To resolve this:"));
        assert!(stderr.contains("Check out"));
        assert!(stderr.contains("Resolve conflicts manually"));
        assert!(stderr.contains("Commit the resolution"));

        // Verify cleanup: we should be back on the original branch after the conflict
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after conflict"
        );

        // Verify temp branch was cleaned up
        assert!(
            !test_env.has_temp_branches()?,
            "Temp branches should be cleaned up after conflict"
        );

        // Verify working directory is clean after cleanup
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        assert!(
            git_ops.is_working_directory_clean()?,
            "Working directory should be clean after cleanup"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_multiple_file_conflicts() -> Result<()> {
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

        // Create base files
        git_ops.write_file("package.json", r#"{"name": "app", "version": "1.0.0"}"#)?;

        // Create src directory and file
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file("src/main.js", "console.log('Hello World');\n")?;
        git_ops.add_and_commit(&["package.json", "src/main.js"], "Add initial files")?;

        // Create feature branch that modifies both files
        git_ops.create_branch_from("feature/update-app", "main")?;
        // Ensure src directory exists when we switch branches
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file(
            "package.json",
            r#"{"name": "app", "version": "2.0.0", "dependencies": {}}"#,
        )?;
        git_ops.write_file(
            "src/main.js",
            "console.log('Hello Updated World');\nconst utils = require('./utils');\n",
        )?;
        git_ops.add_and_commit(
            &["package.json", "src/main.js"],
            "Update app version and main file",
        )?;
        git_ops.checkout_branch("main")?;

        // Create conflicting branch that modifies both files differently
        git_ops.create_branch_from("feature/alternative-update", "main")?;
        // Ensure src directory exists when we switch branches
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file(
            "package.json",
            r#"{"name": "my-app", "version": "1.1.0", "scripts": {}}"#,
        )?;
        git_ops.write_file(
            "src/main.js",
            "console.log('Hello Alternative World');\nconst config = require('./config');\n",
        )?;
        git_ops.add_and_commit(&["package.json", "src/main.js"], "Alternative updates")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has both conflicting branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["feature/update-app", "feature/alternative-update"],
            false,
        )?;

        // Run rebuild command - should fail due to merge conflicts
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail when multiple files have merge conflicts"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Merge conflict detected"));
        assert!(stderr.contains("package.json"));
        assert!(stderr.contains("src/main.js"));

        // Verify cleanup: we should be back on the original branch after the conflict
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after conflict"
        );

        // Verify temp branch was cleaned up
        assert!(
            !test_env.has_temp_branches()?,
            "Temp branches should be cleaned up after conflict"
        );

        // Verify working directory is clean after cleanup
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        assert!(
            git_ops.is_working_directory_clean()?,
            "Working directory should be clean after cleanup"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_conflicting_binary_files() -> Result<()> {
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

        // Create a base binary-like file (using different content to simulate binary)
        let base_binary = "BINARY\x00\x01\x02\x03\x04\x05";
        git_ops.write_file("app.bin", base_binary)?;
        git_ops.add_and_commit(&["app.bin"], "Add base binary")?;

        // Create feature branch with different binary content
        git_ops.create_branch_from("feature/update-binary", "main")?;
        let new_binary = "BINARY\x01\x02\x03\x04\x05\x06";
        git_ops.write_file("app.bin", new_binary)?;
        git_ops.add_and_commit(&["app.bin"], "Update binary content")?;
        git_ops.checkout_branch("main")?;

        // Create conflicting branch with different binary content
        git_ops.create_branch_from("feature/alternative-binary", "main")?;
        let alt_binary = "BINARY\x02\x03\x04\x05\x06\x07";
        git_ops.write_file("app.bin", alt_binary)?;
        git_ops.add_and_commit(&["app.bin"], "Alternative binary content")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has both conflicting binary branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["feature/update-binary", "feature/alternative-binary"],
            false,
        )?;

        // Run rebuild command - should fail due to merge conflicts
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail when binary files have merge conflicts"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Merge conflict detected"));

        Ok(())
    })
}

#[test]
fn test_rebuild_complex_conflict_scenario() -> Result<()> {
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

        // Create base project structure
        git_ops.write_file(
            "Cargo.toml",
            r#"[package]
name = "test-app"
version = "0.1.0"
[dependencies]
serde = "1.0"
"#,
        )?;

        // Create src directory and file
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file("src/lib.rs", "pub fn hello() { println!(\"Hello\"); }\n")?;
        git_ops.add_and_commit(&["Cargo.toml", "src/lib.rs"], "Initial project setup")?;

        // Create first conflicting feature
        git_ops.create_branch_from("feature/api-changes", "main")?;
        // Ensure src directory exists when we switch branches
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file(
            "Cargo.toml",
            r#"[package]
name = "test-app"
version = "0.2.0"
[dependencies]
serde = "1.0"
tokio = "1.0"
"#,
        )?;
        git_ops.write_file(
            "src/lib.rs",
            "pub async fn hello() { println!(\"Hello Async\"); }\n",
        )?;
        git_ops.add_and_commit(&["Cargo.toml", "src/lib.rs"], "Add async support")?;
        git_ops.checkout_branch("main")?;

        // Create second conflicting feature
        git_ops.create_branch_from("feature/web-ui", "main")?;
        // Ensure src directory exists when we switch branches
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file(
            "Cargo.toml",
            r#"[package]
name = "test-app"
version = "0.1.1"
[dependencies]
serde = "1.0"
rocket = "0.5"
"#,
        )?;
        git_ops.write_file(
            "src/lib.rs",
            "pub fn hello() { println!(\"Hello Web\"); }\npub mod routes;",
        )?;
        git_ops.add_and_commit(&["Cargo.toml", "src/lib.rs"], "Add web framework")?;
        git_ops.checkout_branch("main")?;

        // Create third conflicting feature that builds on one of the conflicting ones
        git_ops.create_branch_from("feature/api-changes", "main")?;
        git_ops.create_branch_from("feature/api-enhancements", "feature/api-changes")?;
        // Ensure src directory exists when we switch branches
        std::fs::create_dir_all(test_env.path().join("src"))?;
        git_ops.write_file(
            "src/lib.rs",
            "pub async fn hello() { println!(\"Hello Enhanced Async\"); }\npub mod api;",
        )?;
        git_ops.add_and_commit(&["src/lib.rs"], "Enhance API module")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has multiple conflicting branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &[
                "feature/api-changes",
                "feature/web-ui",
                "feature/api-enhancements",
            ],
            false,
        )?;

        // Run rebuild command - should fail due to complex merge conflicts
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        assert!(
            !output.status.success(),
            "rebuild should fail in complex conflict scenarios"
        );

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Merge conflict detected"));

        // The error should mention which specific branch caused the conflict
        // It could be any of the conflicting branches, so we check for any of them
        assert!(
            stderr.contains("feature/api-changes")
                || stderr.contains("feature/web-ui")
                || stderr.contains("feature/api-enhancements"),
            "Error should mention which branch caused the conflict"
        );

        Ok(())
    })
}

#[test]
fn test_rebuild_no_conflict_after_resolution() -> Result<()> {
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

        // Create base files
        git_ops.write_file("config.txt", "version=1.0\ndebug=false\n")?;
        git_ops.add_and_commit(&["config.txt"], "Add config")?;

        // Create non-conflicting feature branch that adds a new file
        git_ops.create_branch_from("feature/add-feature", "main")?;
        git_ops.write_file("feature.txt", "feature enabled\n")?;
        git_ops.add_and_commit(&["feature.txt"], "Add feature file")?;
        git_ops.checkout_branch("main")?;

        // Create another non-conflicting feature branch that modifies a different file
        git_ops.create_branch_from("feature/update-version", "main")?;
        git_ops.write_file("config.txt", "version=1.1\ndebug=false\n")?;
        git_ops.add_and_commit(&["config.txt"], "Update version")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has non-conflicting branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["feature/add-feature", "feature/update-version"],
            false,
        )?;

        // Run rebuild command - should succeed because there are no actual conflicts
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        if !output.status.success() {
            let stdout = String::from_utf8(output.stdout.clone())?;
            let stderr = String::from_utf8(output.stderr)?;
            let full_output = format!("{}{}", stdout, stderr);
            panic!(
                "rebuild should succeed when branches don't actually conflict. Output: {}",
                full_output
            );
        }

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(full_output.contains("Environment 'dev' rebuilt successfully"));

        // Verify dev branch exists and has combined content
        git_ops.checkout_branch("dev")?;

        // Check config.txt has the updated version
        let config_content = std::fs::read_to_string(test_env.path().join("config.txt"))?;
        assert!(config_content.contains("version=1.1"));
        assert!(config_content.contains("debug=false"));

        // Check feature.txt from the other branch exists
        assert!(
            test_env.path().join("feature.txt").exists(),
            "dev branch should have feature.txt from feature/add-feature branch"
        );

        let feature_content = std::fs::read_to_string(test_env.path().join("feature.txt"))?;
        assert!(feature_content.contains("feature enabled"));

        Ok(())
    })
}

#[test]
fn test_rebuild_partial_conflict_with_multiple_branches() -> Result<()> {
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

        // Create base files
        git_ops.write_file("README.md", "# Project\nInitial description.\n")?;
        git_ops.write_file("utils.js", "function helper() { return 'old'; }\n")?;
        git_ops.add_and_commit(&["README.md", "utils.js"], "Add base files")?;

        // Create non-conflicting feature branch (modifies README only)
        git_ops.create_branch_from("feature/add-docs", "main")?;
        git_ops.write_file(
            "README.md",
            "# Project\nInitial description.\n\n## Usage\nThis is how to use it.\n",
        )?;
        git_ops.add_and_commit(&["README.md"], "Add documentation")?;
        git_ops.checkout_branch("main")?;

        // Create conflicting feature branch (modifies both README and utils to create conflict with first branch)
        git_ops.create_branch_from("feature/conflicting-utils", "main")?;
        git_ops.write_file("README.md", "# Project\nUpdated description.\n")?; // This will conflict with feature/add-docs
        git_ops.write_file(
            "utils.js",
            "function helper() { return 'new'; }\nfunction another() { return 'test'; }\n",
        )?;
        git_ops.add_and_commit(&["README.md", "utils.js"], "Update utils and README")?;
        git_ops.checkout_branch("main")?;

        // Initialize hitch with environment that has both non-conflicting and conflicting branches
        test_env.create_hitch_config_with_environment(
            "dev",
            "main",
            &["feature/add-docs", "feature/conflicting-utils"],
            false,
        )?;

        // Run rebuild command - should fail because of the conflict in utils.js
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        if output.status.success() {
            let stdout = String::from_utf8(output.stdout.clone())?;
            let stderr = String::from_utf8(output.stderr)?;
            let full_output = format!("{}{}", stdout, stderr);
            panic!("rebuild should fail when any branch has conflicts, even if others don't. Output: {}", full_output);
        }

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Merge conflict detected"));
        // Should conflict on README.md since both branches modify it
        assert!(stderr.contains("README.md"));

        // Verify cleanup: we should be back on the original branch after the conflict
        let current_branch = test_env.get_current_branch()?;
        assert_eq!(
            current_branch, "main",
            "Should be back on main branch after conflict"
        );

        // Verify temp branch was cleaned up
        assert!(
            !test_env.has_temp_branches()?,
            "Temp branches should be cleaned up after conflict"
        );

        // Verify working directory is clean after cleanup
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        assert!(
            git_ops.is_working_directory_clean()?,
            "Working directory should be clean after cleanup"
        );

        Ok(())
    })
}
