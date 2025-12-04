//! Custom error types for Git operations
//!
//! Provides more specific error types for Git operations to improve error handling
//! and provide better context to users.

use std::fmt;

/// Custom error types for Git operations
#[derive(Debug)]
#[allow(dead_code)]
pub enum GitError {
    /// Error when repository is not found or not a valid git repository
    NotAGitRepository { path: String },

    /// Error when a branch is not found
    BranchNotFound {
        branch: String,
        remote: Option<String>,
    },

    /// Error when trying to create a branch that already exists
    BranchAlreadyExists { branch: String },

    /// Error when working directory is not clean
    WorkingDirectoryNotClean,

    /// Error when there is no current branch (detached HEAD)
    NoCurrentBranch { commit: String },

    /// Error when a merge conflicts occur
    MergeConflicts { count: usize, files: String },

    /// Error when a remote is not found
    RemoteNotFound { remote: String },

    /// Error when authentication fails
    AuthenticationFailed { remote: String },

    /// Error when network operation fails
    NetworkError { message: String },

    /// Error when a git command fails
    CommandFailed { command: String, stderr: String },

    /// Error for permission issues
    PermissionDenied { operation: String },

    /// Error for lock related issues
    LockError { message: String },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAGitRepository { path } => write!(f, "Not a git repository: {}", path),
            Self::BranchNotFound { branch, remote } => match remote {
                Some(r) => write!(f, "Branch '{}' not found (remote: {})", branch, r),
                None => write!(f, "Branch '{}' not found", branch),
            },
            Self::BranchAlreadyExists { branch } => write!(f, "Branch '{}' already exists", branch),
            Self::WorkingDirectoryNotClean => write!(
                f,
                "Working directory is not clean. Commit or stash changes first."
            ),
            Self::NoCurrentBranch { commit } => {
                write!(f, "No current branch (detached HEAD at {})", commit)
            }
            Self::MergeConflicts { count, files } => write!(
                f,
                "Merge conflicts detected in {} file(s): {}",
                count, files
            ),
            Self::RemoteNotFound { remote } => write!(f, "Remote '{}' not found", remote),
            Self::AuthenticationFailed { remote } => {
                write!(f, "Authentication failed for remote '{}'", remote)
            }
            Self::NetworkError { message } => write!(f, "Network error: {}", message),
            Self::CommandFailed { command, stderr } => {
                write!(f, "Git command failed: git {} - {}", command, stderr)
            }
            Self::PermissionDenied { operation } => write!(f, "Permission denied: {}", operation),
            Self::LockError { message } => write!(f, "Lock error: {}", message),
        }
    }
}

impl std::error::Error for GitError {}

impl GitError {
    /// Create a branch not found error
    #[allow(dead_code)]
    pub fn branch_not_found(branch: &str) -> Self {
        Self::BranchNotFound {
            branch: branch.to_string(),
            remote: None,
        }
    }

    /// Create a remote branch not found error
    #[allow(dead_code)]
    pub fn remote_branch_not_found(branch: &str, remote: &str) -> Self {
        Self::BranchNotFound {
            branch: branch.to_string(),
            remote: Some(format!("{}/{}", remote, branch)),
        }
    }

    /// Create a command failed error from git output
    #[allow(dead_code)]
    pub fn from_git_output(command: &[&str], output: &std::process::Output) -> Self {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Self::CommandFailed {
            command: command.join(" "),
            stderr: stderr.to_string(),
        }
    }

    /// Check if the error is recoverable
    #[allow(dead_code)]
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::NetworkError { .. }
                | Self::AuthenticationFailed { .. }
                | Self::WorkingDirectoryNotClean
        )
    }

    /// Get user-friendly suggestion for recovery
    #[allow(dead_code)]
    pub fn recovery_suggestion(&self) -> Option<&'static str> {
        match self {
            Self::BranchNotFound { .. } => Some("Use 'git branch -a' to list all branches"),
            Self::WorkingDirectoryNotClean => Some("Commit or stash your changes first"),
            Self::RemoteNotFound { .. } => {
                Some("Use 'git remote add <name> <url>' to add a remote")
            }
            Self::AuthenticationFailed { .. } => Some("Check your SSH keys or credentials"),
            Self::NetworkError { .. } => Some("Check your internet connection and try again"),
            Self::MergeConflicts { .. } => Some("Resolve conflicts and commit the result"),
            _ => None,
        }
    }
}

/// Result type alias for Git operations
#[allow(dead_code)]
pub type GitResult<T> = Result<T, GitError>;

/// Trait for converting Git errors to more user-friendly formats
#[allow(dead_code)]
pub trait GitErrorExt<T> {
    /// Add context about the operation being performed
    fn with_operation(self, operation: &str) -> GitResult<T>;

    /// Add context about the repository
    fn with_repo(self, repo_path: &str) -> GitResult<T>;
}

