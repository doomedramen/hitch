use anyhow::Result;
use chrono::Utc;
use std::fs;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

/// Simple ANSI code stripper for test assertions
#[allow(dead_code)]
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

/// Helper extension trait for TestEnv to provide custom methods needed by release tests
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
    #[allow(dead_code)]
    fn tag_exists(&self, tag_name: &str) -> Result<bool>;
    fn list_tags(&self) -> Result<Vec<String>>;
    fn get_tag_message(&self, tag_name: &str) -> Result<String>;
    fn get_environment_config(&self, env_name: &str) -> Result<serde_json::Value>;
    fn commit_file_and_return(&self, filename: &str, content: &str, message: &str) -> Result<()>;
    fn ensure_clean_working_tree(&self) -> Result<()>;
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

        let mut environments = HashMap::new();
        let branches_vec: Vec<String> = branches.iter().map(|s| s.to_string()).collect();

        environments.insert(env_name.to_string(), serde_json::json!({
            "base": base_branch,
            "branches": branches_vec,
            "locked": locked,
            "locked_by": if locked { Some("admin@example.com".to_string()) } else { None },
            "locked_at": if locked { Some(Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()) } else { None },
            "rebuilt_at": None::<String>,
            "released_at": None::<String>
        }));

        let config = serde_json::json!({
            "version": "1.0",
            "environments": environments
        });

        fs::write(self.path().join("hitch.json"), config.to_string())?;
        Ok(())
    }

    fn create_branch_with_content(
        &self,
        branch_name: &str,
        filename: &str,
        content: &str,
    ) -> Result<()> {
        self.ensure_clean_working_tree()?;

        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Create the branch from current main
        git_ops.create_branch_from(branch_name, "main")?;

        // Switch to the branch
        git_ops.checkout_branch(branch_name)?;

        // Create and commit content
        let file_path = self.path().join(filename);
        fs::write(&file_path, content)?;

        git_ops.run_git_command(&["add", filename])?;
        git_ops.run_git_command(&[
            "commit",
            "-m",
            &format!("Add {} on {}", filename, branch_name),
        ])?;

        // Return to main
        git_ops.checkout_branch("main")?;

        Ok(())
    }

    fn tag_exists(&self, tag_name: &str) -> Result<bool> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // List all tags and check if ours exists
        let output = git_ops.run_git_command(&["tag", "-l", tag_name])?;
        if !output.status.success() {
            return Ok(false);
        }

        let tags = String::from_utf8_lossy(&output.stdout);
        Ok(tags.trim().contains(tag_name))
    }

    fn list_tags(&self) -> Result<Vec<String>> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        let output = git_ops.run_git_command(&["tag", "-l"])?;
        if !output.status.success() {
            return Ok(Vec::new());
        }

        let tags_str = String::from_utf8_lossy(&output.stdout);
        let tags: Vec<String> = tags_str
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect();

        Ok(tags)
    }

    fn get_tag_message(&self, tag_name: &str) -> Result<String> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        let output = git_ops.run_git_command(&["tag", "-l", tag_name, "-n99"])?;
        if !output.status.success() {
            return Ok(String::new());
        }

        let tag_line = String::from_utf8_lossy(&output.stdout);
        // Extract just the message part (after the tag name)
        if let Some(start) = tag_line.find(' ') {
            Ok(tag_line[start + 1..].trim().to_string())
        } else {
            Ok(String::new())
        }
    }

    fn get_environment_config(&self, env_name: &str) -> Result<serde_json::Value> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        // Switch to hitch-metadata branch to read the config
        let original_branch = git_ops.get_current_branch()?;

        if git_ops.branch_exists("hitch-metadata")? {
            git_ops.checkout_branch("hitch-metadata")?;
        } else {
            return Err(anyhow::anyhow!("hitch-metadata branch not found"));
        }

        let config_path = self.path().join("hitch.json");

        if !config_path.exists() {
            // Return to original branch before erroring
            git_ops.checkout_branch(&original_branch)?;
            return Err(anyhow::anyhow!(
                "hitch.json not found at {:?}. Environment may not be properly initialized.",
                config_path
            ));
        }

        let config_content = fs::read_to_string(&config_path).map_err(|e| {
            git_ops.checkout_branch(&original_branch).ok();
            anyhow::anyhow!("Failed to read hitch.json: {}", e)
        })?;

        let config: serde_json::Value = serde_json::from_str(&config_content).map_err(|e| {
            git_ops.checkout_branch(&original_branch).ok();
            anyhow::anyhow!("Failed to parse hitch.json: {}", e)
        })?;

        let result = if let Some(environments) = config["environments"].as_object() {
            if environments.contains_key(env_name) {
                Ok(config["environments"][env_name].clone())
            } else {
                let available_envs: Vec<String> = environments.keys().cloned().collect();
                Err(anyhow::anyhow!(
                    "Environment '{}' not found in hitch.json. Available environments: {}",
                    env_name,
                    available_envs.join(", ")
                ))
            }
        } else {
            Err(anyhow::anyhow!("No environments found in hitch.json"))
        };

        // Always return to original branch
        git_ops.checkout_branch(&original_branch)?;

        result
    }

    fn commit_file_and_return(&self, filename: &str, content: &str, message: &str) -> Result<()> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        fs::write(self.path().join(filename), content)?;
        git_ops.run_git_command(&["add", filename])?;
        git_ops.run_git_command(&["commit", "-m", message])?;

        Ok(())
    }

    fn ensure_clean_working_tree(&self) -> Result<()> {
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            self.path().to_str().unwrap(),
        )?;

        if !git_ops.is_working_directory_clean()? {
            git_ops.run_git_command(&["add", "."])?;
            git_ops.run_git_command(&[
                "commit",
                "-m",
                "Auto-commit: clean working tree for test",
            ])?;
        }

        Ok(())
    }
}

