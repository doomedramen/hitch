use std::path::PathBuf;
use std::process::Command;
use tempfile::{TempDir, Builder};
use anyhow::{Result, Context};

/// Isolated test environment for Hitch testing
pub struct TestEnvironment {
    pub temp_dir: TempDir,
    pub repo_path: PathBuf,
}

impl TestEnvironment {
    /// Create a fresh isolated git repository for testing
    pub fn new() -> Result<Self> {
        let temp_dir = Builder::new()
            .prefix("hitch-test-")
            .tempdir()
            .context("Failed to create temporary directory")?;

        let repo_path = temp_dir.path().to_path_buf();

        // Initialize git repository
        Command::new("git")
            .args(&["init"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to initialize git repository")?;

        // Configure git user
        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to set git user name")?;

        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to set git user email")?;

        // Create initial commit
        std::fs::write(repo_path.join("README.md"), "# Test Repository\n")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to add README.md")?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .context("Failed to create initial commit")?;

        Ok(TestEnvironment {
            temp_dir,
            repo_path,
        })
    }

    /// Get the path to the test repository
    pub fn path(&self) -> &PathBuf {
        &self.repo_path
    }

    /// Execute hitch command in the test environment
    pub fn run_hitch(&self, args: &[&str]) -> Result<String> {
        let hitch_path = std::env::current_dir()?
            .join("target")
            .join("debug")
            .join("hitch");

        let mut cmd = Command::new(&hitch_path.display().to_string());
        cmd.args(args);
        cmd.current_dir(&self.repo_path);

        let output = cmd.output()
            .context("Failed to run hitch command")?;

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Check if a file exists in the test repository
    pub fn file_exists(&self, path: &str) -> bool {
        self.repo_path.join(path).exists()
    }

    /// Read file contents from test repository
    pub fn read_file(&self, path: &str) -> Result<String> {
        let content = std::fs::read_to_string(self.repo_path.join(path))
            .context(format!("Failed to read file: {}", path))?;
        Ok(content)
    }

    /// Write file contents to test repository
    pub fn write_file(&self, path: &str, content: &str) -> Result<()> {
        std::fs::write(self.repo_path.join(path), content)
            .context(format!("Failed to write file: {}", path))?;
        Ok(())
    }

    /// Get current git branch
    pub fn current_branch(&self) -> Result<String> {
        let output = Command::new("git")
            .args(&["branch", "--show-current"])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to get current branch")?;

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    /// Check if a branch exists
    pub fn branch_exists(&self, branch: &str) -> bool {
        Command::new("git")
            .args(&["branch", "--list", branch])
            .current_dir(&self.repo_path)
            .output()
            .map(|output| !output.stdout.is_empty())
            .unwrap_or(false)
    }

    /// Create and checkout a new branch
    pub fn create_branch(&self, branch: &str) -> Result<()> {
        Command::new("git")
            .args(&["checkout", "-b", branch])
            .current_dir(&self.repo_path)
            .output()
            .context(format!("Failed to create branch: {}", branch))?;
        Ok(())
    }

    /// Switch to a branch
    pub fn checkout_branch(&self, branch: &str) -> Result<()> {
        Command::new("git")
            .args(&["checkout", branch])
            .current_dir(&self.repo_path)
            .output()
            .context(format!("Failed to checkout branch: {}", branch))?;
        Ok(())
    }

    /// Check if working directory is clean
    pub fn is_clean(&self) -> bool {
        Command::new("git")
            .args(&["status", "--porcelain"])
            .current_dir(&self.repo_path)
            .output()
            .map(|output| output.stdout.is_empty())
            .unwrap_or(false)
    }

    /// Add and commit changes
    pub fn commit(&self, message: &str) -> Result<()> {
        Command::new("git")
            .args(&["add", "."])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to add files")?;

        Command::new("git")
            .args(&["commit", "-m", message])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to commit changes")?;

        Ok(())
    }

    /// Get commit history
    pub fn log(&self, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new("git");
        cmd.args(&["log"]);
        cmd.args(args);
        cmd.current_dir(&self.repo_path);

        let output = cmd.output()
            .context("Failed to get git log")?;

        Ok(String::from_utf8(output.stdout)?)
    }
}