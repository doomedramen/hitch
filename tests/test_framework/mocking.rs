//! Mocking capabilities for external dependencies
//!
//! Provides utilities for mocking external services, network calls, and other
//! external dependencies that Hitch CLI might interact with during testing.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock capabilities for external dependencies
///
/// Provides a framework for mocking external services and dependencies
/// that Hitch CLI might interact with during testing.
#[derive(Debug, Clone)]
pub struct MockCapabilities {
    shared_state: Arc<Mutex<MockSharedState>>,
}

#[derive(Debug, Default)]
struct MockSharedState {
    // Git remote mocking
    git_remotes: HashMap<String, MockGitRemote>,

    // Network mocking
    network_responses: HashMap<String, MockNetworkResponse>,
    network_failures: HashMap<String, String>,

    // File system mocking (for external paths)
    external_files: HashMap<String, String>,

    // Environment variable mocking
    env_vars: HashMap<String, String>,

    // Time mocking
    fixed_time: Option<std::time::SystemTime>,
}

impl Default for MockCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCapabilities {
    /// Create new mock capabilities
    pub fn new() -> Self {
        MockCapabilities {
            shared_state: Arc::new(Mutex::new(MockSharedState::default())),
        }
    }

    // Git remote mocking

    /// Mock a git remote repository
    pub fn mock_git_remote(&self, remote_name: &str, remote: MockGitRemote) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.git_remotes.insert(remote_name.to_string(), remote);
        Ok(())
    }

    /// Get a mocked git remote
    pub fn get_git_remote(&self, remote_name: &str) -> Option<MockGitRemote> {
        let state = self.shared_state.lock().ok()?;
        state.git_remotes.get(remote_name).cloned()
    }

    /// Remove a mocked git remote
    pub fn remove_git_remote(&self, remote_name: &str) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.git_remotes.remove(remote_name);
        Ok(())
    }

    // Network mocking

    /// Mock a network response for a URL
    pub fn mock_network_response(&self, url: &str, response: MockNetworkResponse) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.network_responses.insert(url.to_string(), response);
        Ok(())
    }

    /// Mock a network failure for a URL
    pub fn mock_network_failure(&self, url: &str, error_message: &str) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state
            .network_failures
            .insert(url.to_string(), error_message.to_string());
        Ok(())
    }

    /// Get a mocked network response
    pub fn get_network_response(&self, url: &str) -> Option<MockNetworkResponse> {
        let state = self.shared_state.lock().ok()?;
        state.network_responses.get(url).cloned()
    }

    /// Get a mocked network failure
    pub fn get_network_failure(&self, url: &str) -> Option<String> {
        let state = self.shared_state.lock().ok()?;
        state.network_failures.get(url).cloned()
    }

    // External file mocking

    /// Mock an external file (outside test directory)
    pub fn mock_external_file(&self, path: &str, content: &str) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state
            .external_files
            .insert(path.to_string(), content.to_string());
        Ok(())
    }

    /// Get a mocked external file
    pub fn get_external_file(&self, path: &str) -> Option<String> {
        let state = self.shared_state.lock().ok()?;
        state.external_files.get(path).cloned()
    }

    // Environment variable mocking

    /// Mock an environment variable
    pub fn mock_env_var(&self, key: &str, value: &str) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.env_vars.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Get a mocked environment variable
    pub fn get_env_var(&self, key: &str) -> Option<String> {
        let state = self.shared_state.lock().ok()?;
        state.env_vars.get(key).cloned()
    }

    // Time mocking

    /// Mock a fixed time for deterministic testing
    pub fn mock_fixed_time(&self, time: std::time::SystemTime) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.fixed_time = Some(time);
        Ok(())
    }

    /// Get the mocked fixed time
    pub fn get_fixed_time(&self) -> Option<std::time::SystemTime> {
        let state = self.shared_state.lock().ok()?;
        state.fixed_time
    }

    /// Clear the fixed time mock
    pub fn clear_fixed_time(&self) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.fixed_time = None;
        Ok(())
    }

    /// Clear all mocks
    pub fn clear_all_mocks(&self) -> Result<()> {
        let mut state = self
            .shared_state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock: {}", e))?;

        state.git_remotes.clear();
        state.network_responses.clear();
        state.network_failures.clear();
        state.external_files.clear();
        state.env_vars.clear();
        state.fixed_time = None;

        Ok(())
    }
}

/// Mock configuration for a git remote repository
#[derive(Debug, Clone)]
pub struct MockGitRemote {
    /// Remote URL (can be a fake URL for testing)
    pub url: String,

    /// Available branches in the remote
    pub branches: HashMap<String, MockBranch>,

    /// Whether the remote should simulate connection failures
    pub should_fail: bool,

    /// Error message to return when should_fail is true
    pub error_message: Option<String>,

    /// Delay before responding (in milliseconds)
    pub response_delay_ms: u64,
}

impl Default for MockGitRemote {
    fn default() -> Self {
        MockGitRemote {
            url: "https://github.com/example/repo.git".to_string(),
            branches: HashMap::new(),
            should_fail: false,
            error_message: None,
            response_delay_ms: 0,
        }
    }
}

/// Mock configuration for a git branch
#[derive(Debug, Clone)]
pub struct MockBranch {
    /// Branch name
    pub name: String,

    /// Commit history (oldest to newest)
    pub commits: Vec<MockCommit>,

    /// Whether the branch exists
    pub exists: bool,

    /// Whether the branch is protected
    pub protected: bool,
}

impl MockBranch {
    /// Create a new mock branch
    pub fn new(name: &str) -> Self {
        MockBranch {
            name: name.to_string(),
            commits: Vec::new(),
            exists: true,
            protected: false,
        }
    }

