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

/// Result of `git merge-tree --write-tree --name-only` used for compatibility preflight.
#[derive(Debug, Clone)]
pub struct MergeTreeWriteTreeResult {
    /// OID of the toplevel tree produced by the merge attempt.
    pub tree_oid: String,
    /// List of conflicted file paths (empty when no conflicts).
    pub conflicted_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub timestamp: DateTime<Utc>,
    pub summary: String,
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
        // Force a stable, English locale so that the stdout/stderr substring checks
        // used throughout this module (e.g. "nothing to commit", "No local changes
        // to save", fetch "no remote" messages) are not broken by a user's locale.
        cmd.env("LC_ALL", "C");
        cmd.env("LANG", "C");
        cmd.output().context(format!(
            "Failed to execute git command: git {} in repository at {}",
            args.join(" "),
            self.repo_path
        ))
    }

    /// Perform a merge simulation without touching index or working tree.
    ///
    /// This wraps `git merge-tree --write-tree --name-only --merge-base <base> <our_tree> <their_tree>`.
    /// - On success (exit 0), `conflicted_files` is empty and `tree_oid` is the merged tree.
    /// - On conflict (exit 1), `conflicted_files` contains the paths listed by git and `tree_oid`
    ///   is still provided (as printed by git).
    /// - On other failures, returns an error.
    pub fn merge_tree_write_tree_name_only(
        &self,
        merge_base: &str,
        our_tree: &str,
        their_tree: &str,
    ) -> Result<MergeTreeWriteTreeResult> {
        let output = self.run_git_command(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--merge-base",
            merge_base,
            our_tree,
            their_tree,
        ])?;

        // git merge-tree returns:
        // - 0 for clean merge
        // - 1 for conflicted merge
        // - other for errors
        let code = output.status.code().unwrap_or(2);
        if code != 0 && code != 1 {
            return Err(anyhow::anyhow!(
                "git merge-tree failed (exit {}) — {}",
                code,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();

        let tree_oid = lines
            .next()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .ok_or_else(|| anyhow::anyhow!("git merge-tree produced no tree OID output"))?;

        let mut conflicted_files: Vec<String> = Vec::new();
        for l in lines {
            let l = l.trim();
            if l.is_empty() {
                break;
            }
            conflicted_files.push(l.to_string());
        }

        Ok(MergeTreeWriteTreeResult {
            tree_oid,
            conflicted_files,
        })
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
                    // Return a special name for detached HEAD state. Use a checked
                    // slice so a short/garbled rev-parse output can never panic.
                    Ok(format!(
                        "detached-HEAD-{}",
                        commit_hash.get(..7).unwrap_or(&commit_hash)
                    ))
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
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Some entries (e.g. `.gitignore`) are optional metadata. If the
                // file simply isn't present, git reports "did not match any files".
                // Skipping it is correct — committing the files that ARE present is
                // the intent. Failing here previously left hitch.json staged but
                // uncommitted, stranding the caller on hitch-metadata with a dirty
                // tree so the switch back to the user's branch aborted.
                if stderr.contains("did not match any files") {
                    continue;
                }
                return Err(anyhow::anyhow!(
                    "Failed to add file '{}': {}",
                    file,
                    stderr
                ));
            }
        }

        // Commit (bypass hooks since this is an automated metadata operation)
        let output = self.run_git_command(&["commit", "--no-verify", "-m", message])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            // A no-op write (byte-identical content) is not a failure: git reports
            // "nothing to commit". Treat it as success so redundant metadata updates
            // and rollbacks that restore already-current state don't spuriously error
            // (mirrors `commit` and `clean_working_directory`).
            if stderr.contains("nothing to commit") || stdout.contains("nothing to commit") {
                return Ok(());
            }
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

    /// Resolve a ref/commit-ish to a full SHA.
    pub fn rev_parse(&self, reference: &str) -> Result<String> {
        let output = self.run_git_command(&["rev-parse", reference])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git rev-parse {} failed: {}",
                reference,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn write_file(&self, file: &str, content: &str) -> Result<()> {
        use std::io::Write;

        let file_path = std::path::Path::new(&self.repo_path).join(file);
        let dir = file_path.parent().ok_or_else(|| {
            anyhow::anyhow!("Cannot write '{}': path has no parent directory", file)
        })?;

        // Write atomically: write to a temp file in the SAME directory, fsync it,
        // then rename over the target. rename(2) is atomic on POSIX, so a crash or
        // full disk mid-write can never leave a torn/half-written file for the next
        // `git show`/read to parse. Same-directory ensures the rename stays on one
        // filesystem.
        let tmp_name = format!(
            ".{}.hitch-tmp-{}",
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file"),
            std::process::id()
        );
        let tmp_path = dir.join(tmp_name);

        let write_res = (|| -> Result<()> {
            let mut f = std::fs::File::create(&tmp_path)
                .context(format!("Failed to create temp file for '{}'", file))?;
            f.write_all(content.as_bytes())
                .context(format!("Failed to write temp file for '{}'", file))?;
            f.sync_all()
                .context(format!("Failed to fsync temp file for '{}'", file))?;
            Ok(())
        })();

        if let Err(e) = write_res {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        std::fs::rename(&tmp_path, &file_path).context(format!(
            "Failed to atomically replace '{}' with the new contents",
            file
        ))?;

        Ok(())
    }

    /// Whether a failed `git fetch` stderr indicates a benign "no remote /
    /// offline / ref-not-on-remote" situation that Hitch should tolerate (it is
    /// designed to work in local-only repos), as opposed to a real error (auth
    /// failure, network problem) that a caller may want to surface.
    fn is_benign_fetch_failure(stderr: &str) -> bool {
        stderr.contains("does not appear to be a git repository")
            || stderr.contains("could not read from remote repository")
            || stderr.contains("couldn't find remote ref")
            || stderr.contains("no such remote ref")
            || stderr.contains("No remote configured")
            || stderr.contains("No such remote")
            || stderr.contains("does not have any commits yet")
    }

    /// Fetch a specific branch from the remote origin.
    ///
    /// Returns `Ok(())` when the fetch succeeds or when it fails for a benign
    /// reason (no remote configured, offline, or the ref doesn't exist remotely) —
    /// Hitch is designed to work in local-only repositories. A genuine failure
    /// (authentication, network) is returned as `Err` so callers can warn instead
    /// of silently proceeding on stale data.
    pub fn fetch_branch(&self, branch: &str) -> Result<()> {
        let output = self.run_git_command(&["fetch", "origin", branch])?;

        if output.status.success() {
            return Ok(());
        }

        // No remote / ref-not-on-remote is expected and must not block local-only
        // operation. Genuine failures (auth, network) are returned so callers that
        // care (metadata read/write) can warn instead of silently using stale data.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if Self::is_benign_fetch_failure(&stderr) {
            return Ok(());
        }

        Err(anyhow::anyhow!(
            "Failed to fetch '{}' from origin: {}",
            branch,
            stderr.trim()
        ))
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
        // Use an exact ref lookup rather than `git branch --list <name>`, which
        // treats <name> as a shell-style glob (so a branch like `feature/[wip]`
        // would be matched incorrectly, and `main` would match `main*`).
        let ref_name = format!("refs/heads/{}", branch);
        let output = self.run_git_command(&["show-ref", "--verify", "--quiet", &ref_name])?;

        Ok(output.status.success())
    }

    pub fn is_working_directory_clean(&self) -> Result<bool> {
        let output = self.run_git_command(&["status", "--porcelain"])?;

        // Treat a failed `git status` as an error rather than silently reporting
        // "clean" (empty stdout) when the command itself did not succeed.
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to check working directory status: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

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

    pub fn list_local_branches(&self) -> Result<Vec<String>> {
        let output =
            self.run_git_command(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git for-each-ref failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// List remote-tracking branches for `remote` (e.g. `origin`), returned without the `origin/`
    /// prefix. Does not include `origin/HEAD`.
    pub fn list_remote_branches(&self, remote: &str) -> Result<Vec<String>> {
        let prefix = format!("refs/remotes/{}/", remote);
        let output = self.run_git_command(&[
            "for-each-ref",
            "--format=%(refname)",
            &format!("refs/remotes/{}", remote),
        ])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git for-each-ref failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if !l.starts_with(&prefix) {
                    return None;
                }
                let short = l.trim_start_matches(&prefix);
                if short == "HEAD" || short.is_empty() {
                    None
                } else {
                    Some(short.to_string())
                }
            })
            .collect())
    }

    /// Return a ref to treat as the repo's default branch (e.g. `origin/main`, `main`, etc.).
    pub fn get_default_branch_ref(&self) -> Result<String> {
        // Try to resolve origin/HEAD first.
        if let Ok(output) =
            self.run_git_command(&["symbolic-ref", "-q", "refs/remotes/origin/HEAD"])
        {
            if output.status.success() {
                let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if let Some(suffix) = full.strip_prefix("refs/remotes/origin/") {
                    let candidate = format!("origin/{}", suffix);
                    if self.ref_exists(&candidate) {
                        return Ok(candidate);
                    }
                }
            }
        }

        for candidate in ["main", "origin/main", "master", "origin/master"] {
            if self.ref_exists(candidate) {
                return Ok(candidate.to_string());
            }
        }

        self.get_current_branch()
    }

    pub fn ahead_behind(&self, base: &str, branch: &str) -> Result<(usize, usize)> {
        let output = self.run_git_command(&[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{}...{}", base, branch),
        ])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git rev-list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let mut parts = s.split_whitespace();
        let behind: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let ahead: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Ok((behind, ahead))
    }

    pub fn get_last_commit(&self, reference: &str) -> Result<CommitInfo> {
        let mut commits = self.list_commits(reference, 1)?;
        commits
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No commits found for {}", reference))
    }

    pub fn list_commits(&self, reference: &str, limit: usize) -> Result<Vec<CommitInfo>> {
        let output = self.run_git_command(&[
            "log",
            "-n",
            &limit.to_string(),
            "--format=%H%x00%ct%x00%s",
            reference,
        ])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git log failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let mut out = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split('\0');
            let sha = parts.next().unwrap_or("").to_string();
            let ts = parts.next().unwrap_or("0");
            let summary = parts.next().unwrap_or("").to_string();
            if sha.is_empty() {
                continue;
            }
            let ts_i64: i64 = ts.parse().unwrap_or(0);
            let timestamp = DateTime::from_timestamp(ts_i64, 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?;
            out.push(CommitInfo {
                sha,
                timestamp,
                summary,
            });
        }
        Ok(out)
    }

    fn ref_exists(&self, reference: &str) -> bool {
        self.run_git_command(&["rev-parse", "--verify", reference])
            .ok()
            .is_some_and(|o| o.status.success())
    }

    /// Check if a branch exists (local or remote)
    pub fn branch_exists_anywhere(&self, branch: &str) -> Result<bool> {
        // Check local first
        if self.branch_exists(branch)? {
            return Ok(true);
        }

        // Check remote using the fully-qualified ref so `ls-remote` matches
        // exactly instead of treating the name as a tail/glob pattern.
        let full_ref = format!("refs/heads/{}", branch);
        let output = self.run_git_command(&["ls-remote", "--heads", "origin", &full_ref])?;

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
                // Find any other *user* branch to land on. Exclude hitch-managed
                // branches (the metadata branch and the transient tmp/backup
                // branches) so cleanup never strands the user on internal plumbing.
                // Strip both the current-branch marker ("* ") and the worktree
                // marker ("+ ") that `git branch --list` may prefix.
                let output = self.run_git_command(&["branch", "--list"])?;
                let branches = String::from_utf8_lossy(&output.stdout);
                branches
                    .lines()
                    .map(|b| {
                        b.trim_start_matches("* ")
                            .trim_start_matches("+ ")
                            .trim()
                            .to_string()
                    })
                    .find(|b| {
                        !b.is_empty()
                            && b != branch
                            && b != "hitch-metadata"
                            && !b.starts_with("hitch-tmp-")
                            && !b.starts_with("hitch-backup-")
                    })
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

    /// Merge a branch into the current branch with a merge commit.
    ///
    /// This preserves Git ancestry (unlike squash merges), which is important for stacked branches:
    /// downstream branches based on the merged branch won't need rebasing to avoid duplicated diffs.
    ///
    /// Uses `--no-ff` to always record a merge commit when a merge is performed.
    pub fn merge_no_ff_with_message(&self, source_branch: &str, message: &str) -> Result<()> {
        let output = self.run_git_command(&[
            "merge",
            "--no-ff",
            "--no-edit",
            "--no-verify",
            "-m",
            message,
            source_branch,
        ])?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to merge branch '{}' with a merge commit: {}",
                source_branch,
                String::from_utf8_lossy(&output.stderr)
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

    /// Build a comprehensive conflict result from the CURRENT conflicted state of
    /// the working tree — e.g. immediately after a `merge --squash` that
    /// conflicted — WITHOUT performing any additional merge.
    ///
    /// Callers use this to avoid a second, redundant "dry-run" merge purely to
    /// produce a conflict report: they attempt the real (squash) merge once, and
    /// if it conflicts, describe the conflict from the state it left behind.
    pub fn current_conflict_result(&self, source_branch: &str) -> Result<MergeConflictResult> {
        let target_branch = self
            .get_current_branch()
            .unwrap_or_else(|_| "HEAD".to_string());

        let conflicted_files = self.collect_detailed_conflicts()?;
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

        Ok(MergeConflictResult::with_conflicts(
            source_branch.to_string(),
            target_branch,
            conflicted_files,
            merge_base,
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

    /// Abort any in-progress merge/cherry-pick/revert and, ONLY if the tree is
    /// actually in a conflicted/merge state, hard-reset and clean it.
    ///
    /// The unconditional `reset --hard` + `clean -fd` this used to do would
    /// silently destroy a user's uncommitted changes and untracked files (data
    /// loss) whenever it ran while HEAD was on the user's branch. Callers that
    /// operate on the user's branch stash first (`with_auto_stash`) or require a
    /// clean tree (`pre_check`), so on those paths the tree is already clean and
    /// this is a no-op. The destructive reset now fires only when there is real
    /// merge state to clear — which, on hitch's own transient temp branches, is
    /// exactly what we want.
    pub fn abort_merge_and_clean(&self) -> Result<()> {
        // Aborting an in-progress operation is safe: it only unwinds something git
        // itself started, never the user's committed history or unrelated work.
        let _ = self.run_git_command(&["merge", "--abort"]);
        let _ = self.run_git_command(&["cherry-pick", "--abort"]);
        let _ = self.run_git_command(&["revert", "--abort"]);

        // Decide whether a destructive reset is warranted. After the aborts above,
        // a lingering unmerged index means a `--squash`-style conflict (no
        // MERGE_HEAD to abort), or an in-progress rebase — both of which we do want
        // to clear before checking out another branch.
        let git_dir = self.repo.path();
        let in_progress = git_dir.join("MERGE_HEAD").exists()
            || git_dir.join("CHERRY_PICK_HEAD").exists()
            || git_dir.join("REVERT_HEAD").exists();
        let needs_reset = in_progress || self.has_merge_conflicts().unwrap_or(false);

        if needs_reset {
            let _ = self.run_git_command(&["reset", "--hard"]);
            let _ = self.run_git_command(&["clean", "-fd"]);
        }

        Ok(())
    }

    /// Hard-reset the currently checked-out branch (and working tree) to `reference`.
    ///
    /// Used to roll a branch back to a previously captured commit — e.g. to undo
    /// partially-applied merges when a multi-branch release fails partway through, so
    /// the target branch is never left in a half-released state.
    pub fn reset_hard_to(&self, reference: &str) -> Result<()> {
        let output = self.run_git_command(&["reset", "--hard", reference])?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to reset to '{}': {}",
                reference,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }

    /// Fetch all remote branches from origin
    pub fn fetch_all_remotes(&self) -> Result<()> {
        let output = self.run_git_command(&["fetch", "--all"])?;

        if output.status.success() {
            return Ok(());
        }

        // No remote configured / offline is fine (local-only repos). Real failures
        // are returned so callers can decide whether to warn; `synchronize_branches`
        // intentionally treats this as best-effort so rebuild/release still work
        // offline, using whatever refs are available locally.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if Self::is_benign_fetch_failure(&stderr) {
            return Ok(());
        }

        Err(anyhow::anyhow!(
            "Failed to fetch from origin: {}",
            stderr.trim()
        ))
    }

    /// Create a local branch from a remote tracking branch
    pub fn create_local_branch_from_remote(&self, branch: &str) -> Result<()> {
        // Check if branch already exists locally
        if self.branch_exists(branch)? {
            return Ok(());
        }

        // Create local branch from remote tracking branch without checking it out
        let output =
            self.run_git_command(&["branch", "--track", branch, &format!("origin/{}", branch)])?;

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

    /// Fast-forward a local branch to `origin/<branch>` when it is strictly behind
    /// and has NOT diverged (i.e. the update is a pure fast-forward that cannot
    /// lose any local-only commits).
    ///
    /// Returns `Ok(true)` if the local ref was advanced, `Ok(false)` if nothing
    /// was done (no remote, already up to date, ahead, or diverged). Never rewinds
    /// or discards local work.
    pub fn fast_forward_to_remote_if_behind(&self, branch: &str) -> Result<bool> {
        let local_ref = format!("refs/heads/{}", branch);
        let remote_ref = format!("refs/remotes/origin/{}", branch);

        // Remote counterpart must exist to compare against.
        if !self
            .run_git_command(&["rev-parse", "--verify", "--quiet", &remote_ref])?
            .status
            .success()
        {
            return Ok(false);
        }
        if !self.branch_exists(branch)? {
            return Ok(false);
        }

        // Only advance on a true fast-forward: local must be an ancestor of remote.
        // This excludes the diverged case (would lose local commits) and the
        // local-ahead case (would rewind), and also short-circuits when equal.
        let is_ancestor = self
            .run_git_command(&["merge-base", "--is-ancestor", &local_ref, &remote_ref])?
            .status
            .success();
        if !is_ancestor {
            return Ok(false);
        }

        // Must actually be behind (remote has commits local lacks).
        let behind = self.run_git_command(&[
            "rev-list",
            "--count",
            &format!("{}..{}", local_ref, remote_ref),
        ])?;
        let behind_count: i64 = String::from_utf8_lossy(&behind.stdout)
            .trim()
            .parse()
            .unwrap_or(0);
        if behind_count == 0 {
            return Ok(false); // already up to date
        }

        // Don't rewrite the ref of the currently checked-out branch behind the
        // working tree's back; fast-forward it through the worktree instead.
        let current = self.get_current_branch().unwrap_or_default();
        if current == branch {
            let ff = self.run_git_command(&["merge", "--ff-only", &remote_ref])?;
            return Ok(ff.status.success());
        }

        let update = self.run_git_command(&["update-ref", &local_ref, &remote_ref])?;
        if !update.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to fast-forward '{}' to origin: {}",
                branch,
                String::from_utf8_lossy(&update.stderr).trim()
            ));
        }
        Ok(true)
    }

    /// Synchronize branches to ensure they're available locally AND up to date.
    ///
    /// This function, for each requested branch:
    /// 1. Fetches all remotes to get the latest updates (best-effort — offline is OK).
    /// 2. Creates the local branch from its remote tracking branch if it doesn't exist.
    /// 3. If it already exists locally, fast-forwards it to `origin/<branch>` when it
    ///    is strictly behind (so rebuild/release don't operate on stale commits and
    ///    then force-push over newer remote work). Diverged/ahead branches are left
    ///    untouched to avoid losing local commits.
    ///
    /// This is critical before operations like rebuild to ensure we have all
    /// necessary branches at their current commits.
    pub fn synchronize_branches(&self, branches: &[String]) -> Result<()> {
        // Best-effort fetch: a genuine network/auth failure should not block a
        // local-only rebuild/release. We proceed with whatever refs we have; the
        // mutating command's metadata fetch surfaces remote-connectivity problems.
        let _ = self.fetch_all_remotes();

        for branch in branches {
            if self.branch_exists(branch)? {
                // Advance stale local branches so we build from current commits.
                // Best-effort: if the fast-forward can't be performed we fall back
                // to the existing local ref rather than failing the whole operation.
                let _ = self.fast_forward_to_remote_if_behind(branch);
                continue;
            }

            // Not local yet — create it from the remote tracking branch if present.
            if self.branch_exists_anywhere(branch)? {
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
        // Unmerged index entries indicate conflicts regardless of how they arose
        // (regular merge, squash merge, cherry-pick, rebase). This is the reliable
        // signal: a `--squash` merge in particular leaves conflicts with SQUASH_MSG
        // and NO MERGE_HEAD, so keying off MERGE_HEAD alone would miss them.
        if let Ok(output) = self.run_git_command(&["ls-files", "--unmerged"]) {
            if output.status.success() && !output.stdout.is_empty() {
                return Ok(true);
            }
        }

        // Also treat an in-progress rebase/apply as an unresolved state.
        let git_dir = self.repo.path();
        if git_dir.join("rebase-apply").exists() || git_dir.join("rebase-merge").exists() {
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
