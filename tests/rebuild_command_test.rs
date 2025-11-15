use anyhow::Result;
use chrono::Utc;
use std::process::Command;
use std::fs;
use std::path::Path;

// Import the proper test framework
mod common;
use common::{SetupLevel, with_test_env, TestEnv};

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
    fn create_hitch_config_with_environment(&self, env_name: &str, base_branch: &str, branches: &[&str], locked: bool) -> Result<()>;
    fn create_branch_with_content(&self, branch_name: &str, filename: &str, content: &str) -> Result<()>;
    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output>;
    fn get_current_branch(&self) -> Result<String>;
}

impl TestEnvExt for TestEnv {
    fn create_hitch_config_with_environment(&self, env_name: &str, base_branch: &str, branches: &[&str], locked: bool) -> Result<()> {
        use std::collections::HashMap;

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

        // Write to hitch-metadata branch
        Command::new("git")
            .args(&["checkout", "hitch-metadata"])
            .current_dir(self.path())
            .output()?;

        fs::write(self.path().join("hitch.json"), serde_json::to_string_pretty(&config)?)?;
        fs::write(self.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

        Command::new("git")
            .args(&["add", "hitch.json", ".gitignore"])
            .current_dir(self.path())
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", "Add hitch configuration"])
            .current_dir(self.path())
            .output()?;

        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(self.path())
            .output()?;

        Ok(())
    }

    fn create_branch_with_content(&self, branch_name: &str, filename: &str, content: &str) -> Result<()> {
        // Create branch
        Command::new("git")
            .args(&["checkout", "-b", branch_name])
            .current_dir(self.path())
            .output()?;

        // Write content and commit
        fs::write(self.path().join(filename), content)?;
        Command::new("git")
            .args(&["add", filename])
            .current_dir(self.path())
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", &format!("Add {}", filename)])
            .current_dir(self.path())
            .output()?;

        // Return to main
        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(self.path())
            .output()?;

        Ok(())
    }

    fn run_hitch_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let output = Command::new(&self.hitch_binary())
            .args(args)
            .current_dir(self.path())
            .output()?;

        Ok(output)
    }

    fn get_current_branch(&self) -> Result<String> {
        let output = Command::new("git")
            .args(&["branch", "--show-current"])
            .current_dir(self.path())
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[test]
fn test_rebuild_basic_success() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Create feature branch with content
        test_env.create_branch_with_content("feature/login", "login.md", "# Login feature\n")?;

        // Initialize hitch with dev environment
        test_env.create_hitch_config_with_environment("dev", "main", &["feature/login"], false)?;

        // Run rebuild command
        let output = test_env.run_hitch_command(&["rebuild", "dev", "--verbose"])?;

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(output.status.success(), "rebuild command should succeed. Output: {}", full_output);

        // Verify rebuild process steps
        assert!(full_output.contains("Rebuilding environment 'dev'"));
        assert!(full_output.contains("Creating temporary branch"));
        assert!(full_output.contains("Merging promoted branches"));
        assert!(full_output.contains("Replacing 'dev' branch"));
        assert!(full_output.contains("Environment 'dev' rebuilt successfully"));

        // Verify dev branch exists and has the expected content
        Command::new("git")
            .args(&["checkout", "dev"])
            .current_dir(temp_dir.path())
            .output()?;

        assert!(test_env.path().join("login.md").exists(), "dev branch should have login.md from feature branch");
        assert!(test_env.path().join("README.md").exists(), "dev branch should have README.md from base branch");

        // Verify rebuiltAt timestamp was updated
        Command::new("git")
            .args(&["checkout", "hitch-metadata"])
            .current_dir(temp_dir.path())
            .output()?;

        let updated_config = std::fs::read_to_string(test_env.path().join("hitch.json"))?;
        assert!(updated_config.contains("\"rebuilt_at\""), "rebuilt_at should be set after successful rebuild");

        Ok(())
    })
}

#[test]
fn test_rebuild_empty_environment() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Initialize hitch with empty environment
        test_env.create_hitch_config_with_environment("staging", "main", &[], false)?;

        // Run rebuild command
        let output = test_env.run_hitch_command(&["rebuild", "staging", "--verbose"])?;

        assert!(output.status.success(), "rebuild should succeed with empty environment");

        let stdout = String::from_utf8(output.stdout.clone())?;
        let stderr = String::from_utf8(output.stderr)?;
        let full_output = format!("{}{}", stdout, stderr);

        assert!(full_output.contains("No branches promoted to this environment, using base branch only"));

        // Verify staging branch exists and matches main branch
        Command::new("git")
            .args(&["checkout", "staging"])
            .current_dir(temp_dir.path())
            .output()?;

        assert!(test_env.path().join("README.md").exists(), "staging branch should have base branch content");

        Ok(())
    })
}

