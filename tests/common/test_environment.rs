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
        Self::new_with_base(false)
    }

    /// Create a fresh isolated git repository with optional base structure
    pub fn new_with_base(with_base_structure: bool) -> Result<Self> {
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

        // Create base repository structure if requested
        if with_base_structure {
            Self::create_base_structure(&repo_path)?;
        }

        Ok(TestEnvironment {
            temp_dir,
            repo_path,
        })
    }

    /// Create a realistic base repository structure for testing
    fn create_base_structure(repo_path: &PathBuf) -> Result<()> {
        // Create some feature branches with realistic content
        let branches = vec![
            ("feature/login", "feat: Add login functionality\n- User authentication\n- Session management\n- Login form UI"),
            ("feature/payment", "feat: Add payment processing\n- Credit card integration\n- Payment gateway API\n- Billing interface"),
            ("feature/dashboard", "feat: Implement admin dashboard\n- User management\n- Analytics view\n- System settings"),
            ("dev", "Development branch with integrated features\n- Latest feature merges\n- Development configurations\n- Testing setup"),
            ("staging", "Staging environment preparation\n- Pre-production features\n- Performance optimizations\n- Security hardening"),
        ];

        for (branch_name, content) in branches {
            // Create and checkout branch
            Command::new("git")
                .args(&["checkout", "-b", branch_name])
                .current_dir(repo_path)
                .output()
                .context(format!("Failed to create branch: {}", branch_name))?;

            // Create feature-specific file
            let file_content = format!(
                "# {}\n\n{}\n\n## Implementation Details\n\n- Added in this branch\n- Ready for deployment\n- Includes tests\n",
                branch_name, content
            );

            let file_name = format!("{}.md", branch_name.replace("/", "_"));
            std::fs::write(repo_path.join(&file_name), file_content)?;

            // Also create a shared config file in some branches
            if branch_name == "dev" || branch_name == "staging" {
                let config_content = format!(
                    "# Configuration for {}\n\nenvironment=\"{}\"\ndebug={}\ndatabase_url=\"localhost\"\n",
                    branch_name,
                    branch_name,
                    if branch_name == "dev" { "true" } else { "false" }
                );
                std::fs::write(repo_path.join("config.toml"), config_content)?;
            }

            // Commit the changes
            Command::new("git")
                .args(&["add", "."])
                .current_dir(repo_path)
                .output()
                .context("Failed to add files")?;

            Command::new("git")
                .args(&["commit", "-m", &format!("Add {} branch functionality", branch_name)])
                .current_dir(repo_path)
                .output()
                .context("Failed to commit changes")?;
        }

        // Return to main branch
        Command::new("git")
            .args(&["checkout", "main"])
            .current_dir(repo_path)
            .output()
            .context("Failed to return to main branch")?;

        Ok(())
    }

    /// Get the path to the test repository
    pub fn path(&self) -> &PathBuf {
        &self.repo_path
    }

    /// Create a non-git directory for testing error cases
    pub fn new_non_git() -> Result<Self> {
        let temp_dir = Builder::new()
            .prefix("hitch-test-non-git-")
            .tempdir()
            .context("Failed to create temporary directory")?;

        let repo_path = temp_dir.path().to_path_buf();

        // Create some basic files but don't initialize git
        std::fs::write(repo_path.join("README.md"), "# Test Repository\n")?;
        std::fs::write(repo_path.join("some_file.txt"), "This is not a git repository\n")?;

        Ok(TestEnvironment {
            temp_dir,
            repo_path,
        })
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

        // Combine stdout and stderr for complete output
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;

        if !stderr.is_empty() {
            Ok(format!("{}\n{}", stdout, stderr))
        } else {
            Ok(stdout)
        }
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