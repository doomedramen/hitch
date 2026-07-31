// Command runners for Hitch and Git operations
//
// Provides fluent API for running hitch and git commands in tests with
// proper error handling and output capture.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Output};

/// Command runner for Hitch CLI operations
///
/// Provides fluent API for running hitch commands in tests with proper
/// argument handling, output capture, and error management.
#[derive(Debug, Clone)]
pub struct HitchCommandRunner {
    binary_path: std::path::PathBuf,
    current_dir: std::path::PathBuf,
}

impl HitchCommandRunner {
    /// Create a new Hitch command runner
    pub fn new(binary_path: &std::path::Path, current_dir: &std::path::Path) -> Self {
        HitchCommandRunner {
            binary_path: binary_path.to_path_buf(),
            current_dir: current_dir.to_path_buf(),
        }
    }

    /// Start building a hitch command
    pub fn run(&self) -> HitchCommandBuilder<'_> {
        HitchCommandBuilder::new(&self.binary_path).current_dir(&self.current_dir)
    }

    /// Start building a hitch command without forcing `--no-push`
    ///
    /// Most tests should use `run()` (which defaults to `--no-push`), but this
    /// is useful for asserting true "no-args" behavior.
    pub fn run_raw(&self) -> HitchCommandBuilder<'_> {
        HitchCommandBuilder::new(&self.binary_path)
            .current_dir(&self.current_dir)
            .with_no_push(false)
    }

    /// Run hitch command with arguments directly (simpler API)
    pub fn exec(&self, args: &[&str]) -> Result<HitchCommandResult> {
        self.run().args(args).execute()
    }
}

/// Builder pattern for hitch commands with fluent API
pub struct HitchCommandBuilder<'a> {
    binary_path: &'a Path,
    args: Vec<String>,
    verbose: bool,
    no_push: bool,
    yes: bool,
    current_dir: Option<std::path::PathBuf>,
    env: Vec<(String, String)>,
}

impl<'a> HitchCommandBuilder<'a> {
    fn new(binary_path: &'a Path) -> Self {
        HitchCommandBuilder {
            binary_path,
            args: Vec::new(),
            verbose: false,
            no_push: true, // Always use --no-push in tests to avoid remote pushes
            yes: true,     // Tests have no TTY, so auto-confirm prompts by default
            current_dir: None,
            env: Vec::new(),
        }
    }

    /// Add command arguments
    pub fn args(mut self, args: &[&str]) -> Self {
        self.args.extend(args.iter().map(|s| s.to_string()));
        self
    }

    /// Add single argument
    pub fn arg(mut self, arg: &str) -> Self {
        self.args.push(arg.to_string());
        self
    }

    /// Enable verbose output
    pub fn verbose(mut self) -> Self {
        if !self.args.contains(&"--verbose".to_string()) {
            self.args.push("--verbose".to_string());
        }
        self.verbose = true;
        self
    }

    /// Enable no-push flag
    pub fn no_push(mut self) -> Self {
        if !self.args.contains(&"--no-push".to_string()) {
            self.args.push("--no-push".to_string());
        }
        self.no_push = true;
        self
    }

    /// Control whether `--no-push` is injected at execution time.
    pub fn with_no_push(mut self, no_push: bool) -> Self {
        self.no_push = no_push;
        self
    }

    /// Control whether `--yes` is injected at execution time.
    ///
    /// Set to `false` to assert the non-interactive refusal behaviour.
    pub fn with_yes(mut self, yes: bool) -> Self {
        self.yes = yes;
        self
    }

    /// Set working directory for command
    pub fn current_dir(mut self, dir: &Path) -> Self {
        self.current_dir = Some(dir.to_path_buf());
        self
    }