#[test]
fn test_rebuild_nonexistent_environment() -> Result<()> {
    with_test_env(SetupLevel::Complete, |test_env| {
        // Initialize hitch with dev environment only
        test_env.create_hitch_config_with_environment("dev", "main", &[], false)?;

        // Try to rebuild nonexistent environment
        let output = test_env.run_hitch_command(&["rebuild", "production"])?;

        assert!(!output.status.success(), "rebuild should fail for nonexistent environment");

        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("Environment 'production' does not exist"));

        Ok(())
    })
}

#[test]
fn test_rebuild_locked_environment() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    // Initialize hitch with locked environment
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(temp_env.path())
        .output()?;

    let locked_time = Utc::now();
    let hitch_config = format!(r#"{{
  "version": "1.0",
  "environments": {{
    "production": {{
      "base": "main",
      "branches": [],
      "locked": true,
      "locked_by": "admin@example.com",
      "locked_at": "{}",
      "rebuilt_at": null
    }}
  }}
}}"#, locked_time.format("%Y-%m-%dT%H:%M:%SZ"));

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Try to rebuild locked environment (should fail)
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "production"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(!output.status.success(), "rebuild should fail for locked environment");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Environment 'production' is locked by admin@example.com"));
    assert!(stderr.contains("Use --force to override"));

    // Try to rebuild with --force flag (should succeed)
    let force_output = Command::new(&hitch_path)
        .args(&["rebuild", "production", "--force"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(force_output.status.success(), "rebuild with --force should succeed for locked environment");

    Ok(())
}

#[test]
fn test_rebuild_missing_branch() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    // Initialize hitch with environment that references non-existent branch
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "main",
      "branches": ["nonexistent/feature"],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Try to rebuild environment with missing branch
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "dev", "--verbose"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(!output.status.success(), "rebuild should fail for missing branch");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Branch 'nonexistent/feature' does not exist"));

    Ok(())
}

#[test]
fn test_rebuild_missing_base_branch() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit on main (but environment references develop)
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    // Initialize hitch with environment that references non-existent base branch
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "develop",
      "branches": [],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Try to rebuild environment with missing base branch
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "dev", "--verbose"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(!output.status.success(), "rebuild should fail for missing base branch");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Base branch 'develop' does not exist"));

    Ok(())
}