/// Test helper to create a test environment with proper setup
fn setup_release_test_env(test_env: &TestEnv, env_name: &str, branches: &[&str]) -> Result<()> {
    // Initialize hitch
    test_env.run_hitch_init()?;

    // Create environment with promoted branches
    test_env.create_hitch_config_with_environment(env_name, "main", branches, false)?;

    // Create the promoted branches with content
    for (i, branch) in branches.iter().enumerate() {
        let filename = format!("feature_{}.js", i + 1);
        let content = format!("// Feature content for {}", branch);
        test_env.create_branch_with_content(branch, &filename, &content)?;
    }

    Ok(())
}

/// Test helper to verify release artifacts
fn verify_release_artifacts(
    test_env: &TestEnv,
    env_name: &str,
    target_branch: &str,
    _tag_pattern: &str,
) -> Result<()> {
    // Verify tag was created with the new format pattern
    let tag_pattern = format!(
        "hitch-release-{}-to-{}",
        env_name,
        target_branch.replace('/', "-")
    );
    let tags = test_env.list_tags()?;

    let matching_tags: Vec<_> = tags
        .iter()
        .filter(|tag| tag.starts_with(&tag_pattern))
        .collect();

    assert!(
        !matching_tags.is_empty(),
        "No release tags found matching pattern '{}'. Available tags: {:?}",
        tag_pattern,
        tags
    );

    let created_tag = &matching_tags[0];

    // Verify tag message contains release information
    let tag_message = test_env.get_tag_message(created_tag)?;
    assert!(
        tag_message.contains(env_name) && tag_message.contains(target_branch),
        "Tag message should contain release information. Got: {}",
        tag_message
    );

    // Verify release timestamp was updated in environment config
    let env_config = test_env.get_environment_config(env_name)?;
    assert!(
        env_config["released_at"].is_string(),
        "Environment should have release timestamp"
    );

    Ok(())
}

#[test]
fn test_release_basic_success() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Setup the test environment
        if let Err(e) = setup_release_test_env(test_env, "qa", &["feature/login", "feature/api"]) {
            panic!("Setup failed: {}", e);
        }

        // Release the environment
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push", "--verbose"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "Release command failed. stdout: {}, stderr: {}",
                stdout, stderr
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Releasing environment 'qa'"),
            "Should show releasing message. Got: {}",
            stdout
        );
        assert!(
            stdout.contains("released successfully") || stdout.contains("✅"),
            "Should show success message. Got: {}",
            stdout
        );

        // Verify release artifacts
        if let Err(e) = verify_release_artifacts(test_env, "qa", "main", "release-qa-main") {
            panic!("Release artifacts verification failed: {}", e);
        }

        Ok(())
    })
}