    /// Set environment variable for command
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// Execute the hitch command
    pub fn execute(self) -> Result<HitchCommandResult> {
        // The harness spawns the actual hitch binary under test; it is a
        // blessed spawn point with its own null-stdin handling (see below).
        #[allow(clippy::disallowed_methods)]
        let mut cmd = Command::new(self.binary_path);
        cmd.args(&self.args);

        // Add --no-push flag if enabled (default for tests to avoid remote pushes)
        if self.no_push {
            cmd.arg("--no-push");
        }

        // Add --yes flag (default for tests: no TTY is available to answer prompts)
        if self.yes {
            cmd.arg("--yes");
        } else {
            // Make sure an inherited HITCH_YES from the developer's shell can't
            // silently re-enable auto-confirmation.
            cmd.env_remove("HITCH_YES");
        }

        if let Some(dir) = &self.current_dir {
            cmd.current_dir(dir);
        }

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        // Same reasoning as GitCommandRunner::run above: never let the
        // hitch subprocess itself inherit a real terminal's stdin.
        cmd.stdin(std::process::Stdio::null());

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute hitch command: {:?}", self.args))?;

        Ok(HitchCommandResult {
            args: self.args,
            output,
            verbose: self.verbose,
            no_push: self.no_push,
        })
    }
}

/// Result of a hitch command execution
#[derive(Debug)]
pub struct HitchCommandResult {
    args: Vec<String>,
    output: Output,
    #[allow(dead_code)]
    verbose: bool,
    #[allow(dead_code)]
    no_push: bool,
}

impl HitchCommandResult {
    /// Check if command succeeded
    pub fn success(&self) -> bool {
        self.output.status.success()
    }

    /// Get stdout as string
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).to_string()
    }

    /// Get stderr as string
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).to_string()
    }

    /// Get exit code
    pub fn exit_code(&self) -> Option<i32> {
        self.output.status.code()
    }

    /// Assert that command succeeded
    pub fn assert_success(self) -> Self {
        if !self.success() {
            panic!(
                "Hitch command failed: {:?}\nstdout: {}\nstderr: {}",
                self.args,
                self.stdout(),
                self.stderr()
            );
        }
        self
    }

    /// Assert that command failed
    pub fn assert_failure(self) -> Self {
        if self.success() {
            panic!(
                "Expected hitch command to fail but it succeeded: {:?}\nstdout: {}",
                self.args,
                self.stdout()
            );
        }
        self
    }

    /// Assert an exact exit code — for commands with more than a plain
    /// success/failure result, e.g. `hitch rebuild`'s 0 (clean) / 2
    /// (succeeded with held branches) / 1 (failed) taxonomy.
    pub fn assert_exit_code(self, code: i32) -> Self {
        if self.exit_code() != Some(code) {
            panic!(
                "Expected exit code {}, got {:?}: {:?}\nstdout: {}\nstderr: {}",
                code,
                self.exit_code(),
                self.args,
                self.stdout(),
                self.stderr()
            );
        }
        self
    }

    /// Assert stdout contains specific text
    pub fn assert_stdout_contains(self, text: &str) -> Self {
        if !self.stdout().contains(text) {
            panic!(
                "Expected stdout to contain '{}', but it didn't:\nstdout: {}",
                text,
                self.stdout()
            );
        }
        self
    }

    /// Assert stderr contains specific text
    pub fn assert_stderr_contains(self, text: &str) -> Self {
        if !self.stderr().contains(text) {
            panic!(
                "Expected stderr to contain '{}', but it didn't:\nstderr: {}",
                text,
                self.stderr()
            );
        }
        self
    }

    /// Get the raw output for advanced assertions
    pub fn into_output(self) -> Output {
        self.output
    }
}

/// Command runner for Git operations
///
/// Provides simplified git command execution for test environments
#[derive(Debug, Clone)]
pub struct GitCommandRunner {
    repo_path: std::path::PathBuf,
}

impl GitCommandRunner {
    /// Create a new git command runner for the given repository path
    pub fn new(repo_path: &Path) -> Result<Self> {
        Ok(GitCommandRunner {
            repo_path: repo_path.to_path_buf(),
        })
    }

