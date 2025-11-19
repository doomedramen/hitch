use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use git2::Repository;
use std::process::Command;

pub struct GitOperations {
    #[allow(dead_code)]
    repo: Repository,
    repo_path: String,
}

impl GitOperations {
    pub fn new() -> Result<Self> {
        let repo = Repository::discover(".").context("Not in a git repository")?;
        let repo_path = repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("Repository has no working directory"))?
            .to_string_lossy()
            .to_string();
        Ok(GitOperations { repo, repo_path })
    }

    #[allow(dead_code)]
    pub fn new_at_path(path: &str) -> Result<Self> {
        // Open the repository at the exact path to avoid discovering parent repositories
        let repo = Repository::open(path).context("Not in a git repository")?;
        let repo_path = repo
            .workdir()
            .ok_or_else(|| anyhow::anyhow!("Repository has no working directory"))?
            .to_string_lossy()
            .to_string();
        Ok(GitOperations { repo, repo_path })
    }

    pub fn run_git_command(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        cmd.current_dir(&self.repo_path);
        cmd.output().context("Failed to execute git command")
    }

    /// Run multiple git commands sequentially, returning early on first failure
    #[allow(dead_code)]
    fn run_git_commands(&self, commands: &[&[&str]]) -> Result<()> {
        for args in commands {
            let output = self.run_git_command(args)?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Git command failed: git {} - {}",
                    args.join(" "),
                    stderr.trim()
                ));
            }
        }
        Ok(())
    }

    pub fn get_current_branch(&self) -> Result<String> {
        // Use git command to handle orphan branches properly
        let output = self.run_git_command(&["branch", "--show-current"])?;

        if output.status.success() {
            let branch_name = String::from_utf8(output.stdout)?.trim().to_string();
            if !branch_name.is_empty() {
                Ok(branch_name)
            } else {
                // Handle detached HEAD - get the commit hash instead
                let rev_output = self.run_git_command(&["rev-parse", "HEAD"])?;

                if rev_output.status.success() {
                    let commit_hash = String::from_utf8(rev_output.stdout)?.trim().to_string();
                    // Return a special name for detached HEAD state
                    Ok(format!("detached-HEAD-{}", &commit_hash[..7]))
                } else {
                    Err(anyhow::anyhow!(
                        "Failed to get current branch or HEAD commit"
                    ))
                }
            }
        } else {
            Err(anyhow::anyhow!(
                "Failed to get current branch: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub fn checkout_branch(&self, branch: &str) -> Result<()> {
        let output = self.run_git_command(&["checkout", branch])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to checkout branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn create_orphan_branch(&self, branch_name: &str) -> Result<()> {
        let output = self.run_git_command(&["checkout", "--orphan", branch_name])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create orphan branch '{}': {}",
                branch_name,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Clean working directory
        let _ = self
            .run_git_command(&["rm", "-rf", "."])
            .context("Failed to clean working directory")?;

        Ok(())
    }

    pub fn add_and_commit(&self, files: &[&str], message: &str) -> Result<()> {
        // Add files (use -f to force-add files that might be ignored by .gitignore)
        for file in files {
            let output = self.run_git_command(&["add", "-f", file])?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to add file '{}': {}",
                    file,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        // Commit (bypass hooks since this is an automated metadata operation)
        let output = self.run_git_command(&["commit", "--no-verify", "-m", message])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "Failed to commit: {}\nstdout: {}\nstderr: {}",
                stderr,
                stdout,
                stderr
            ));
        }

        Ok(())
    }

    pub fn read_file_from_branch(&self, branch: &str, file: &str) -> Result<String> {
        let output = self.run_git_command(&["show", &format!("{}:{}", branch, file)])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "File '{}' not found in branch '{}'",
                file,
                branch
            ));
        }

        let content = String::from_utf8(output.stdout).context("Failed to parse file content")?;

        Ok(content)
    }

    pub fn write_file(&self, file: &str, content: &str) -> Result<()> {
        let file_path = std::path::Path::new(&self.repo_path).join(file);
        std::fs::write(file_path, content).context(format!("Failed to write file '{}'", file))?;
        Ok(())
    }

    pub fn fetch_branch(&self, branch: &str) -> Result<()> {
        let output = self.run_git_command(&["fetch", "origin", branch])?;

        if !output.status.success() {
            // Don't fail if fetch fails - user may not have a remote or branch may not exist remotely
            // According to SPEC.md, fetching should be optional and not block operations
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check for common "no remote" scenarios that should not fail the operation
            if stderr.contains("does not appear to be a git repository")
                || stderr.contains("could not read from remote repository")
                || stderr.contains("couldn't find remote ref")
                || stderr.contains("no such remote ref")
                || stderr.contains("fatal: unable to access")
                || stderr.contains("fatal: could not read")
            {
                // These are expected "no remote available" scenarios - continue gracefully
                return Ok(());
            }

            // Other fetch errors might be network issues, but should still not block local operations
            // according to SPEC.md requirements for working with local repositories
            return Ok(());
        }

        Ok(())
    }

    pub fn push_branch(&self, branch: &str) -> Result<()> {
        let output = self.run_git_command(&["push", "origin", branch])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to push branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn force_push_branch(&self, branch: &str) -> Result<()> {
        let output =
            self.run_git_command(&["push", "origin", branch, "--force", "--set-upstream"])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to force push branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = self.run_git_command(&["branch", "--list", branch])?;

        Ok(output.status.success() && !output.stdout.is_empty())
    }

    pub fn is_working_directory_clean(&self) -> Result<bool> {
        let output = self.run_git_command(&["status", "--porcelain"])?;

        Ok(output.stdout.is_empty())
    }

    /// Clean working directory by adding all changes and committing them
    pub fn clean_working_directory(&self, message: &str) -> Result<()> {
        let add_output = self.run_git_command(&["add", "--all"])?;
        if !add_output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to add all files: {}",
                String::from_utf8_lossy(&add_output.stderr)
            ));
        }

        let commit_output = self.run_git_command(&["commit", "-m", message])?;
        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            let stdout = String::from_utf8_lossy(&commit_output.stdout);
            // Don't fail if there's nothing to commit
            if !(stderr.contains("nothing to commit") || stdout.contains("nothing to commit")) {
                return Err(anyhow::anyhow!(
                    "Failed to commit changes: stderr={}, stdout={}",
                    stderr,
                    stdout
                ));
            }
        }

        Ok(())
    }

    pub fn get_user_email(&self) -> Result<String> {
        let output = self.run_git_command(&["config", "user.email"])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Git user.email not configured"));
        }

        let email = String::from_utf8(output.stdout).context("Failed to parse user email")?;
        Ok(email.trim().to_string())
    }

    /// Get the latest commit SHA for a branch
    pub fn get_branch_commit_sha(&self, branch: &str) -> Result<String> {
        let output = self.run_git_command(&["rev-parse", &format!("refs/heads/{}", branch)])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Branch '{}' does not exist or is not accessible: {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let sha = String::from_utf8(output.stdout).context("Failed to parse commit SHA")?;
        Ok(sha.trim().to_string())
    }

    /// Get the timestamp for a commit
    pub fn get_commit_timestamp(&self, sha: &str) -> Result<DateTime<Utc>> {
        let output = self.run_git_command(&["show", "-s", "--format=%ct", sha])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to get timestamp for commit '{}': {}",
                sha,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let timestamp_str =
            String::from_utf8(output.stdout).context("Failed to parse timestamp")?;
        let timestamp_i64: i64 = timestamp_str
            .trim()
            .parse()
            .context("Failed to parse timestamp as integer")?;

        Ok(DateTime::from_timestamp(timestamp_i64, 0).unwrap_or_else(Utc::now))
    }

    /// Check if a branch exists (local or remote)
    pub fn branch_exists_anywhere(&self, branch: &str) -> Result<bool> {
        // Check local first
        if self.branch_exists(branch)? {
            return Ok(true);
        }

        // Check remote
        let output = self.run_git_command(&["ls-remote", "--heads", "origin", branch])?;

        Ok(output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    /// Create a new branch from a specific source branch
    pub fn create_branch_from(&self, new_branch: &str, source_branch: &str) -> Result<()> {
        let output = self.run_git_command(&["checkout", "-b", new_branch, source_branch])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create branch '{}' from '{}': {}",
                new_branch,
                source_branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    /// Rename current branch to a new name
    pub fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<()> {
        let output = self.run_git_command(&["branch", "-m", old_name, new_name])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to rename branch '{}' to '{}': {}",
                old_name,
                new_name,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    /// Delete a branch (local)
    pub fn delete_branch(&self, branch: &str, force: bool) -> Result<()> {
        // Get current branch to ensure we're not trying to delete the branch we're on
        let current_branch = self.get_current_branch().unwrap_or_default();

        // If we're currently on the branch we want to delete, switch to main first
        if current_branch == branch {
            let output = self
                .run_git_command(&["checkout", "main"])
                .context("Failed to switch to main branch before deleting current branch")?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to switch to main branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        let mut args = vec!["branch", "-d"];
        if force {
            args[1] = "-D"; // Force delete
        }
        args.push(branch);

        let output = self.run_git_command(&args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // If it's a worktree issue, we need a more aggressive approach
            if stderr.contains("used by worktree") {
                // For worktree issues, try to force delete with additional flags
                let force_args = vec!["branch", "-D", "--force", branch];
                let force_output = self.run_git_command(&force_args).context(format!(
                    "Failed to force delete branch '{}' with --force",
                    branch
                ))?;

                if force_output.status.success() {
                    return Ok(());
                }

                let force_stderr = String::from_utf8_lossy(&force_output.stderr);

                // Last resort: try to remove the branch ref directly
                let ref_path = format!("refs/heads/{}", branch);
                let direct_args = vec!["update-ref", "-d", &ref_path];
                let direct_output = self
                    .run_git_command(&direct_args)
                    .context(format!("Failed to delete branch ref directly '{}'", branch))?;

                if direct_output.status.success() {
                    return Ok(());
                }

                let direct_stderr = String::from_utf8_lossy(&direct_output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to delete branch '{}': {} (force delete failed: {}, direct ref delete failed: {})",
                    branch, stderr, force_stderr, direct_stderr
                ));
            }

            return Err(anyhow::anyhow!(
                "Failed to delete branch '{}': {}",
                branch,
                stderr
            ));
        }

        Ok(())
    }

    /// Squash merge a branch into the current branch
    pub fn squash_merge(&self, source_branch: &str, message: &str) -> Result<()> {
        let output = self.run_git_command(&["merge", "--squash", source_branch])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to squash merge branch '{}': {}",
                source_branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Add any remaining untracked files before committing
        let add_output = self
            .run_git_command(&["add", "--all"])
            .context("Failed to add untracked files before squash merge commit")?;

        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to add untracked files before squash merge: {}",
                stderr
            ));
        }

        // Check if there's anything to commit
        let status_output = self.run_git_command(&["status", "--porcelain"])?;

        if status_output.status.success() && status_output.stdout.is_empty() {
            // Nothing to commit - this is fine, means branches are already up to date
            // This can happen when promoted branches have no new commits beyond the base branch
            return Ok(());
        }

        // Commit the squash merge (bypass hooks since this is an automated operation)
        let commit_output = self.run_git_command(&["commit", "--no-verify", "-m", message])?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            let stdout = String::from_utf8_lossy(&commit_output.stdout);

            // Check if it's just "nothing to commit" case
            if stderr.contains("nothing to commit") || stdout.contains("nothing to commit") {
                // This is actually fine - no changes needed
                return Ok(());
            }

            return Err(anyhow::anyhow!(
                "Failed to commit squash merge: {}\nstdout: {}\nstderr: {}",
                stderr,
                stdout,
                stderr
            ));
        }

        Ok(())
    }

    /// Check if a merge would result in conflicts and return detailed conflict info
    pub fn check_merge_conflicts_detailed(
        &self,
        source_branch: &str,
    ) -> Result<(bool, Option<Vec<String>>)> {
        let output = self.run_git_command(&["merge", "--no-commit", "--no-ff", source_branch])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check if it's a non-existent branch error
            if stderr.contains("did not match any file(s) known to git")
                || stderr.contains("unknown revision or path")
                || stderr.contains("not something we can merge")
                || stderr.contains("Non-fast-forward commit does not make sense into an empty head")
            {
                return Err(anyhow::anyhow!("Branch '{}' does not exist", source_branch));
            }

            // Check if it's unrelated histories error
            if stderr.contains("refusing to merge unrelated histories") {
                // This is not a merge conflict, but a git history issue
                // Abort the merge attempt to clean up
                let _ = self.run_git_command(&["merge", "--abort"]);
                return Ok((true, None)); // We'll treat this as a conflict for now
            }

            // If merge fails for other reasons, it's likely due to conflicts
            // Try to get the list of conflicted files
            let conflicted_files = self.get_conflicted_files().unwrap_or_default();

            // Abort the failed merge to clean up the working directory
            let _ = self.run_git_command(&["merge", "--abort"]);

            return Ok((true, Some(conflicted_files)));
        }

        // Abort the test merge
        let _ = self.run_git_command(&["merge", "--abort"]);

        Ok((false, None))
    }

    /// Get list of conflicted files in the current working directory
    pub fn get_conflicted_files(&self) -> Result<Vec<String>> {
        let output = self.run_git_command(&["diff", "--name-only", "--diff-filter=U"])?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let conflicted_files: Vec<String> = stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
                .collect();
            Ok(conflicted_files)
        } else {
            // If git diff fails, return empty list
            Ok(Vec::new())
        }
    }

    /// Abort any ongoing merge operation and reset working directory to clean state
    pub fn abort_merge_and_clean(&self) -> Result<()> {
        // First try to abort any ongoing merge
        let _ = self.run_git_command(&["merge", "--abort"]);

        // Then reset any conflicted files to clean state
        let _ = self.run_git_command(&["reset", "--hard"]);

        // Clean up any untracked files
        let _ = self.run_git_command(&["clean", "-fd"]);

        Ok(())
    }

    /// Fetch all remote branches from origin
    pub fn fetch_all_remotes(&self) -> Result<()> {
        let output = self.run_git_command(&["fetch", "--all"])?;

        if !output.status.success() {
            // Don't fail if fetch fails - user may not have a remote or network issues
            // Similar to fetch_branch, continue gracefully
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check for common "no remote" scenarios that should not fail the operation
            if stderr.contains("does not appear to be a git repository")
                || stderr.contains("could not read from remote repository")
                || stderr.contains("couldn't find remote ref")
                || stderr.contains("no such remote ref")
                || stderr.contains("fatal: unable to access")
                || stderr.contains("fatal: could not read")
            {
                // These are expected "no remote available" scenarios - continue gracefully
                return Ok(());
            }

            // Other fetch errors might be network issues, but should still not block local operations
            return Ok(());
        }

        Ok(())
    }

    /// Create a local branch from a remote tracking branch
    pub fn create_local_branch_from_remote(&self, branch: &str) -> Result<()> {
        // Check if branch already exists locally
        if self.branch_exists(branch)? {
            return Ok(());
        }

        // Create local branch from remote tracking branch
        let output =
            self.run_git_command(&["checkout", "-b", branch, &format!("origin/{}", branch)])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create local branch '{}' from 'origin/{}': {}",
                branch,
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    /// Synchronize branches: ensure all specified branches are available locally
    /// This will fetch all remotes and create local branches from remote tracking branches as needed
    pub fn synchronize_branches(&self, branches: &[String]) -> Result<()> {
        // First, fetch all remote branches to get latest updates
        self.fetch_all_remotes()?;

        // Then, ensure each branch exists locally by creating from remote if needed
        for branch in branches {
            // Skip if branch already exists locally
            if self.branch_exists(branch)? {
                continue;
            }

            // Check if remote branch exists
            if self.branch_exists_anywhere(branch)? {
                // Create local branch from remote tracking branch
                self.create_local_branch_from_remote(branch)?;
            }
        }

        Ok(())
    }
}