#[test]
fn test_release_with_target_override() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "staging", &["feature/ui"])?;

        // Create a stable branch to release to
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.create_branch_from("stable", "main")?;

        // Release to stable branch (override default main)
        let output = test_env
            .hitch_command()
            .args(["release", "staging", "stable", "--no-push"])
            .output()?;

        assert!(
            output.status.success(),
            "Release to target branch should succeed"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Releasing environment 'staging'"),
            "Should show releasing message"
        );

        // Verify release artifacts for target override
        verify_release_artifacts(test_env, "staging", "stable", "release-staging-stable")?;

        Ok(())
    })
}

#[test]
fn test_release_empty_environment() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Initialize hitch and create empty environment
        test_env.run_hitch_init()?;
        test_env.create_hitch_config_with_environment("qa", "main", &[], false)?;

        // Ensure working tree is clean before release
        test_env.ensure_clean_working_tree()?;

        // Release empty environment
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "Release of empty environment failed. stdout: {}, stderr: {}",
                stdout, stderr
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("No branches promoted") || stdout.contains("nothing to release"),
            "Should inform about no branches to release. Got: {}",
            stdout
        );

        Ok(())
    })
}

#[test]
fn test_release_locked_environment_fails() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "prod", &["feature/locked"])?;

        // Lock the environment
        let lock_output = test_env
            .hitch_command()
            .args(["lock", "prod", "--no-push"])
            .output()?;
        assert!(
            lock_output.status.success(),
            "Environment locking should succeed"
        );

        // Try to release locked environment without force
        let output = test_env
            .hitch_command()
            .args(["release", "prod", "--no-push"])
            .output()?;

        assert!(
            !output.status.success(),
            "Release of locked environment should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("locked") || stderr.contains("locked"),
            "Should mention environment is locked"
        );

        Ok(())
    })
}

#[test]
fn test_release_locked_environment_with_force() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "prod", &["feature/force"])?;

        // Lock the environment
        let lock_output = test_env
            .hitch_command()
            .args(["lock", "prod", "--no-push"])
            .output()?;
        assert!(
            lock_output.status.success(),
            "Environment locking should succeed"
        );

        // Force release locked environment
        let output = test_env
            .hitch_command()
            .args(["release", "prod", "--force", "--no-push"])
            .output()?;

        assert!(
            output.status.success(),
            "Force release of locked environment should succeed"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Force releasing") || stdout.contains("Force"),
            "Should mention force release"
        );

        // Verify release artifacts
        verify_release_artifacts(test_env, "prod", "main", "release-prod-main")?;

        Ok(())
    })
}

#[test]
fn test_release_nonexistent_environment() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        test_env.run_hitch_init()?;

        // Ensure working tree is clean
        test_env.ensure_clean_working_tree()?;

        // Try to release nonexistent environment
        let output = test_env
            .hitch_command()
            .args(["release", "nonexistent", "--no-push"])
            .output()?;

        assert!(
            !output.status.success(),
            "Release of nonexistent environment should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("does not exist") || stderr.contains("not found"),
            "Should mention environment doesn't exist. Got: {}",
            stderr
        );

        Ok(())
    })
}

#[test]
fn test_release_dirty_working_directory_fails() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        // Initialize hitch first
        test_env.run_hitch_init()?;

        // Create environment with promoted branches
        test_env.create_hitch_config_with_environment("qa", "main", &["feature/dirty"], false)?;

        // Create the feature branch first
        test_env.create_branch_with_content(
            "feature/dirty",
            "dirty_feature.js",
            "// dirty feature",
        )?;

        // Ensure working tree is clean after setup, then create dirty files
        test_env.ensure_clean_working_tree()?;

        // Create dirty working directory after setup
        fs::write(test_env.path().join("dirty.txt"), "uncommitted changes")?;

        // Try to release with dirty working directory
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push"])
            .output()?;

        assert!(
            !output.status.success(),
            "Release with dirty working directory should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);

        // The error message is in stderr
        assert!(
            stderr.contains("Working tree is not clean"),
            "Should mention working tree not clean. Got: {}",
            stderr
        );

        Ok(())
    })
}

#[test]
fn test_release_missing_target_branch_fails() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "qa", &["feature/missing"])?;

        // Try to release to non-existent target branch
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "nonexistent-target", "--no-push"])
            .output()?;

        assert!(
            !output.status.success(),
            "Release to missing target should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Branch") && stderr.contains("does not exist"),
            "Should mention branch doesn't exist. Actual error: {}",
            stderr
        );

        Ok(())
    })
}

