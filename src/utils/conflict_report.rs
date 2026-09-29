//! Conflict report formatting for merge conflicts
//!
//! Provides detailed, human-readable reports for merge conflicts
//! to help users diagnose and resolve issues.

use std::fmt;

/// Represents the type of merge conflict
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictType {
    /// Both branches modified the same file
    ModifyModify,
    /// One branch modified, the other deleted
    ModifyDelete,
    /// One branch deleted, the other modified
    DeleteModify,
    /// Both branches added the same file with different content
    AddAdd,
    /// Both branches renamed the file differently
    RenameRename,
    /// Unknown conflict type
    Unknown,
}

impl fmt::Display for ConflictType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConflictType::ModifyModify => write!(f, "both branches modified"),
            ConflictType::ModifyDelete => write!(f, "modified in source, deleted in target"),
            ConflictType::DeleteModify => write!(f, "deleted in source, modified in target"),
            ConflictType::AddAdd => write!(f, "both branches added with different content"),
            ConflictType::RenameRename => write!(f, "both branches renamed differently"),
            ConflictType::Unknown => write!(f, "unknown conflict type"),
        }
    }
}

/// Detailed information about a conflicted file
#[derive(Debug, Clone)]
pub struct ConflictedFile {
    /// Path to the conflicted file
    pub path: String,
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// The actual conflict content with markers (<<<<<<, ======, >>>>>>)
    pub conflict_content: Option<String>,
}

impl ConflictedFile {
    /// Create a new ConflictedFile with basic info
    pub fn new(path: String, conflict_type: ConflictType) -> Self {
        Self {
            path,
            conflict_type,
            conflict_content: None,
        }
    }

    /// Create a new ConflictedFile with conflict content
    pub fn with_content(path: String, conflict_type: ConflictType, content: String) -> Self {
        Self {
            path,
            conflict_type,
            conflict_content: Some(content),
        }
    }
}

/// Information about the merge base
#[derive(Debug, Clone)]
pub struct MergeBaseInfo {
    /// The commit hash of the merge base
    pub commit_hash: String,
    /// Short form of the commit hash
    pub short_hash: String,
    /// Date of the merge base commit (optional)
    pub date: Option<String>,
}

impl MergeBaseInfo {
    pub fn new(commit_hash: String) -> Self {
        let short_hash = if commit_hash.len() >= 7 {
            commit_hash[..7].to_string()
        } else {
            commit_hash.clone()
        };
        Self {
            commit_hash,
            short_hash,
            date: None,
        }
    }

    pub fn with_date(mut self, date: String) -> Self {
        self.date = Some(date);
        self
    }
}

/// Parse conflict type from git status output
///
/// Git uses two-letter codes to indicate file status:
/// - DD: both deleted
/// - AU: added by us
/// - UD: deleted by them
/// - UA: added by them
/// - DU: deleted by us
/// - AA: both added
/// - UU: both modified
pub fn parse_conflict_type(status_code: &str) -> ConflictType {
    match status_code.trim() {
        "UU" => ConflictType::ModifyModify,
        "AA" => ConflictType::AddAdd,
        "UD" => ConflictType::ModifyDelete,
        "DU" => ConflictType::DeleteModify,
        "AU" | "UA" => ConflictType::AddAdd,
        _ => ConflictType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_type_display() {
        assert_eq!(
            ConflictType::ModifyModify.to_string(),
            "both branches modified"
        );
        assert_eq!(
            ConflictType::ModifyDelete.to_string(),
            "modified in source, deleted in target"
        );
        assert_eq!(
            ConflictType::AddAdd.to_string(),
            "both branches added with different content"
        );
    }

    #[test]
    fn test_parse_conflict_type() {
        assert_eq!(parse_conflict_type("UU"), ConflictType::ModifyModify);
        assert_eq!(parse_conflict_type("AA"), ConflictType::AddAdd);
        assert_eq!(parse_conflict_type("UD"), ConflictType::ModifyDelete);
        assert_eq!(parse_conflict_type("DU"), ConflictType::DeleteModify);
        assert_eq!(parse_conflict_type("XX"), ConflictType::Unknown);
    }

    #[test]
    fn test_conflicted_file_creation() {
        let file = ConflictedFile::new("src/main.rs".to_string(), ConflictType::ModifyModify);
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.conflict_type, ConflictType::ModifyModify);
        assert!(file.conflict_content.is_none());

        let file_with_content = ConflictedFile::with_content(
            "Cargo.toml".to_string(),
            ConflictType::ModifyModify,
            "<<<<<<< HEAD\nversion = \"1.0\"\n=======\nversion = \"2.0\"\n>>>>>>> branch"
                .to_string(),
        );
        assert!(file_with_content.conflict_content.is_some());
    }

    #[test]
    fn test_merge_base_info() {
        let base = MergeBaseInfo::new("abc123def456789".to_string());
        assert_eq!(base.short_hash, "abc123d");
        assert!(base.date.is_none());

        let base_with_date = base.with_date("2024-12-01".to_string());
        assert_eq!(base_with_date.date, Some("2024-12-01".to_string()));
    }
}
