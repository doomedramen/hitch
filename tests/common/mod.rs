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
    repo: Repository,
    original_dir: std::path::PathBuf,
    hitch_binary_path: std::path::PathBuf,
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

        // Get absolute path to hitch binary before we change directories
        let hitch_binary_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("hitch")
            .canonicalize()?;

        // Create git repository using git2
        let repo = Self::setup_git_repository(temp_dir.path())?;

        Ok(TestEnv {
            temp_dir,
            repo,
            original_dir,
            hitch_binary_path,
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
        let output = Command::new(&self.hitch_binary_path)
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

    /// Create a branch in the test repository with commit using git2
    pub fn create_branch(&self, branch_name: &str) -> Result<()> {
        // Get current HEAD commit
        let head_commit = self.repo.head()?.peel_to_commit()?;
        let sig = Signature::now("Test User", "test@example.com")?;

        // Create a new branch from HEAD
        let _branch_ref = self.repo.branch(branch_name, &head_commit, false)?;
        let branch_ref_name = format!("refs/heads/{}", branch_name);

        // Create a file and commit on the new branch
        let file_path = self.path().join(format!("{}.md", branch_name));
        println!("Creating file at: {:?}", file_path);
        fs::write(&file_path, format!("Content for {} branch", branch_name))?;

        // Create index for the new commit
        let mut index = self.repo.index()?;
        index.add_path(Path::new(&format!("{}.md", branch_name)))?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;

        // Create commit on the branch
        self.repo.commit(
            Some(&branch_ref_name),
            &sig,
            &sig,
            &format!("Add {} branch", branch_name),
            &tree,
            &[&head_commit]
        )?;

        Ok(())
    }

    /// Create test branches for comprehensive testing
    pub fn create_test_branches(&self) -> Result<()> {
        self.create_branch("feature/user-auth")?;
        self.create_branch("feature/api-endpoints")?;
        self.create_branch("dev")?;
        self.create_branch("qa")?;

        // Switch back to main
        Command::new("git").args(&["checkout", "main"]).output()?;

        Ok(())
    }

    /// Get the path to the test directory
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Get the absolute path to the hitch binary
    pub fn hitch_binary(&self) -> &std::path::Path {
        &self.hitch_binary_path
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Return to original directory first
        let _ = std::env::set_current_dir(&self.original_dir);

        // TempDir automatically cleans up when it goes out of scope
    }
}

/// Simple temp directory setup for basic isolation
pub fn setup_temp_dir() -> (TempDir, std::path::PathBuf) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let path = temp_dir.path().to_path_buf();
    (temp_dir, path)
}