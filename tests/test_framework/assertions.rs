//! Assertion helpers for Hitch-specific validations
//!
//! Provides custom assertions for common test scenarios in Hitch CLI testing
//! including git state validation, environment checks, and command output validation.

use anyhow::Result;

use crate::test_framework::command_runners::{GitCommandRunner, HitchCommandResult};
use crate::test_framework::file_system_helpers::FileSystemHelpers;

/// Assertion helpers for Hitch-specific validations
///
/// Provides convenient methods for asserting common conditions in Hitch tests
#[derive(Debug, Default)]
pub struct AssertionHelpers {
    // Currently stateless, but could hold configuration in the future
}

impl AssertionHelpers {
    /// Create new assertion helpers
    pub fn new() -> Self {
        Self::default()
    }

    // File system assertions

    /// Assert that a file exists
    pub fn file_exists(&self, fs: &FileSystemHelpers, relative_path: &str) -> &Self {
        if !fs.file_exists(relative_path) {
            panic!("Expected file '{}' to exist, but it doesn't", relative_path);
        }
        self
    }

    /// Assert that a file does not exist
    pub fn file_not_exists(&self, fs: &FileSystemHelpers, relative_path: &str) -> &Self {
        if fs.file_exists(relative_path) {
            panic!(
                "Expected file '{}' to not exist, but it does",
                relative_path
            );
        }
        self
    }

    /// Assert that a directory exists
    pub fn dir_exists(&self, fs: &FileSystemHelpers, relative_path: &str) -> &Self {
        if !fs.dir_exists(relative_path) {
            panic!(
                "Expected directory '{}' to exist, but it doesn't",
                relative_path
            );
        }
        self
    }

    /// Assert that a directory does not exist
    pub fn dir_not_exists(&self, fs: &FileSystemHelpers, relative_path: &str) -> &Self {
        if fs.dir_exists(relative_path) {
            panic!(
                "Expected directory '{}' to not exist, but it does",
                relative_path
            );
        }
        self
    }

    /// Assert file content matches expected value
    pub fn file_content_equals(
        &self,
        fs: &FileSystemHelpers,
        relative_path: &str,
        expected: &str,
    ) -> &Self {
        match fs.read_file(relative_path) {
            Ok(content) if content == expected => self,
            Ok(content) => panic!(
                "File '{}' content mismatch.\nExpected: {:?}\nActual: {:?}",
                relative_path, expected, content
            ),
            Err(e) => panic!("Failed to read file '{}': {}", relative_path, e),
        }
    }

    /// Assert file content contains expected substring
    pub fn file_content_contains(
        &self,
        fs: &FileSystemHelpers,
        relative_path: &str,
        expected: &str,
    ) -> &Self {
        match fs.read_file(relative_path) {
            Ok(content) if content.contains(expected) => self,
            Ok(content) => panic!(
                "File '{}' does not contain expected substring.\nExpected to contain: {:?}\nActual content: {:?}",
                relative_path, expected, content
            ),
            Err(e) => panic!("Failed to read file '{}': {}", relative_path, e),
        }
    }

    // Git state assertions

    /// Assert that git repository exists
    pub fn git_repo_exists(&self, git: &GitCommandRunner) -> Result<&Self> {
        let result = git.run(&["rev-parse", "--git-dir"])?;
        if !result.success() {
            panic!("Expected git repository to exist, but git rev-parse failed");
        }
        Ok(self)
    }

    /// Assert that a git branch exists
    pub fn git_branch_exists(&self, git: &GitCommandRunner, branch_name: &str) -> Result<&Self> {
        let result = git.run(&["branch", "--list", branch_name])?;
        if !result.success() || !result.stdout().contains(branch_name) {
            panic!(
                "Expected git branch '{}' to exist, but it doesn't",
                branch_name
            );
        }
        Ok(self)
    }

    /// Assert that a git branch does not exist
    pub fn git_branch_not_exists(
        &self,
        git: &GitCommandRunner,
        branch_name: &str,
    ) -> Result<&Self> {
        let result = git.run(&["branch", "--list", branch_name])?;
        if result.success() && result.stdout().contains(branch_name) {
            panic!(
                "Expected git branch '{}' to not exist, but it does",
                branch_name
            );
        }
        Ok(self)
    }