#[allow(dead_code)]
impl<T> GitErrorExt<T> for Result<T, GitError> {
    fn with_operation(self, operation: &str) -> GitResult<T> {
        self.map_err(|e| GitError::CommandFailed {
            command: format!("{}: {}", operation, e),
            stderr: e.to_string(),
        })
    }

    fn with_repo(self, _repo_path: &str) -> GitResult<T> {
        self // The GitError already contains path information when relevant
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        // Test BranchNotFound with no remote
        let err = GitError::BranchNotFound {
            branch: "main".to_string(),
            remote: None,
        };
        assert!(matches!(
            err,
            GitError::BranchNotFound { branch, remote }
            if branch == "main" && remote.is_none()
        ));

        // Test BranchNotFound with remote
        let err = GitError::BranchNotFound {
            branch: "feature".to_string(),
            remote: Some("origin/feature".to_string()),
        };
        assert!(matches!(
            err,
            GitError::BranchNotFound { branch, remote }
            if branch == "feature" && remote == Some("origin/feature".to_string())
        ));

        // Test other error types
        let err = GitError::NotAGitRepository {
            path: "/tmp".to_string(),
        };
        assert!(matches!(
            err,
            GitError::NotAGitRepository { path }
            if path == "/tmp"
        ));

        let err = GitError::BranchAlreadyExists {
            branch: "test".to_string(),
        };
        assert!(matches!(
            err,
            GitError::BranchAlreadyExists { branch }
            if branch == "test"
        ));

        let err = GitError::MergeConflicts {
            count: 3,
            files: "file1.rs file2.rs file3.rs".to_string(),
        };
        assert!(matches!(
            err,
            GitError::MergeConflicts { count, files }
            if count == 3 && files.contains("file1.rs")
        ));
    }

    #[test]
    fn test_error_display() {
        // Test NotAGitRepository
        let err = GitError::NotAGitRepository {
            path: "/tmp".to_string(),
        };
        assert_eq!(err.to_string(), "Not a git repository: /tmp");

        // Test BranchNotFound with no remote
        let err = GitError::BranchNotFound {
            branch: "main".to_string(),
            remote: None,
        };
        assert_eq!(err.to_string(), "Branch 'main' not found");

        // Test BranchNotFound with remote
        let err = GitError::BranchNotFound {
            branch: "feature".to_string(),
            remote: Some("origin".to_string()),
        };
        assert!(err
            .to_string()
            .contains("Branch 'feature' not found (remote: origin)"));

        // Test BranchAlreadyExists
        let err = GitError::BranchAlreadyExists {
            branch: "test".to_string(),
        };
        assert_eq!(err.to_string(), "Branch 'test' already exists");

        // Test WorkingDirectoryNotClean
        let err = GitError::WorkingDirectoryNotClean;
        assert!(err
            .to_string()
            .contains("Working directory is not clean. Commit or stash changes first."));

        // Test NoCurrentBranch
        let err = GitError::NoCurrentBranch {
            commit: "abc123".to_string(),
        };
        assert!(err
            .to_string()
            .contains("No current branch (detached HEAD at abc123)"));

        // Test MergeConflicts
        let err = GitError::MergeConflicts {
            count: 2,
            files: "src/main.rs src/lib.rs".to_string(),
        };
        assert!(err
            .to_string()
            .contains("Merge conflicts detected in 2 file(s)"));

        // Test RemoteNotFound
        let err = GitError::RemoteNotFound {
            remote: "origin".to_string(),
        };
        assert_eq!(err.to_string(), "Remote 'origin' not found");

        // Test AuthenticationFailed
        let err = GitError::AuthenticationFailed {
            remote: "origin".to_string(),
        };
        assert!(err
            .to_string()
            .contains("Authentication failed for remote 'origin'"));

        // Test NetworkError
        let err = GitError::NetworkError {
            message: "Connection timeout".to_string(),
        };
        assert_eq!(err.to_string(), "Network error: Connection timeout");

        // Test CommandFailed
        let err = GitError::CommandFailed {
            command: "push".to_string(),
            stderr: "Permission denied".to_string(),
        };
        assert!(err
            .to_string()
            .contains("Git command failed: git push - Permission denied"));

        // Test PermissionDenied
        let err = GitError::PermissionDenied {
            operation: "create branch".to_string(),
        };
        assert_eq!(err.to_string(), "Permission denied: create branch");

        // Test LockError
        let err = GitError::LockError {
            message: "Environment already locked".to_string(),
        };
        assert_eq!(err.to_string(), "Lock error: Environment already locked");
    }

