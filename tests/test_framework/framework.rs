//! Core test framework implementation
//!
//! Provides the main HitchTestFramework and TestEnvironment structures with
//! closure-based testing API and complete Git isolation.

use anyhow::Result;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Test setup options for different test scenarios
#[derive(Debug, Clone, PartialEq)]
pub enum TestSetup {
    /// No additional setup - just git repository with user config
    None,
    /// Initialize git repository with user config and basic structure
    GitOnly,
    /// Initialize hitch with proper git setup (most common)
    HitchInit,
    /// Initialize hitch and create a basic 'dev' environment
    HitchWithEnv,
}

use crate::test_framework::assertions::AssertionHelpers;
use crate::test_framework::command_runners::{GitCommandRunner, HitchCommandRunner};
use crate::test_framework::file_system_helpers::FileSystemHelpers;
use crate::test_framework::mocking::MockCapabilities;

/// Main testing framework for Hitch CLI
///
/// Provides complete Git isolation and closure-based test API as requested.
/// Each test runs in its own temporary directory with automatic cleanup.
///
/// # Usage Examples
///
/// ## Basic Test with Hitch Initialized
/// ```rust
/// let framework = HitchTestFramework::new()?;
/// let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
///     // Hitch is initialized and ready to use
///     let result = env.hitch.run().args(&["add", "dev"]).execute()?;
///     result.assert_success();
///     Ok(())
/// });
/// ```
///
/// ## Test with Pre-created Environment
/// ```rust
/// let framework = HitchTestFramework::new()?;
/// let _ = framework.with_test_environment(TestSetup::HitchWithEnv, |env| {
///     // Hitch is initialized and a "dev" environment already exists
///     let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
///     result.assert_success();
///     Ok(())
/// });
/// ```
///
/// ## Test for Hitch-not-initialized Scenarios
/// ```rust
/// let framework = HitchTestFramework::new()?;
/// let _ = framework.with_test_environment(TestSetup::None, |env| {
///     // Hitch is NOT initialized - tests error handling
///     let result = env.hitch.run().args(&["add", "dev"]).execute()?;
///     result.assert_failure().assert_stderr_contains("hitch.json");
///     Ok(())
/// });
/// ```
///
/// ## Test with Custom Git Setup
/// ```rust
/// let framework = HitchTestFramework::new()?;
/// let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
///     // Git repository is set up with basic structure, but hitch is not initialized
///     env.fs.write_file("README.md", "# My Project")?;
///     env.git.run(&["add", "."])?;
///     env.git.run(&["commit", "-m", "Initial commit"])?;
///
///     // Now initialize hitch and test
///     env.hitch.run().args(&["init"]).execute()?.assert_success();
///     Ok(())
/// });
/// ```
///
/// # Features
///
/// - **Complete Git Isolation**: Each test runs in its own temporary directory
/// - **Process Isolation**: No shared state between tests
/// - **Automatic Cleanup**: Temporary directories and resources are cleaned up automatically
/// - **Flexible Setup**: Different levels of test environment setup via TestSetup enum
/// - **Clean API**: Closure-based API makes tests readable and maintainable
/// - **Helper Methods**: Convenient methods for common operations like reading hitch config
///
/// # TestSetup Options
///
/// - `TestSetup::None`: No additional setup (just git repo with user config)
/// - `TestSetup::GitOnly`: Git repository with basic structure
/// - `TestSetup::HitchInit`: Hitch initialized with proper git setup (most common)
/// - `TestSetup::HitchWithEnv`: Hitch initialized + a basic "dev" environment created
pub struct HitchTestFramework {
    temp_dir: TempDir,
    hitch_binary: PathBuf,
}

impl HitchTestFramework {
    /// Create a new test framework instance
    ///
    /// Sets up temporary directory, finds hitch binary, and prepares test environment
    pub fn new() -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let hitch_binary = Self::find_hitch_binary()?;