    /// Assert current git branch
    pub fn git_current_branch(
        &self,
        git: &GitCommandRunner,
        expected_branch: &str,
    ) -> Result<&Self> {
        let result = git.run(&["branch", "--show-current"])?;
        if !result.success() {
            panic!("Failed to get current git branch");
        }
        let stdout = result.stdout();
        let current = stdout.trim();
        if current != expected_branch {
            panic!(
                "Expected current branch to be '{}', but it is '{}'",
                expected_branch, current
            );
        }
        Ok(self)
    }

    /// Assert that working directory is clean
    pub fn git_working_dir_clean(&self, git: &GitCommandRunner) -> Result<&Self> {
        let result = git.run(&["status", "--porcelain"])?;
        if !result.success() {
            panic!("Failed to check git status");
        }
        if !result.stdout().trim().is_empty() {
            panic!(
                "Expected working directory to be clean, but there are changes:\n{}",
                result.stdout()
            );
        }
        Ok(self)
    }

    /// Assert that working directory has changes
    pub fn git_working_dir_dirty(&self, git: &GitCommandRunner) -> Result<&Self> {
        let result = git.run(&["status", "--porcelain"])?;
        if !result.success() {
            panic!("Failed to check git status");
        }
        if result.stdout().trim().is_empty() {
            panic!("Expected working directory to have changes, but it is clean");
        }
        Ok(self)
    }

    // Hitch-specific assertions

    /// Assert that Hitch is initialized (hitch.json exists and is valid)
    pub fn hitch_initialized(&self, fs: &FileSystemHelpers) -> Result<&Self> {
        self.file_exists(fs, "hitch.json");

        // Try to parse hitch.json to ensure it's valid
        match fs.read_json::<serde_json::Value>("hitch.json") {
            Ok(_) => Ok(self),
            Err(e) => panic!("hitch.json exists but is not valid JSON: {}", e),
        }
    }

    /// Assert that Hitch is not initialized
    pub fn hitch_not_initialized(&self, fs: &FileSystemHelpers) -> &Self {
        self.file_not_exists(fs, "hitch.json")
    }

    /// Assert that an environment exists in hitch.json
    pub fn hitch_environment_exists(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
    ) -> Result<&Self> {
        self.hitch_initialized(fs)?;

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if environments.contains_key(env_name) {
                return Ok(self);
            }
        }

