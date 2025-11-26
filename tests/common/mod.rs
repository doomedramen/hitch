use std::path::Path;
use std::fs;
use std::process::Command;
use tempfile::TempDir;
use anyhow::Result;

/// Test environment setup levels
pub enum SetupLevel {
    Basic,      // Just a temp directory
    GitOnly,    // Temp directory + git repo (no hitch init)
}

/// Test environment for isolated test runs
pub struct TestEnv {
    temp_dir: TempDir,
    original_dir: std::path::PathBuf,
    _repo: Option<git2::Repository>,
}

impl TestEnv {
    /// Create a new test environment with specified setup level
    pub fn new(level: SetupLevel) -> Result<Self> {
        Self::with_setup(level)
    }

    /// Create test environment with specific setup
    fn with_setup(level: SetupLevel) -> Result<Self> {
        let test_env = Self::new_with_git(matches!(level, SetupLevel::GitOnly))?;

        // Setup based on level
        let setup_result: Result<()> = match level {
            SetupLevel::Basic => {
                // No setup needed - just a plain temp directory
                Ok(())
            }
            SetupLevel::GitOnly => {
                // Basic git setup is already done in TestEnv::new()
                // Just need to ensure git config is set for hitch operations
                let config_result = Command::new("git")
                    .args(["config", "user.name", "Test User"])
                    .current_dir(test_env.path())
                    .output()?;
                if !config_result.status.success() {
                    return Err(anyhow::anyhow!("Failed to set git user name"));
                }

                let email_result = Command::new("git")
                    .args(["config", "user.email", "test@example.com"])
                    .current_dir(test_env.path())
                    .output()?;
                if !email_result.status.success() {
                    return Err(anyhow::anyhow!("Failed to set git user email"));
                }

                Ok(())
            }
        };

        setup_result?;

        Ok(test_env)
    }

    /// Create a new test environment with optional git setup
    fn new_with_git(with_git: bool) -> Result<Self> {
        // Create unique temp directory with thread ID, timestamp, and random component
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros();

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
    fn setup_git_repository(repo_path: &Path) -> Result<git2::Repository> {
        // Initialize repository
        let repo = git2::Repository::init(repo_path)?;

        // Create signature for commits
        let sig = git2::Signature::now("Test User", "test@example.com")?;

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

            // Clear the index to ensure clean working tree
            index.clear()?;
        }

        Ok(repo)
    }

    /// Get the path to the test directory
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Get the path to the hitch binary
    pub fn hitch_binary(&self) -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join("release")
            .join("hitch")
    }

    /// Change to the test directory
    pub fn cd_to_test_dir(&self) -> Result<()> {
        std::env::set_current_dir(self.path())?;
        Ok(())
    }

    /// Restore the original directory
    pub fn restore_original_dir(&self) -> Result<()> {
        std::env::set_current_dir(&self.original_dir)?;
        Ok(())
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = self.restore_original_dir();
    }
}

/// Helper function to run tests with isolated environments
pub fn with_test_env<F>(level: SetupLevel, test_fn: F) -> Result<()>
where
    F: FnOnce(&TestEnv) -> Result<()>,
{
    let test_env = TestEnv::new(level)?;
    test_env.cd_to_test_dir()?;

    let result = test_fn(&test_env);

    // Restore original directory before cleanup
    let _ = test_env.restore_original_dir();

    result
}