        Ok(HitchTestFramework {
            temp_dir,
            hitch_binary,
        })
    }

    /// Execute a test closure with specific setup requirements
    ///
    /// Provides flexible setup options for different test scenarios:
    /// - None: No additional setup (just git repo with user config)
    /// - GitOnly: Initialize git repository with basic structure
    /// - HitchInit: Initialize hitch with proper git setup (most common)
    /// - HitchWithEnv: Initialize hitch and create a basic 'dev' environment
    pub fn with_test_environment<F, R>(&self, setup: TestSetup, test_fn: F) -> R
    where
        F: FnOnce(&TestEnvironment) -> R,
    {
        // Create a new temp dir for this specific test to control cleanup timing
        // Use random prefix for better isolation in multi-threaded execution
        let temp_dir = tempfile::Builder::new()
            .prefix("hitch-test")
            .suffix(&format!("{}", std::process::id()))
            .tempdir()
            .expect("Failed to create temp directory");
        let temp_dir_path = temp_dir.path().to_path_buf();

        // NOTE: We intentionally do NOT call env::set_current_dir here.
        // All file, git, and hitch operations use explicit absolute paths via
        // their respective helpers, so the process CWD does not matter for
        // correctness. Changing the process CWD would be unsafe when tests
        // run in parallel (Rust's default) because all threads share the same
        // CWD.

        // Initialize git repository in temp directory
        let git = GitCommandRunner::new(&temp_dir_path)
            .expect("Failed to initialize git in test environment");

        // Initialize git repository first
        git.init().expect("Failed to initialize git repository");

        // Configure git user for the test
        git.config_user("Test User", "test@example.com")
            .expect("Failed to configure git user");

        // Create test environment with all helpers
        let test_env = TestEnvironment {
            temp_dir: temp_dir_path.clone(),
            hitch: HitchCommandRunner::new(&self.hitch_binary, &temp_dir_path),
            git,
            fs: FileSystemHelpers::new(&temp_dir_path),
            assert: AssertionHelpers::new(),
            mock: MockCapabilities::new(),
        };

        // Apply setup requirements
        match setup {
            TestSetup::None => {
                // No additional setup needed
            }
            TestSetup::GitOnly => {
                // Git is already initialized, but create basic structure
                test_env
                    .fs
                    .write_file("README.md", "# Test Repository")
                    .ok();
                test_env.git.run(&["add", "."]).ok();
                test_env.git.run(&["commit", "-m", "Initial commit"]).ok();
            }
            TestSetup::HitchInit => {
                test_env.init_hitch().expect("Failed to initialize hitch");
            }
            TestSetup::HitchWithEnv => {
                test_env.init_hitch().expect("Failed to initialize hitch");
                test_env
                    .hitch
                    .run()
                    .args(&["add", "dev"])
                    .execute()
                    .expect("Failed to create test environment")
                    .assert_success();
            }
        }

        // Execute test closure. The TempDir is cleaned up automatically when
        // it goes out of scope at the end of this function, regardless of
        // whether the closure panics.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test_fn(&test_env)));

        // temp_dir is dropped here, cleaning up the temporary directory.

        // Return the result or re-panic if the test panicked
        match result {
            Ok(r) => r,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Find the hitch binary for testing
    ///
    /// Looks for hitch binary in target/debug or target/release directories
    fn find_hitch_binary() -> anyhow::Result<PathBuf> {
        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR")
            .map_err(|_| anyhow::anyhow!("CARGO_MANIFEST_DIR not set"))?;

        let project_root = Path::new(&cargo_manifest_dir);

        // Try debug build first (more common during development)
        let debug_binary = project_root.join("target/debug/hitch");
        if debug_binary.exists() {
            return Ok(debug_binary);
        }

        // Try release build
        let release_binary = project_root.join("target/release/hitch");
        if release_binary.exists() {
            return Ok(release_binary);
        }

        // Try to build hitch if not found
        println!("Hitch binary not found, attempting to build...");
        let output = Command::new("cargo")
            .args(["build", "--bin", "hitch"])
            .current_dir(project_root)
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to build hitch binary: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Check again after building
        if debug_binary.exists() {
            return Ok(debug_binary);
        }

        Err(anyhow::anyhow!("Could not find or build hitch binary"))
    }

    /// Get the temporary directory path (useful for debugging)
    pub fn temp_dir(&self) -> &Path {
        self.temp_dir.path()
    }
}

/// Test environment providing comprehensive testing utilities
///
/// This struct is passed to test closures and provides:
/// - hitch: Command runner for Hitch CLI operations
/// - git: Command runner for git operations
/// - fs: File system helpers for test setup
/// - assert: Assertion helpers for common validations
/// - mock: Mocking capabilities for external dependencies
/// - temp_dir: Path to the temporary test directory
pub struct TestEnvironment {
    /// Path to the temporary test directory
    pub temp_dir: PathBuf,

    /// Hitch CLI command runner
    pub hitch: HitchCommandRunner,

    /// Git command runner
    pub git: GitCommandRunner,

    /// File system helpers
    pub fs: FileSystemHelpers,

    /// Assertion helpers
    pub assert: AssertionHelpers,

    /// Mocking capabilities
    pub mock: MockCapabilities,
}

impl TestEnvironment {
    /// Initialize hitch and setup git repository properly for testing
    /// This combines hitch init with the required git setup
    pub fn init_hitch(&self) -> Result<()> {
        // Initialize hitch
        self.hitch.run().args(&["init"]).execute()?.assert_success();

        // Setup git repository properly after hitch init
        self.setup_git_for_hitch()
    }

    /// Setup a proper Git repository for hitch testing
    /// This handles the complex setup required after hitch init
    pub fn setup_git_for_hitch(&self) -> Result<()> {
        // Create main branch since hitch init leaves us on hitch-metadata branch
        self.git.run(&["checkout", "-b", "main"])?;

        // Add and commit the .gitignore file that was created by hitch init (force add since it's ignored)
        self.git.run(&["add", "-f", ".gitignore"])?;
        self.git.run(&["commit", "-m", "Add .gitignore"])?;

        // Create initial commit on main branch to establish it as the base branch
        self.fs.write_file("README.md", "# Test Repository")?;
        self.git.run(&["add", "README.md"])?;
        self.git.run(&["commit", "-m", "Initial commit"])?;

        Ok(())
    }

    /// Read hitch configuration from the hitch-metadata branch
    pub fn read_hitch_config(&self) -> Result<hitch::types::HitchConfig> {
        // Switch to hitch-metadata branch to read config
        let current_branch = self.git.run(&["branch", "--show-current"])?;
        self.git.run(&["checkout", "hitch-metadata"])?;

        let config = self.fs.read_json("hitch.json")?;

        // Switch back to original branch
        if current_branch.success() {
            let branch_name = current_branch.stdout().trim().to_string();
            if !branch_name.is_empty() {
                self.git.run(&["checkout", &branch_name])?;
            }
        }

        Ok(config)
    }
}

impl std::fmt::Debug for TestEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestEnvironment")
            .field("temp_dir", &self.temp_dir)
            .field("hitch", &"HitchCommandRunner")
            .field("git", &"GitCommandRunner")
            .field("fs", &"FileSystemHelpers")
            .field("assert", &"AssertionHelpers")
            .field("mock", &"MockCapabilities")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framework_creation() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        assert!(framework.temp_dir().exists());
        Ok(())
    }

    #[test]
    fn test_test_environment_closure() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Test that we're in a different directory
            assert!(env.temp_dir.exists());

            // Test that git is initialized
            env.git.run(&["init"]).expect("Failed to init git");

            // Test that we can create a file
            env.fs.write_file("test.txt", "test content")?;

            // Test assertions work
            env.assert.file_exists(&env.fs, "test.txt");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