#[test]
fn test_release_with_merge_conflicts_fails() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "qa", &["feature/conflict"])?;

        // Create conflicting content in main branch
        test_env.commit_file_and_return(
            "conflict.txt",
            "main branch content",
            "Add conflict file to main",
        )?;

        // Create conflicting content in feature branch
        let git_ops = hitch::utils::git_operations::GitOperations::new_at_path(
            test_env.path().to_str().unwrap(),
        )?;
        git_ops.checkout_branch("feature/conflict")?;
        fs::write(
            test_env.path().join("conflict.txt"),
            "feature branch content",
        )?;
        git_ops.run_git_command(&["add", "conflict.txt"])?;
        git_ops.run_git_command(&["commit", "-m", "Add conflicting content to feature"])?;
        git_ops.checkout_branch("main")?;

        // Try to release - should detect conflicts
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push"])
            .output()?;

        assert!(
            !output.status.success(),
            "Release with conflicts should fail"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("conflict") || stderr.contains("conflict"),
            "Should mention merge conflicts"
        );

        Ok(())
    })
}

#[test]
fn test_release_command_help() -> Result<()> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hitch"));
    let output = cmd.args(["release", "--help"]).output()?;

    assert!(output.status.success(), "Help command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Release environment branches to target branch"),
        "Should contain command description"
    );
    assert!(
        stdout.contains("<ENV_NAME>"),
        "Should show environment argument"
    );
    assert!(
        stdout.contains("[TARGET_BRANCH]"),
        "Should show target branch argument"
    );
    assert!(stdout.contains("--force"), "Should show force flag");
    assert!(
        stdout.contains("Force release even if environment is locked"),
        "Should show force flag description"
    );

    Ok(())
}

#[test]
fn test_release_multiple_branches() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(
            test_env,
            "qa",
            &[
                "feature/ui",
                "feature/api",
                "feature/auth",
                "feature/database",
            ],
        )?;

        // Release environment with multiple branches
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push", "--verbose"])
            .output()?;

        assert!(
            output.status.success(),
            "Release of multiple branches should succeed"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Releasing 4 promoted branches"),
            "Should mention number of branches"
        );

        // Should show merge operations for all branches
        assert!(
            stdout.contains("Merging 'feature/ui'"),
            "Should mention UI feature"
        );
        assert!(
            stdout.contains("Merging 'feature/api'"),
            "Should mention API feature"
        );
        assert!(
            stdout.contains("Merging 'feature/auth'"),
            "Should mention auth feature"
        );
        assert!(
            stdout.contains("Merging 'feature/database'"),
            "Should mention database feature"
        );

        // Verify release artifacts
        verify_release_artifacts(test_env, "qa", "main", "release-qa-main")?;

        Ok(())
    })
}

#[test]
fn test_release_preserves_promoted_branches() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "qa", &["feature/preserve"])?;

        // Get initial environment state
        let initial_config = test_env.get_environment_config("qa")?;
        let initial_branches: Vec<String> = initial_config["branches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        // Release the environment
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push"])
            .output()?;

        assert!(output.status.success(), "Release should succeed");

        // Verify promoted branches are still in environment after release
        let final_config = test_env.get_environment_config("qa")?;
        let final_branches: Vec<String> = final_config["branches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        assert_eq!(
            initial_branches, final_branches,
            "Promoted branches should be preserved after release"
        );

        Ok(())
    })
}

#[test]
fn test_release_verbose_output() -> Result<()> {
    with_test_env(SetupLevel::GitOnly, |test_env| {
        setup_release_test_env(test_env, "qa", &["feature/verbose"])?;

        // Release with verbose flag
        let output = test_env
            .hitch_command()
            .args(["release", "qa", "--no-push", "--verbose"])
            .output()?;

        assert!(output.status.success(), "Release should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Should contain detailed verbose messages
        assert!(
            stdout.contains("Running release validation"),
            "Should show validation step"
        );
        assert!(
            stdout.contains("Resolving target branch"),
            "Should show resolution step"
        );
        assert!(
            stdout.contains("Synchronizing branches"),
            "Should show synchronization"
        );
        assert!(
            stdout.contains("Switching to target branch"),
            "Should show branch switch"
        );
        assert!(
            stdout.contains("Committed release"),
            "Should show commit confirmation"
        );
        assert!(
            stdout.contains("Created release tag"),
            "Should show tag creation"
        );

        Ok(())
    })
}