    #[test]
    fn test_recovery_suggestions() {
        // Test BranchNotFound
        let err = GitError::BranchNotFound {
            branch: "main".to_string(),
            remote: None,
        };
        assert_eq!(
            err.recovery_suggestion(),
            Some("Use 'git branch -a' to list all branches")
        );

        // Test WorkingDirectoryNotClean
        let err = GitError::WorkingDirectoryNotClean;
        assert_eq!(
            err.recovery_suggestion(),
            Some("Commit or stash your changes first")
        );

        // Test RemoteNotFound
        let err = GitError::RemoteNotFound {
            remote: "origin".to_string(),
        };
        assert_eq!(
            err.recovery_suggestion(),
            Some("Use 'git remote add <name> <url>' to add a remote")
        );

        // Test AuthenticationFailed
        let err = GitError::AuthenticationFailed {
            remote: "origin".to_string(),
        };
        assert_eq!(
            err.recovery_suggestion(),
            Some("Check your SSH keys or credentials")
        );

        // Test NetworkError
        let err = GitError::NetworkError {
            message: "Connection failed".to_string(),
        };
        assert_eq!(
            err.recovery_suggestion(),
            Some("Check your internet connection and try again")
        );

        // Test MergeConflicts
        let err = GitError::MergeConflicts {
            count: 1,
            files: "file.txt".to_string(),
        };
        assert_eq!(
            err.recovery_suggestion(),
            Some("Resolve conflicts and commit the result")
        );

        // Test errors without suggestions
        let err = GitError::BranchAlreadyExists {
            branch: "test".to_string(),
        };
        assert_eq!(err.recovery_suggestion(), None);

        let err = GitError::PermissionDenied {
            operation: "read".to_string(),
        };
        assert_eq!(err.recovery_suggestion(), None);

        let err = GitError::LockError {
            message: "Lock error".to_string(),
        };
        assert_eq!(err.recovery_suggestion(), None);
    }

    #[test]
    fn test_is_recoverable() {
        // Test recoverable errors
        let recoverable_errors = vec![
            GitError::NetworkError {
                message: "timeout".to_string(),
            },
            GitError::AuthenticationFailed {
                remote: "origin".to_string(),
            },
            GitError::WorkingDirectoryNotClean,
        ];

        for err in recoverable_errors {
            assert!(
                err.is_recoverable(),
                "Error should be recoverable: {:?}",
                err
            );
        }

        // Test non-recoverable errors
        let non_recoverable_errors = vec![
            GitError::NotAGitRepository {
                path: "/tmp".to_string(),
            },
            GitError::BranchNotFound {
                branch: "main".to_string(),
                remote: None,
            },
            GitError::BranchAlreadyExists {
                branch: "test".to_string(),
            },
            GitError::NoCurrentBranch {
                commit: "abc123".to_string(),
            },
            GitError::MergeConflicts {
                count: 1,
                files: "file.txt".to_string(),
            },
            GitError::RemoteNotFound {
                remote: "origin".to_string(),
            },
            GitError::CommandFailed {
                command: "push".to_string(),
                stderr: "error".to_string(),
            },
            GitError::PermissionDenied {
                operation: "read".to_string(),
            },
            GitError::LockError {
                message: "locked".to_string(),
            },
        ];

        for err in non_recoverable_errors {
            assert!(
                !err.is_recoverable(),
                "Error should not be recoverable: {:?}",
                err
            );
        }
    }

    #[test]
    fn test_git_error_ext() {
        // Test with_operation on Ok result
        let result: Result<String, GitError> = Ok("success".to_string());
        let result = result.with_operation("test operation");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");

        // Test with_operation on Err result
        let result: Result<String, GitError> = Err(GitError::NetworkError {
            message: "connection failed".to_string(),
        });
        let result = result.with_operation("git push");
        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::CommandFailed { command, stderr } => {
                assert!(command.contains("git push"));
                assert!(stderr.contains("Network error"));
            }
            _ => panic!("Expected CommandFailed error"),
        }

        // Test with_repo (should just pass through)
        let result: Result<String, GitError> = Err(GitError::BranchNotFound {
            branch: "main".to_string(),
            remote: None,
        });
        let result = result.with_repo("/path/to/repo");
        assert!(result.is_err());
        match result.unwrap_err() {
            GitError::BranchNotFound { branch, remote } => {
                assert_eq!(branch, "main");
                assert!(remote.is_none());
            }
            _ => panic!("Expected BranchNotFound error"),
        }
    }

    // Helper function tests
    #[test]
    fn test_branch_not_found_helper() {
        let err = GitError::branch_not_found("main");
        assert!(matches!(
            err,
            GitError::BranchNotFound { branch, remote }
            if branch == "main" && remote.is_none()
        ));
    }

    #[test]
    fn test_remote_branch_not_found_helper() {
        let err = GitError::remote_branch_not_found("feature", "origin");
        assert!(matches!(
            err,
            GitError::BranchNotFound { branch, remote }
            if branch == "feature" && remote == Some("origin/feature".to_string())
        ));
    }
}