    /// Add a commit to the branch
    pub fn add_commit(mut self, commit: MockCommit) -> Self {
        self.commits.push(commit);
        self
    }
}

/// Mock configuration for a git commit
#[derive(Debug, Clone)]
pub struct MockCommit {
    /// Commit SHA
    pub sha: String,

    /// Author name
    pub author: String,

    /// Commit message
    pub message: String,

    /// Commit timestamp
    pub timestamp: std::time::SystemTime,

    /// Files changed in this commit
    pub files_changed: Vec<String>,
}

impl MockCommit {
    /// Create a new mock commit
    pub fn new(sha: &str, author: &str, message: &str) -> Self {
        MockCommit {
            sha: sha.to_string(),
            author: author.to_string(),
            message: message.to_string(),
            timestamp: std::time::SystemTime::now(),
            files_changed: Vec::new(),
        }
    }
}

/// Mock configuration for a network response
#[derive(Debug, Clone)]
pub struct MockNetworkResponse {
    /// HTTP status code
    pub status_code: u16,

    /// Response body
    pub body: String,

    /// Response headers
    pub headers: HashMap<String, String>,

    /// Delay before responding (in milliseconds)
    pub response_delay_ms: u64,
}

impl MockNetworkResponse {
    /// Create a new successful response
    pub fn success(body: &str) -> Self {
        MockNetworkResponse {
            status_code: 200,
            body: body.to_string(),
            headers: HashMap::new(),
            response_delay_ms: 0,
        }
    }

    /// Create a new error response
    pub fn error(status_code: u16, body: &str) -> Self {
        MockNetworkResponse {
            status_code,
            body: body.to_string(),
            headers: HashMap::new(),
            response_delay_ms: 0,
        }
    }

    /// Add a header to the response
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    /// Set response delay
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.response_delay_ms = delay_ms;
        self
    }
}

/// Builder for creating common mock configurations
pub struct MockBuilder;

impl MockBuilder {
    /// Create a mock git remote with common setup
    pub fn git_remote(url: &str) -> MockGitRemote {
        MockGitRemote {
            url: url.to_string(),
            branches: HashMap::new(),
            should_fail: false,
            error_message: None,
            response_delay_ms: 0,
        }
    }

    /// Create a mock network response
    pub fn network_response(body: &str) -> MockNetworkResponse {
        MockNetworkResponse::success(body)
    }

    /// Create a mock commit
    pub fn commit(sha: &str, author: &str, message: &str) -> MockCommit {
        MockCommit::new(sha, author, message)
    }

    /// Create a mock branch with commits
    pub fn branch(name: &str, commits: Vec<MockCommit>) -> MockBranch {
        MockBranch {
            name: name.to_string(),
            commits,
            exists: true,
            protected: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn test_git_remote_mocking() -> Result<()> {
        let mock = MockCapabilities::new();

        let remote = MockBuilder::git_remote("https://github.com/example/repo.git");
        mock.mock_git_remote("origin", remote)?;

        let retrieved = mock.get_git_remote("origin");
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().url,
            "https://github.com/example/repo.git"
        );

        Ok(())
    }

    #[test]
    fn test_network_mocking() -> Result<()> {
        let mock = MockCapabilities::new();

        let response = MockBuilder::network_response("Hello, World!")
            .with_header("Content-Type", "text/plain");

        mock.mock_network_response("https://api.example.com/test", response)?;

        let retrieved = mock.get_network_response("https://api.example.com/test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().body, "Hello, World!");

        Ok(())
    }

    #[test]
    fn test_env_var_mocking() -> Result<()> {
        let mock = MockCapabilities::new();

        mock.mock_env_var("TEST_VAR", "test_value")?;

        let value = mock.get_env_var("TEST_VAR");
        assert_eq!(value, Some("test_value".to_string()));

        Ok(())
    }

    #[test]
    fn test_time_mocking() -> Result<()> {
        let mock = MockCapabilities::new();

        let fixed_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1234567890);
        mock.mock_fixed_time(fixed_time)?;

        let retrieved = mock.get_fixed_time();
        assert_eq!(retrieved, Some(fixed_time));

        mock.clear_fixed_time()?;
        let cleared = mock.get_fixed_time();
        assert_eq!(cleared, None);

        Ok(())
    }

    #[test]
    fn test_clear_all_mocks() -> Result<()> {
        let mock = MockCapabilities::new();

        // Add some mocks
        mock.mock_env_var("TEST", "value")?;
        mock.mock_external_file("/tmp/test", "content")?;
        mock.mock_network_response("https://example.com", MockNetworkResponse::success("OK"))?;

        // Verify they exist
        assert!(mock.get_env_var("TEST").is_some());
        assert!(mock.get_external_file("/tmp/test").is_some());
        assert!(mock.get_network_response("https://example.com").is_some());

        // Clear all
        mock.clear_all_mocks()?;

        // Verify they're cleared
        assert!(mock.get_env_var("TEST").is_none());
        assert!(mock.get_external_file("/tmp/test").is_none());
        assert!(mock.get_network_response("https://example.com").is_none());

        Ok(())
    }

    #[test]
    fn test_mock_builder() -> Result<()> {
        let commit = MockBuilder::commit("abc123", "Test Author", "Test message");
        assert_eq!(commit.sha, "abc123");
        assert_eq!(commit.author, "Test Author");
        assert_eq!(commit.message, "Test message");

        let branch = MockBuilder::branch("main", vec![commit]);
        assert_eq!(branch.name, "main");
        assert_eq!(branch.commits.len(), 1);
        assert!(branch.exists);

        Ok(())
    }
}