    /// Run git command with arguments
    pub fn run(&self, args: &[&str]) -> Result<GitCommandResult> {
        // The harness deliberately spawns plain git to simulate what a user
        // types; it is a blessed spawn point with its own null-stdin handling.
        #[allow(clippy::disallowed_methods)]
        let mut cmd = Command::new("git");
        cmd.args(args);
        cmd.current_dir(&self.repo_path);
        // Command::output() leaves stdin inherited from the test process by
        // default, so a git call that wants interactive input for any
        // reason (an editor, commit signing, ...) can hang the whole suite
        // instead of erroring — see the matching fix and gotcha entry for
        // hitch's own git_operations.rs::run_git_command in AGENTS.md.
        cmd.stdin(std::process::Stdio::null());

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute git command: git {}", args.join(" ")))?;

        Ok(GitCommandResult {
            args: args.iter().map(|s| s.to_string()).collect(),
            output,
        })
    }

    /// Configure git user for the repository
    pub fn config_user(&self, name: &str, email: &str) -> Result<()> {
        self.run(&["config", "user.name", name])?.assert_success();
        self.run(&["config", "user.email", email])?.assert_success();
        Ok(())
    }

    /// Initialize git repository
    pub fn init(&self) -> Result<()> {
        self.run(&["init", "--initial-branch=main"])?
            .assert_success();
        Ok(())
    }

    /// Create and checkout a new branch
    pub fn checkout_branch(&self, branch_name: &str) -> Result<()> {
        self.run(&["checkout", "-b", branch_name])?.assert_success();
        Ok(())
    }

    /// Create a commit with all staged changes
    pub fn commit(&self, message: &str) -> Result<()> {
        self.run(&["add", "--all"])?;
        self.run(&["commit", "-m", message])?.assert_success();
        Ok(())
    }

    /// Create a new file and commit it
    pub fn create_file_and_commit(
        &self,
        file_path: &str,
        content: &str,
        message: &str,
    ) -> Result<()> {
        std::fs::write(self.repo_path.join(file_path), content)
            .with_context(|| format!("Failed to write file: {}", file_path))?;
        self.commit(message)?;
        Ok(())
    }

    /// Get the current checked out branch name
    pub fn get_current_branch(&self) -> Result<String> {
        let result = self.run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        let branch = result.stdout().trim().to_string();
        Ok(branch)
    }
}

/// Result of a git command execution
#[derive(Debug)]
pub struct GitCommandResult {
    args: Vec<String>,
    output: Output,
}

impl GitCommandResult {
    /// Check if command succeeded
    pub fn success(&self) -> bool {
        self.output.status.success()
    }

    /// Get stdout as string
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).to_string()
    }

    /// Get stderr as string
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).to_string()
    }

    /// Get exit code
    pub fn exit_code(&self) -> Option<i32> {
        self.output.status.code()
    }

    /// Assert that command succeeded
    pub fn assert_success(self) -> Self {
        if !self.success() {
            panic!(
                "Git command failed: {:?}\nstdout: {}\nstderr: {}",
                self.args,
                self.stdout(),
                self.stderr()
            );
        }
        self
    }

    /// Assert that command failed
    pub fn assert_failure(self) -> Self {
        if self.success() {
            panic!(
                "Expected git command to fail but it succeeded: {:?}\nstdout: {}",
                self.args,
                self.stdout()
            );
        }
        self
    }

    /// Assert stdout contains specific text
    pub fn assert_stdout_contains(self, text: &str) -> Self {
        if !self.stdout().contains(text) {
            panic!(
                "Expected stdout to contain '{}', but it didn't:\nstdout: {}",
                text,
                self.stdout()
            );
        }
        self
    }

    /// Assert stderr contains specific text
    pub fn assert_stderr_contains(self, text: &str) -> Self {
        if !self.stderr().contains(text) {
            panic!(
                "Expected stderr to contain '{}', but it didn't:\nstderr: {}",
                text,
                self.stderr()
            );
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_git_command_runner() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let git = GitCommandRunner::new(temp_dir.path())?;

        // Test git initialization
        git.init()?;
        git.config_user("Test User", "test@example.com")?;

        // Test file creation and commit
        git.create_file_and_commit("test.txt", "test content", "Initial commit")?;

        // Test command execution
        let result = git.run(&["status"])?;
        assert!(result.success());
        assert!(result.stdout().contains("nothing to commit"));

        Ok(())
    }
}
