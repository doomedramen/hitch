use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use git2::Repository;
use std::process::Command;

use super::conflict_report::{parse_conflict_type, ConflictedFile, MergeBaseInfo};

/// Detailed result from a merge conflict check
#[derive(Debug)]
pub struct MergeConflictResult {
    /// Whether conflicts were detected
    pub has_conflicts: bool,
    /// List of conflicted files with detailed info
    pub conflicted_files: Vec<ConflictedFile>,
    /// Merge base information
    pub merge_base: Option<MergeBaseInfo>,
    /// The source branch that was being merged
    #[allow(dead_code)]
    pub source_branch: String,
    /// The target branch (current branch)
    pub target_branch: String,
}

impl MergeConflictResult {
    /// Create a new result indicating no conflicts
    pub fn no_conflicts(source_branch: String, target_branch: String) -> Self {
        Self {
            has_conflicts: false,
            conflicted_files: Vec::new(),
            merge_base: None,
            source_branch,
            target_branch,
        }
    }

    /// Create a new result indicating conflicts
    pub fn with_conflicts(
        source_branch: String,
        target_branch: String,
        conflicted_files: Vec<ConflictedFile>,
        merge_base: Option<MergeBaseInfo>,
    ) -> Self {
        Self {
            has_conflicts: true,
            conflicted_files,
            merge_base,
            source_branch,
            target_branch,
        }
    }
}

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

    /// Return the path to the `.git` directory for this repository
    pub fn get_git_dir(&self) -> String {
        self.repo.path().to_string_lossy().to_string()
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
        cmd.output().context(format!(
            "Failed to execute git command: git {} in repository at {}",
            args.join(" "),
            self.repo_path
        ))
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

    /// Get the current branch name, handling special cases properly
    ///
    /// This function handles:
    /// - Normal branches: Returns the branch name
    /// - Orphan branches: Returns the branch name
    /// - Detached HEAD: Returns "detached-HEAD-abcdef1" where abcdef1 is the first 7 chars of the commit hash
    ///
    /// # Returns
    /// - `Ok(String)`: Branch name or special detached HEAD identifier
    /// - `Err(anyhow::Error)`: If git commands fail
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

    /// Create an orphan branch (a branch with no history)
    ///
    /// An orphan branch is a branch that starts with no commits and no history.
    /// This is used for the hitch-metadata branch to store configuration separately
    /// from the main project history.
    ///
    /// # Process
    /// 1. Creates orphan branch with --orphan flag
    /// 2. Removes all files from the working directory (since orphan branches start clean)
    ///
    /// # Arguments
    /// - `branch_name`: Name of the orphan branch to create
    ///
    /// # Returns
    /// - `Ok(())`: Orphan branch created and cleaned
    /// - `Err(anyhow::Error)`: If git commands fail
    pub fn create_orphan_branch(&self, branch_name: &str) -> Result<()> {
        let output = self.run_git_command(&["checkout", "--orphan", branch_name])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create orphan branch '{}': {}",
                branch_name,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Clean working directory after creating orphan branch
        let _ = self.run_git_command(&["rm", "-rf", "."]).context(format!(
            "Failed to clean working directory after creating orphan branch '{}'",
            branch_name
        ))?;

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

        let content = String::from_utf8(output.stdout).context(format!(
            "Failed to parse file content: '{}' from branch '{}' (file may contain binary data)",
            file, branch
        ))?;

        Ok(content)
    }

    pub fn write_file(&self, file: &str, content: &str) -> Result<()> {
        let file_path = std::path::Path::new(&self.repo_path).join(file);
        std::fs::write(file_path, content).context(format!("Failed to write file '{}'", file))?;
        Ok(())
    }

    /// Fetch a specific branch from the remote origin
    ///
    /// This function attempts to fetch a specific branch from the origin remote.
    /// It's designed to be graceful about failures - if the fetch fails due to
    /// no remote or branch not existing remotely, it continues without error.
    ///
    /// This is important because Hitch should work in offline mode or with
    /// local-only repositories.
    ///
    /// # Arguments
    /// - `branch`: Branch name to fetch from origin
    ///
    /// # Returns
    /// - `Ok(())`: Always returns Ok (even if fetch fails for expected reasons)
    /// - `Err(anyhow::Error)`: Only for unexpected errors
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
    ///
    /// This function checks both local and remote branches, similar to branch_exists_anywhere().
    /// It first tries to get the SHA from the local branch (refs/heads/{branch}),
    /// and if that fails, tries to get it from the remote branch (refs/remotes/origin/{branch}).
    ///
    /// # Arguments
    /// - `branch`: Branch name to get the commit SHA for
    ///
    /// # Returns
    /// - `Ok(String)`: The commit SHA
    /// - `Err(anyhow::Error)`: If the branch doesn't exist locally or remotely
    pub fn get_branch_commit_sha(&self, branch: &str) -> Result<String> {
        // First try local branch
        let local_ref = format!("refs/heads/{}", branch);
        let output = self.run_git_command(&["rev-parse", &local_ref]);

        if let Ok(output) = output {
            if output.status.success() {
                let sha = String::from_utf8(output.stdout).context("Failed to parse commit SHA")?;
                return Ok(sha.trim().to_string());
            }
        }

        // If local doesn't exist, try remote branch
        let remote_ref = format!("refs/remotes/origin/{}", branch);
        let remote_output = self.run_git_command(&["rev-parse", &remote_ref])?;

        if !remote_output.status.success() {
            return Err(anyhow::anyhow!(
                "Branch '{}' does not exist locally or on remote origin: {}",
                branch,
                String::from_utf8_lossy(&remote_output.stderr)
            ));
        }

        let sha = String::from_utf8(remote_output.stdout).context("Failed to parse commit SHA")?;
        Ok(sha.trim().to_string())
    }

    /// Get the timestamp for a commit
    pub fn get_commit_timestamp(&self, sha: &str) -> Result<DateTime<Utc>> {
        let output = self.run_git_command(&["log", "-1", "--format=%at", sha])?;

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

        DateTime::from_timestamp(timestamp_i64, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid timestamp: {}", timestamp_i64))
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
        let current_branch = self.get_current_branch()?;

        // If we're currently on the branch we want to delete, we need to switch away first
        if current_branch == branch {
            // Try to find a safe branch to switch to
            // Priority: main -> master -> any other branch that's not the one being deleted
            let safe_branch: String = if self.branch_exists("main")? {
                "main".to_string()
            } else if self.branch_exists("master")? {
                "master".to_string()
            } else {
                // Find any other branch
                let output = self.run_git_command(&["branch", "--list"])?;
                let branches = String::from_utf8_lossy(&output.stdout);
                branches
                    .lines()
                    .map(|b| b.trim().trim_start_matches("* ").to_string())
                    .find(|b| !b.is_empty() && b != branch)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cannot delete branch '{}': no other branch to switch to",
                            branch
                        )
                    })?
            };

            let output = self
                .run_git_command(&["checkout", &safe_branch])
                .context(format!(
                    "Failed to switch to '{}' branch before deleting current branch",
                    safe_branch
                ))?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to switch to '{}' branch: {}",
                    safe_branch,
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
    ///
    /// This function performs a squash merge which combines all changes from the source branch
    /// into the current branch as a single commit. It's used during environment rebuilds.
    ///
    /// # Process
    /// 1. Stages changes from source branch without committing (--squash)
    /// 2. Adds any untracked files to ensure complete state capture
    /// 3. Checks if there are actually changes to commit (handles no-op merges)
    /// 4. Creates a single commit with the provided message, bypassing git hooks
    ///
    /// # Arguments
    /// - `source_branch`: Branch to squash merge into the current branch
    /// - `message`: Commit message for the squash merge
    ///
    /// # Returns
    /// - `Ok(())`: Merge succeeded (or was a no-op)
    /// - `Err(anyhow::Error)`: If any git command fails
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

    /// Check if a merge would result in conflicts and return comprehensive conflict information
    ///
    /// This enhanced version provides:
    /// - Detailed conflict type for each file
    /// - Full conflict content with markers
    /// - Merge base information
    ///
    /// # Arguments
    /// * `source_branch` - The branch to merge into the current branch
    ///
    /// # Returns
    /// A `MergeConflictResult` with all conflict details
    pub fn check_merge_conflicts_comprehensive(
        &self,
        source_branch: &str,
    ) -> Result<MergeConflictResult> {
        let target_branch = self
            .get_current_branch()
            .unwrap_or_else(|_| "HEAD".to_string());

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
                let _ = self.run_git_command(&["merge", "--abort"]);
                return Ok(MergeConflictResult::with_conflicts(
                    source_branch.to_string(),
                    target_branch,
                    vec![ConflictedFile::new(
                        "(unrelated histories)".to_string(),
                        super::conflict_report::ConflictType::Unknown,
                    )],
                    None,
                ));
            }

            // Get detailed conflict information before aborting
            let conflicted_files = self.collect_detailed_conflicts()?;

            // Get merge base information
            let merge_base = self
                .get_merge_base(&target_branch, source_branch)?
                .map(|hash| {
                    let date = self.get_commit_date(&hash).ok().flatten();
                    let mut base_info = MergeBaseInfo::new(hash);
                    if let Some(d) = date {
                        base_info = base_info.with_date(d);
                    }
                    base_info
                });

            // Abort the failed merge to clean up the working directory
            let _ = self.run_git_command(&["merge", "--abort"]);

            return Ok(MergeConflictResult::with_conflicts(
                source_branch.to_string(),
                target_branch,
                conflicted_files,
                merge_base,
            ));
        }

        // Abort the test merge
        let _ = self.run_git_command(&["merge", "--abort"]);

        Ok(MergeConflictResult::no_conflicts(
            source_branch.to_string(),
            target_branch,
        ))
    }

    /// Collect detailed information about conflicted files
    ///
    /// This function gathers:
    /// - File paths
    /// - Conflict types (UU, AA, UD, DU, etc.)
    /// - Actual conflict content from files
    fn collect_detailed_conflicts(&self) -> Result<Vec<ConflictedFile>> {
        let conflicts_with_status = self.get_conflicted_files_with_status()?;

        let mut detailed_conflicts = Vec::new();

        for (status, file_path) in conflicts_with_status {
            let conflict_type = parse_conflict_type(&status);

            // Try to read the conflict content from the file
            let conflict_content = self.get_file_conflict_content(&file_path).ok().flatten();

            let conflicted_file = if let Some(content) = conflict_content {
                ConflictedFile::with_content(file_path, conflict_type, content)
            } else {
                ConflictedFile::new(file_path, conflict_type)
            };

            detailed_conflicts.push(conflicted_file);
        }

        Ok(detailed_conflicts)
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
    /// Stash all uncommitted changes (staged + unstaged + untracked).
    ///
    /// Returns `true` if a stash entry was actually created (i.e. the tree was
    /// dirty), `false` if there was nothing to stash.
    pub fn stash_push(&self, message: &str) -> Result<bool> {
        // First check if there is actually anything to stash
        let output = self.run_git_command(&["status", "--porcelain"])?;
        if output.stdout.is_empty() {
            return Ok(false);
        }

        let out = self.run_git_command(&["stash", "push", "-u", "-m", message])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!("git stash push failed: {}", stderr));
        }
        // "No local changes to save" means nothing was stashed
        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(!stdout.contains("No local changes to save"))
    }

    /// Pop the most recent stash entry.
    pub fn stash_pop(&self) -> Result<()> {
        let out = self.run_git_command(&["stash", "pop"])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!("git stash pop failed: {}", stderr));
        }
        Ok(())
    }

    /// Return the message of the most recent stash entry, or `None` if there
    /// is no stash.
    pub fn stash_top_message(&self) -> Option<String> {
        let out = self
            .run_git_command(&["stash", "list", "--format=%s", "-n", "1"])
            .ok()?;
        if out.status.success() {
            let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if msg.is_empty() {
                None
            } else {
                Some(msg)
            }
        } else {
            None
        }
    }

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

    /// Synchronize branches to ensure they're available locally
    ///
    /// This function ensures all specified branches exist locally by:
    /// 1. Fetching all remotes to get latest updates
    /// 2. Creating local branches from remote tracking branches if they don't exist locally
    ///
    /// This is critical before operations like rebuild to ensure we have all necessary branches.
    ///
    /// # Arguments
    /// - `branches`: List of branch names to ensure are available locally
    ///
    /// # Returns
    /// - `Ok(())`: All branches are now available locally
    /// - `Err(anyhow::Error)`: If fetch or branch creation fails
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

    /// Commit staged changes with a message
    pub fn commit(&self, message: &str) -> Result<()> {
        let output = self.run_git_command(&["commit", "--no-verify", "-m", message])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Check if it's just "nothing to commit" case
            if stderr.contains("nothing to commit") || stdout.contains("nothing to commit") {
                return Ok(());
            }

            return Err(anyhow::anyhow!(
                "Failed to commit: {}\nstdout: {}\nstderr: {}",
                message,
                stdout,
                stderr
            ));
        }
        Ok(())
    }

    /// Create an annotated git tag
    pub fn create_tag(&self, tag_name: &str, message: &str) -> Result<()> {
        let output = self.run_git_command(&["tag", "-a", tag_name, "-m", message])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to create tag '{}': {}",
                tag_name,
                stderr
            ));
        }
        Ok(())
    }

    /// Push a tag to remote
    pub fn push_tag(&self, tag_name: &str) -> Result<()> {
        let output = self.run_git_command(&["push", "origin", tag_name])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to push tag '{}': {}",
                tag_name,
                stderr
            ));
        }
        Ok(())
    }

    /// Check if a branch is merged into another branch
    pub fn is_branch_merged_into(&self, source_branch: &str, target_branch: &str) -> Result<bool> {
        let output =
            self.run_git_command(&["merge-base", "--is-ancestor", source_branch, target_branch])?;
        Ok(output.status.success())
    }

    /// Get the merge base (common ancestor) between two branches
    ///
    /// # Arguments
    /// * `branch1` - First branch name
    /// * `branch2` - Second branch name
    ///
    /// # Returns
    /// The commit hash of the merge base, or an error if not found
    pub fn get_merge_base(&self, branch1: &str, branch2: &str) -> Result<Option<String>> {
        let output = self.run_git_command(&["merge-base", branch1, branch2])?;

        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !hash.is_empty() {
                Ok(Some(hash))
            } else {
                Ok(None)
            }
        } else {
            // No common ancestor (unrelated histories)
            Ok(None)
        }
    }

    /// Get the date of a commit
    ///
    /// # Arguments
    /// * `commit_hash` - The commit hash to get the date for
    ///
    /// # Returns
    /// The commit date in YYYY-MM-DD format
    pub fn get_commit_date(&self, commit_hash: &str) -> Result<Option<String>> {
        let output = self.run_git_command(&["log", "-1", "--format=%cs", commit_hash])?;

        if output.status.success() {
            let date = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !date.is_empty() {
                Ok(Some(date))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Get detailed conflict information with status codes
    ///
    /// Returns a list of (status_code, file_path) tuples for conflicted files.
    /// Status codes are two-letter codes from git status:
    /// - UU: both modified
    /// - AA: both added
    /// - UD: modified/deleted
    /// - DU: deleted/modified
    pub fn get_conflicted_files_with_status(&self) -> Result<Vec<(String, String)>> {
        // Use git status --porcelain to get status codes
        let output = self.run_git_command(&["status", "--porcelain"])?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let conflicts: Vec<(String, String)> = stdout
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.len() >= 3 {
                    let status = &line[0..2];
                    // Check if it's a conflict status (contains U or both same letter)
                    if status.contains('U') || status == "AA" || status == "DD" {
                        let file_path = line[3..].trim().to_string();
                        return Some((status.to_string(), file_path));
                    }
                }
                None
            })
            .collect();

        Ok(conflicts)
    }

    /// List local branches whose names start with a given prefix
    ///
    /// Used to detect stale hitch-managed branches (e.g. hitch-tmp-*, hitch-backup-*)
    pub fn list_local_branches_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let pattern = format!("{}*", prefix);
        let output = self.run_git_command(&["branch", "--list", &pattern])?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .map(|line| line.trim().trim_start_matches("* ").to_string())
            .filter(|b| !b.is_empty())
            .collect();

        Ok(branches)
    }

    /// Read the conflict markers from a file
    ///
    /// This reads the file and extracts all conflict sections (between <<<<<<< and >>>>>>>)
    ///
    /// # Arguments
    /// * `file_path` - Path to the file relative to repo root
    ///
    /// # Returns
    /// The full file content with conflict markers, or None if file doesn't exist
    pub fn get_file_conflict_content(&self, file_path: &str) -> Result<Option<String>> {
        use std::fs;
        use std::path::Path;

        let full_path = Path::new(&self.repo_path).join(file_path);

        if !full_path.exists() {
            return Ok(None);
        }

        match fs::read_to_string(&full_path) {
            Ok(content) => {
                // Extract just the conflict sections
                let mut result = String::new();
                let mut in_conflict = false;
                let mut conflict_count = 0;

                for line in content.lines() {
                    if line.starts_with("<<<<<<<") {
                        in_conflict = true;
                        conflict_count += 1;
                        if conflict_count > 1 {
                            result.push_str("\n---\n\n"); // Separator between conflicts
                        }
                    }

                    if in_conflict {
                        result.push_str(line);
                        result.push('\n');
                    }

                    if line.starts_with(">>>>>>>") {
                        in_conflict = false;
                    }
                }

                if result.is_empty() {
                    // No conflict markers found, but file is marked as conflicted
                    // This can happen with binary files or certain conflict types
                    Ok(Some(
                        "(Binary file or conflict markers not available)".to_string(),
                    ))
                } else {
                    Ok(Some(result.trim().to_string()))
                }
            }
            Err(_) => Ok(None),
        }
    }

    /// Return one-line commit descriptions for commits in `branch` that are
    /// not reachable from `base` (i.e., `git log --oneline base..branch`).
    pub fn get_commits_between(&self, base: &str, branch: &str) -> Result<Vec<String>> {
        let range = format!("{}..{}", base, branch);
        let output = self.run_git_command(&["log", "--oneline", &range])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git log failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let lines = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(lines)
    }

    /// Check if a local branch is behind its remote counterpart
    ///
    /// This compares the local branch to origin/<branch> to determine if
    /// there are commits on the remote that haven't been pulled locally.
    ///
    /// # Arguments
    /// - `branch`: Branch name to check
    ///
    /// # Returns
    /// - `Ok(true)`: Local branch is behind remote (needs pull)
    /// - `Ok(false)`: Local branch is up to date or ahead of remote
    /// - `Err`: If unable to determine (e.g., no remote branch exists)
    pub fn is_branch_behind_remote(&self, branch: &str) -> Result<bool> {
        let local_ref = format!("refs/heads/{}", branch);
        let remote_ref = format!("refs/remotes/origin/{}", branch);

        // Check if remote branch exists
        let remote_output = self.run_git_command(&["rev-parse", &remote_ref])?;
        if !remote_output.status.success() {
            // No remote branch, so can't be behind
            return Ok(false);
        }

        // Check if local branch exists
        let local_output = self.run_git_command(&["rev-parse", &local_ref])?;
        if !local_output.status.success() {
            // No local branch
            return Err(anyhow::anyhow!("Local branch '{}' does not exist", branch));
        }

        // Check if local is behind remote using rev-list --count
        // This counts commits in remote that are not in local
        let output = self.run_git_command(&[
            "rev-list",
            "--count",
            &format!("{}..{}", local_ref, remote_ref),
        ])?;

        if output.status.success() {
            let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let count: i32 = count_str.parse().unwrap_or(0);
            return Ok(count > 0);
        }

        Ok(false)
    }

    /// Check if there are any merge conflicts in the current working tree
    ///
    /// # Returns
    /// - `Ok(true)`: There are merge conflicts
    /// - `Ok(false)`: No merge conflicts
    /// - `Err`: If unable to check
    pub fn has_merge_conflicts(&self) -> Result<bool> {
        // Check for merge state
        let merge_head = self.run_git_command(&["rev-parse", "--verify", "MERGE_HEAD"]);

        // If MERGE_HEAD exists, we're in a merge state
        if merge_head.is_ok() && merge_head.unwrap().status.success() {
            // Check if there are actual conflict files
            let conflicted = self.get_conflicted_files()?;
            return Ok(!conflicted.is_empty());
        }

        // Also check for rebase/apply states
        let git_dir = self.repo.path();
        let rebase_apply = git_dir.join("rebase-apply");
        let rebase_merge = git_dir.join("rebase-merge");

        if rebase_apply.exists() || rebase_merge.exists() {
            return Ok(true);
        }

        Ok(false)
    }

    /// Return the `--stat` summary of changes introduced by `branch` relative
    /// to its common ancestor with `base` (`git diff --stat base...branch`).
    pub fn get_diff_stat(&self, base: &str, branch: &str) -> Result<String> {
        let range = format!("{}...{}", base, branch);
        let output = self.run_git_command(&["diff", "--stat", &range])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git diff --stat failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