        panic!("Expected environment '{}' to exist in hitch.json", env_name);
    }

    /// Assert that an environment does not exist in hitch.json
    pub fn hitch_environment_not_exists(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
    ) -> Result<&Self> {
        if !fs.file_exists("hitch.json") {
            return Ok(self); // Hitch not initialized means no environments exist
        }

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if environments.contains_key(env_name) {
                panic!(
                    "Expected environment '{}' to not exist in hitch.json",
                    env_name
                );
            }
        }

        Ok(self)
    }

    /// Assert that a branch is promoted to an environment
    pub fn hitch_branch_promoted(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
        branch: &str,
    ) -> Result<&Self> {
        self.hitch_environment_exists(fs, env_name)?;

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if let Some(env) = environments.get(env_name).and_then(|e| e.as_object()) {
                if let Some(branches) = env.get("branches").and_then(|b| b.as_array()) {
                    let branch_names: Vec<&str> =
                        branches.iter().filter_map(|b| b.as_str()).collect();

                    if branch_names.contains(&branch) {
                        return Ok(self);
                    }
                }
            }
        }

        panic!(
            "Expected branch '{}' to be promoted to environment '{}'",
            branch, env_name
        );
    }

    /// Assert that a branch is not promoted to an environment
    pub fn hitch_branch_not_promoted(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
        branch: &str,
    ) -> Result<&Self> {
        if !fs.file_exists("hitch.json") {
            return Ok(self); // Hitch not initialized means no promotions
        }

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if let Some(env) = environments.get(env_name).and_then(|e| e.as_object()) {
                if let Some(branches) = env.get("branches").and_then(|b| b.as_array()) {
                    let branch_names: Vec<&str> =
                        branches.iter().filter_map(|b| b.as_str()).collect();

                    if branch_names.contains(&branch) {
                        panic!(
                            "Expected branch '{}' to not be promoted to environment '{}'",
                            branch, env_name
                        );
                    }
                }
            }
        }

        Ok(self)
    }

    /// Assert that an environment is locked
    pub fn hitch_environment_locked(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
    ) -> Result<&Self> {
        self.hitch_environment_exists(fs, env_name)?;

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if let Some(env) = environments.get(env_name).and_then(|e| e.as_object()) {
                if let Some(locked) = env.get("locked").and_then(|l| l.as_bool()) {
                    if locked {
                        return Ok(self);
                    }
                }
            }
        }

        panic!("Expected environment '{}' to be locked", env_name);
    }

    /// Assert that an environment is not locked
    pub fn hitch_environment_unlocked(
        &self,
        fs: &FileSystemHelpers,
        env_name: &str,
    ) -> Result<&Self> {
        if !fs.file_exists("hitch.json") {
            return Ok(self); // Hitch not initialized means environments are unlocked
        }

        let config: serde_json::Value = fs.read_json("hitch.json")?;

        if let Some(environments) = config.get("environments").and_then(|e| e.as_object()) {
            if let Some(env) = environments.get(env_name).and_then(|e| e.as_object()) {
                if let Some(locked) = env.get("locked").and_then(|l| l.as_bool()) {
                    if locked {
                        panic!("Expected environment '{}' to be unlocked", env_name);
                    }
                }
            }
        }

        Ok(self)
    }

    // Hitch command output assertions

    /// Assert hitch command output contains success indicators
    pub fn hitch_command_success(&self, result: &HitchCommandResult) -> &Self {
        if !result.success() {
            panic!(
                "Expected hitch command to succeed, but it failed\nstdout: {}\nstderr: {}",
                result.stdout(),
                result.stderr()
            );
        }
        self
    }

    /// Assert hitch command output contains failure indicators
    pub fn hitch_command_failure(&self, result: &HitchCommandResult) -> &Self {
        if result.success() {
            panic!(
                "Expected hitch command to fail, but it succeeded\nstdout: {}",
                result.stdout()
            );
        }
        self
    }

    /// Assert hitch command output contains specific text
    pub fn hitch_output_contains(&self, result: &HitchCommandResult, expected_text: &str) -> &Self {
        let full_output = format!("{} {}", result.stdout(), result.stderr());
        if !full_output.contains(expected_text) {
            panic!(
                "Expected hitch command output to contain '{}', but it didn't\nFull output: {}",
                expected_text, full_output
            );
        }
        self
    }

    /// Assert hitch command output does not contain specific text
    pub fn hitch_output_not_contains(
        &self,
        result: &HitchCommandResult,
        forbidden_text: &str,
    ) -> &Self {
        let full_output = format!("{} {}", result.stdout(), result.stderr());
        if full_output.contains(forbidden_text) {
            panic!(
                "Expected hitch command output to not contain '{}', but it did\nFull output: {}",
                forbidden_text, full_output
            );
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_framework::command_runners::GitCommandRunner;
    use crate::test_framework::file_system_helpers::FileSystemHelpers;
    use tempfile::TempDir;

    #[test]
    fn test_file_assertions() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());
        let assert = AssertionHelpers::new();

        // Test file existence assertions
        assert.file_not_exists(&fs, "test.txt");

        fs.write_file("test.txt", "Hello, World!")?;
        assert.file_exists(&fs, "test.txt");
        assert.file_content_equals(&fs, "test.txt", "Hello, World!");
        assert.file_content_contains(&fs, "test.txt", "World");

        Ok(())
    }

    #[test]
    fn test_git_assertions() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let git = GitCommandRunner::new(temp_dir.path())?;
        let assert = AssertionHelpers::new();

        // Initialize repo
        git.init()?;
        git.config_user("Test User", "test@example.com")?;

        // Test git assertions
        assert.git_repo_exists(&git)?;
        assert.git_branch_exists(&git, "main")?;
        assert.git_current_branch(&git, "main")?;
        assert.git_working_dir_clean(&git)?;

        // Create some changes
        git.create_file_and_commit("test.txt", "test content", "Initial commit")?;
        assert.git_working_dir_clean(&git)?;

        Ok(())
    }

    #[test]
    fn test_hitch_assertions() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());
        let assert = AssertionHelpers::new();

        // Test hitch not initialized
        assert.hitch_not_initialized(&fs);
        assert.hitch_environment_not_exists(&fs, "dev")?;

        // Initialize hitch
        let hitch_config = serde_json::json!({
            "version": "1.0",
            "environments": {
                "dev": {
                    "base": "main",
                    "branches": ["feature-1"],
                    "locked": false
                }
            }
        });

        fs.write_json("hitch.json", &hitch_config)?;

        // Test hitch initialized assertions
        assert.hitch_initialized(&fs)?;
        assert.hitch_environment_exists(&fs, "dev")?;
        assert.hitch_branch_promoted(&fs, "dev", "feature-1")?;
        assert.hitch_environment_unlocked(&fs, "dev")?;

        Ok(())
    }
}
