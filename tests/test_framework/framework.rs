//! Core test framework implementation
//!
//! Provides the main HitchTestFramework and TestEnvironment structures with
//! closure-based testing API and complete Git isolation.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

use crate::test_framework::assertions::AssertionHelpers;
use crate::test_framework::command_runners::{GitCommandRunner, HitchCommandRunner};
use crate::test_framework::file_system_helpers::FileSystemHelpers;
use crate::test_framework::mocking::MockCapabilities;

/// Main testing framework for Hitch CLI
///
/// Provides complete Git isolation and closure-based test API as requested.
/// Each test runs in its own temporary directory with automatic cleanup.
pub struct HitchTestFramework {
    temp_dir: TempDir,
    original_cwd: PathBuf,
    hitch_binary: PathBuf,
}

impl HitchTestFramework {
    /// Create a new test framework instance
    ///
    /// Sets up temporary directory, finds hitch binary, and prepares test environment
    pub fn new() -> anyhow::Result<Self> {
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        let hitch_binary = Self::find_hitch_binary()?;

        Ok(HitchTestFramework {
            temp_dir,
            original_cwd,
            hitch_binary,
        })
    }

    /// Execute a test closure with an isolated test environment
    ///
    /// This is the core closure-based API as requested:
    /// - Creates temp directory
    /// - Changes directory to temp location
    /// - Provides hitch and git command access
    /// - Automatic cleanup via Drop implementation
    pub fn with_test_environment<F, R>(&self, test_fn: F) -> R
    where
        F: FnOnce(&TestEnvironment) -> R,
    {
        // Change to temporary directory
        env::set_current_dir(self.temp_dir.path()).expect("Failed to change to temp directory");

        // Initialize git repository in temp directory
        let git = GitCommandRunner::new(self.temp_dir.path())
            .expect("Failed to initialize git in test environment");

        // Initialize git repository first
        git.init().expect("Failed to initialize git repository");

        // Configure git user for the test
        git.config_user("Test User", "test@example.com")
            .expect("Failed to configure git user");

        // Create test environment with all helpers
        let test_env = TestEnvironment {
            temp_dir: self.temp_dir.path().to_path_buf(),
            hitch: HitchCommandRunner::new(&self.hitch_binary),
            git,
            fs: FileSystemHelpers::new(self.temp_dir.path()),
            assert: AssertionHelpers::new(),
            mock: MockCapabilities::new(),
        };

        // Execute test closure
        let result = test_fn(&test_env);

        // Restore original working directory
        env::set_current_dir(&self.original_cwd).expect("Failed to restore original directory");

        result
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

        let _ = framework.with_test_environment(|env| {
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
