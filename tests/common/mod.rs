//! Common test utilities for all Hitch tests

use anyhow::Result;
use std::fs;
use std::process::Command;
use std::path::Path;
use tempfile::TempDir;
use git2::{Repository, Signature};

/// Test environment with complete git2-based isolation
pub struct TestEnv {
    temp_dir: TempDir,
    original_dir: std::path::PathBuf,
    _repo: Repository, // Used for drop behavior but not directly accessed
}

impl TestEnv {
    /// Create a new isolated test environment with git2 repository
    pub fn new() -> Result<Self> {
        // Create unique temp directory with thread ID and timestamp
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis();

        let thread_id = std::thread::current().id();
        let thread_str = format!("{:?}", thread_id);
        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("hitch_test_{}_{}", timestamp, thread_str))
            .tempdir()?;

        let original_dir = std::env::current_dir()?;

        
        // Create git repository using git2
        let repo = Self::setup_git_repository(temp_dir.path())?;

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

            let initial_commit = repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Initial commit",
                &tree,
                &[]
            )?;

            // Set up main branch explicitly
            repo.branch("main", &repo.find_commit(initial_commit)?, true)?;
        } // tree is dropped here, releasing the borrow on repo

        Ok(repo)
    }

    /// Create a complete test environment with everything needed for Hitch testing
    pub fn setup_complete_hitch_env(&self) -> Result<()> {
        // Change to the test directory
        std::env::set_current_dir(self.path())?;

        // Set git config using command-line git (hitch needs this)
        Command::new("git").args(&["config", "user.name", "Test User"]).output()?;
        Command::new("git").args(&["config", "user.email", "test@example.com"]).output()?;
        Command::new("git").args(&["config", "core.autocrlf", "false"]).output()?;
        Command::new("git").args(&["config", "core.filemode", "false"]).output()?;

        // Ensure working tree is clean (git2 setup might leave uncommitted changes)
        let output = Command::new("git").args(&["status", "--porcelain"]).output()?;
        let status_output = String::from_utf8_lossy(&output.stdout);
        if !status_output.trim().is_empty() {
            Command::new("git").args(&["add", "."]).output()?;
            Command::new("git").args(&["commit", "-m", "Clean up initial setup"]).output()?;
        }

        // Initialize Hitch
        let output = Command::new(&self.hitch_binary())
            .args(&["init"])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to run hitch init: {}", String::from_utf8_lossy(&output.stderr)));
        }

        // Clean up any remaining changes from hitch init
        let output = Command::new("git").args(&["status", "--porcelain"]).output()?;
        let status_output = String::from_utf8_lossy(&output.stdout);
        if !status_output.trim().is_empty() {
            Command::new("git").args(&["add", "."]).output()?;
            Command::new("git").args(&["commit", "-m", "Clean up after hitch init"]).output()?;
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

