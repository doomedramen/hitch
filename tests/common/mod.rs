//! Common test utilities for all Hitch tests

use anyhow::Result;
use git2::{Repository, Signature};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Test environment with complete git2-based isolation
pub struct TestEnv {
    temp_dir: TempDir,
    pub(crate) original_dir: std::path::PathBuf,
    _repo: Option<Repository>, // Used for drop behavior but not directly accessed
}

impl TestEnv {
    /// Create a new isolated test environment with git2 repository
    #[allow(dead_code)] // Used by tests that haven't migrated to closure framework yet
    pub fn new() -> Result<Self> {
        Self::new_with_git(true)
    }

    /// Create a new isolated test environment, optionally with git repository
    pub fn new_with_git(with_git: bool) -> Result<Self> {
        // Create unique temp directory with thread ID, timestamp, and random component
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(); // Use microseconds for better uniqueness

        let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
        let thread_id = std::thread::current().id();
        let thread_str = format!("{:?}", thread_id);
        let temp_dir = tempfile::Builder::new()
            .prefix(&format!(
                "hitch_test_{}_{}_{}",
                timestamp, counter, thread_str
            ))
            .tempdir()?;

        let original_dir = std::env::current_dir()?;

        let repo = if with_git {
            Some(Self::setup_git_repository(temp_dir.path())?)
        } else {
            None
        };

        Ok(TestEnv {
            temp_dir,
            original_dir,
            _repo: repo,
        })
    }

    /// Setup a git repository using git2 library
    fn setup_git_repository(repo_path: &Path) -> Result<Repository> {
        // Initialize repository
        let repo = Repository::init(repo_path)?;

        // Create signature for commits
        let sig = Signature::now("Test User", "test@example.com")?;

        // Create initial commit on main branch
        {
            let mut index = repo.index()?;
            let readme_path = repo_path.join("README.md");
            fs::write(&readme_path, "# Test Repository")?;
            index.add_path(Path::new("README.md"))?;
            let tree_id = index.write_tree()?;
            let tree = repo.find_tree(tree_id)?;

            // Set HEAD to point to main branch before creating commit
            repo.set_head("refs/heads/main")?;

            // Create initial commit on main branch
            let _initial_commit =
                repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        } // tree is dropped here, releasing the borrow on repo

        Ok(repo)
    }

    /// Create a complete test environment with everything needed for Hitch testing
    pub fn setup_complete_hitch_env(&self) -> Result<()> {
        // Change to the test directory
        std::env::set_current_dir(self.path())?;

        // Set git config using command-line git (hitch needs this)
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .output()?;
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .output()?;
        Command::new("git")
            .args(["config", "core.autocrlf", "false"])
            .output()?;
        Command::new("git")
            .args(["config", "core.filemode", "false"])
            .output()?;

        // Ensure working tree is clean (git2 setup might leave uncommitted changes)
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(self.path())
            .output()?;
        let status_output = String::from_utf8_lossy(&output.stdout);
        if !status_output.trim().is_empty() {
            Command::new("git")
                .args(["add", "."])
                .current_dir(self.path())
                .output()?;
            Command::new("git")
                .args(["commit", "-m", "Clean up initial setup"])
                .current_dir(self.path())
                .output()?;
        }

        // Initialize Hitch
        let output = Command::new(self.hitch_binary()).args(["init"]).output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to run hitch init: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Clean up any remaining changes from hitch init
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(self.path())
            .output()?;
        let status_output = String::from_utf8_lossy(&output.stdout);
        if !status_output.trim().is_empty() {
            Command::new("git")
                .args(["add", "."])
                .current_dir(self.path())
                .output()?;
            Command::new("git")
                .args(["commit", "-m", "Clean up after hitch init"])
                .current_dir(self.path())
                .output()?;
        }

        Ok(())
    }

    /// Get the path to the test directory
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Get the absolute path to the hitch binary
    pub fn hitch_binary(&self) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("hitch")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Return to original directory first
        let _ = std::env::set_current_dir(&self.original_dir);

        // TempDir automatically cleans up when it goes out of scope
    }
}

/// Setup level for test environments
#[derive(Debug, Clone, Copy)]
pub enum SetupLevel {
    /// Basic - minimal setup (alias for GitOnly)
    #[allow(dead_code)] // Used by tests that haven't migrated to closure framework yet
    Basic,
    /// Git only - basic git repository setup
    #[allow(dead_code)] // Used by tests that haven't migrated to closure framework yet
    GitOnly,
    /// Complete setup - git repository + hitch initialized
    #[allow(dead_code)] // Used by tests that haven't migrated to closure framework yet
    Complete,
}

/// Run a test with a managed test environment
/// This function handles the creation and cleanup of the test environment
/// and ensures proper setup based on the specified level
#[allow(dead_code)] // Used by multiple test files
pub fn with_test_env<F>(level: SetupLevel, test_fn: F) -> Result<()>
where
    F: FnOnce(&TestEnv) -> Result<()>,
{
    let original_dir = std::env::current_dir()?;

    let test_env = match level {
        SetupLevel::Basic => TestEnv::new_with_git(false)?,
        SetupLevel::GitOnly | SetupLevel::Complete => TestEnv::new_with_git(true)?,
    };

    // Ensure we're in the test directory before running any setup
    std::env::set_current_dir(test_env.path())?;

    // Setup based on level
    let setup_result = match level {
        SetupLevel::Basic => {
            // No setup needed - just a plain temp directory
            Ok(())
        }
        SetupLevel::GitOnly => {
            // Basic git setup is already done in TestEnv::new()
            // Just need to ensure git config is set for hitch operations
            Command::new("git")
                .args(["config", "user.name", "Test User"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["config", "core.autocrlf", "false"])
                .current_dir(test_env.path())
                .output()?;
            Command::new("git")
                .args(["config", "core.filemode", "false"])
                .current_dir(test_env.path())
                .output()?;

            // Ensure working tree is clean (git2 setup might leave uncommitted changes)
            let output = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(test_env.path())
                .output()?;
            let status_output = String::from_utf8_lossy(&output.stdout);
            if !status_output.trim().is_empty() {
                // Add all changes including deleted files
                let add_output = Command::new("git")
                    .args(["add", "-A"])
                    .current_dir(test_env.path())
                    .output()?;
                if !add_output.status.success() {
                    return Err(anyhow::anyhow!(
                        "Failed to add files: {}",
                        String::from_utf8_lossy(&add_output.stderr)
                    ));
                }
                let commit_output = Command::new("git")
                    .args(["commit", "-m", "Clean up initial setup"])
                    .current_dir(test_env.path())
                    .output()?;
                if !commit_output.status.success() {
                    let stderr = String::from_utf8_lossy(&commit_output.stderr);
                    let stdout = String::from_utf8_lossy(&commit_output.stdout);
                    // Don't treat "nothing to commit" as an error
                    if !(stderr.contains("nothing to commit")
                        || stdout.contains("nothing to commit"))
                    {
                        return Err(anyhow::anyhow!(
                            "Failed to commit: stderr={}, stdout={}",
                            stderr,
                            stdout
                        ));
                    }
                }
            }
            Ok(())
        }
        SetupLevel::Complete => test_env.setup_complete_hitch_env(),
    };

    // If setup failed, return to original directory and return error
    if let Err(e) = setup_result {
        let _ = std::env::set_current_dir(&original_dir);
        return Err(e);
    }

    // Run the test function
    let test_result = test_fn(&test_env);

    // Always return to original directory
    let _ = std::env::set_current_dir(&original_dir);

    test_result
}