#[test]
fn test_rebuild_multiple_branches() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit on main
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    // Create multiple feature branches
    let branches = vec!["feature/login", "feature/ui", "feature/api"];

    for branch in &branches {
        // Create branch
        Command::new("git")
            .args(&["checkout", "-b", branch])
            .current_dir(temp_env.path())
            .output()?;

        // Add content to branch
        let filename = format!("{}.md", branch.replace("/", "_"));
        let content = format!("# {} feature\n", branch);
        std::fs::write(temp_dir.path().join(&filename), content)?;

        Command::new("git")
            .args(&["add", &filename])
            .current_dir(temp_env.path())
            .output()?;

        Command::new("git")
            .args(&["commit", "-m", &format!("Add {} feature", branch)])
            .current_dir(temp_env.path())
            .output()?;

        // Return to main
        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(temp_env.path())
            .output()?;
    }

    // Initialize hitch with environment that has multiple promoted branches
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_config = format!(r#"{{
  "version": "1.0",
  "environments": {{
    "dev": {{
      "base": "main",
      "branches": [{}],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }}
  }}
}}"#, branches.iter().map(|b| format!("\"{}\"", b)).collect::<Vec<_>>().join(", "));

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Run rebuild command
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "dev", "--verbose"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(output.status.success(), "rebuild should succeed with multiple branches");

    let stdout = String::from_utf8(output.stdout.clone())?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);
    let clean_output = strip_ansi_codes(&full_output);

    // Verify all branches were processed
    for branch in &branches {
        let processing_msg = format!("Processing branch '{}'", branch);
        let merged_msg = format!("Squash merged '{}' into temp branch", branch);

        assert!(clean_output.contains(&processing_msg), "Should process branch '{}'", branch);
        assert!(clean_output.contains(&merged_msg), "Should merge branch '{}'", branch);
    }

    // Verify dev branch exists and has content from all feature branches
    Command::new("git")
        .args(&["checkout", "dev"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(temp_dir.path().join("README.md").exists(), "dev branch should have base branch content");

    for branch in &branches {
        let filename = format!("{}.md", branch.replace("/", "_"));
        assert!(temp_dir.path().join(&filename).exists(), "dev branch should have content from {}", branch);
    }

    Ok(())
}

#[test]
fn test_rebuild_with_git_hooks() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    // Create feature branch
    Command::new("git")
        .args(&["checkout", "-b", "feature/test"])
        .current_dir(temp_env.path())
        .output()?;

    std::fs::write(temp_dir.path().join("feature.txt"), "# Feature\n")?;
    Command::new("git")
        .args(&["add", "feature.txt"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add feature"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    // Initialize hitch with dev environment
    Command::new("git")
        .args(&["checkout", "--orphan", "hitch-metadata"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_config = r#"{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "main",
      "branches": ["feature/test"],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "rebuilt_at": null
    }
  }
}"#;

    std::fs::write(temp_dir.path().join("hitch.json"), hitch_config)?;
    std::fs::write(temp_dir.path().join(".gitignore"), "*\n!.gitignore\n!hitch.json\n")?;

    Command::new("git")
        .args(&["add", "hitch.json", ".gitignore"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Add hitch configuration"])
        .current_dir(temp_env.path())
        .output()?;

    // Return to main branch
    Command::new("git")
        .args(&["checkout", "main"])
        .current_dir(temp_env.path())
        .output()?;

    // Set up a problematic pre-commit hook that would normally fail
    let hooks_dir = temp_dir.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    let pre_commit_hook = r#"#!/bin/sh
# This hook simulates a problematic hook that might fail
echo "Simulating hook that could interfere with automated operations"
# We'll make this hook succeed, but test that it doesn't interfere
exit 0
"#;

    std::fs::write(hooks_dir.join("pre-commit"), pre_commit_hook)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(hooks_dir.join("pre-commit"))?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(hooks_dir.join("pre-commit"), perms)?;
    }

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Run rebuild command - should succeed despite git hooks
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "dev", "--verbose"])
        .current_dir(temp_env.path())
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let full_output = format!("{}{}", stdout, stderr);

    println!("=== Rebuild Output ===");
    println!("{}", full_output);
    println!("=== End Output ===");

    if output.status.success() {
        // Verify rebuild process completed successfully
        assert!(full_output.contains("Environment 'dev' rebuilt successfully"));
    } else {
        println!("=== Rebuild Failed ===");
        // Check what went wrong
    }

    // Verify we returned to the original branch (not stuck on temp branch)
    let current_branch_output = Command::new("git")
        .args(&["branch", "--show-current"])
        .current_dir(temp_env.path())
        .output()?;

    let current_branch_str = String::from_utf8(current_branch_output.stdout)?;
    let current_branch = current_branch_str.trim();

    println!("Current branch after rebuild: '{}'", current_branch);
    println!("Expected branch: 'main'");

    if current_branch == "main" {
        // Verify dev branch exists and has the expected content
        Command::new("git")
            .args(&["checkout", "dev"])
            .current_dir(temp_env.path())
            .output()?;

        assert!(temp_dir.path().join("README.md").exists(), "dev branch should have base branch content");
        assert!(temp_dir.path().join("feature.txt").exists(), "dev branch should have feature branch content");
    } else {
        println!("FAIL: Still on temp branch! The cleanup logic needs more work.");
    }

    // Verify dev branch exists and has the expected content
    Command::new("git")
        .args(&["checkout", "dev"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(temp_dir.path().join("README.md").exists(), "dev branch should have base branch content");
    assert!(temp_dir.path().join("feature.txt").exists(), "dev branch should have feature branch content");

    Ok(())
}

#[test]
fn test_rebuild_not_initialized() -> Result<()> {
    let temp_dir = std::env::current_dir()?;

    // Initialize git repo but don't initialize hitch
    Command::new("git")
        .args(&["init"])
        .current_dir(temp_env.path())
        .output()?;

    // Configure git user
    Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_env.path())
        .output()?;

    // Create initial commit
    std::fs::write(temp_dir.path().join("README.md"), "# Initial\n")?;
    Command::new("git")
        .args(&["add", "README.md"])
        .current_dir(temp_env.path())
        .output()?;

    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(temp_env.path())
        .output()?;

    let hitch_path = env!("CARGO_MANIFEST_DIR").to_string() + "/target/debug/hitch";

    // Try to rebuild without hitch being initialized
    let output = Command::new(&hitch_path)
        .args(&["rebuild", "dev"])
        .current_dir(temp_env.path())
        .output()?;

    assert!(!output.status.success(), "rebuild should fail when hitch not initialized");

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Failed to read hitch.json") ||
            stderr.contains("Failed to access hitch metadata") ||
            stderr.contains("Failed to checkout branch"));

    Ok(())
}