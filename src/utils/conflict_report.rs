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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

/// Format a detailed conflict report
///
/// # Arguments
/// * `source_branch` - The branch being merged (e.g., feature branch)
/// * `target_branch` - The branch being merged into (e.g., environment branch)
/// * `base_branch` - The base branch for rebuilds (e.g., main)
/// * `env_name` - The environment name
/// * `conflicts` - List of conflicted files with details
/// * `merge_base` - Optional merge base information
pub fn format_conflict_report(
    source_branch: &str,
    target_branch: &str,
    base_branch: &str,
    env_name: &str,
    conflicts: &[ConflictedFile],
    merge_base: Option<&MergeBaseInfo>,
) -> String {
    let mut report = String::new();

    // Header
    report.push_str(&format!(
        "Merge conflict detected when merging '{}' into '{}'\n\n",
        source_branch, target_branch
    ));

    // Merge base info
    if let Some(base) = merge_base {
        if let Some(date) = &base.date {
            report.push_str(&format!("Merge base: {} ({})\n\n", base.short_hash, date));
        } else {
            report.push_str(&format!("Merge base: {}\n\n", base.short_hash));
        }
    }

    // Conflicting files section
    let file_count = conflicts.len();
    report.push_str(&format!("Conflicting files ({}):\n", file_count));
    report.push_str(&format!("{}\n\n", "=".repeat(56)));

    for conflict in conflicts {
        // File header with conflict type
        report.push_str(&format!(
            "  {} ({} - {})\n",
            conflict.path,
            format_conflict_type_short(&conflict.conflict_type),
            conflict.conflict_type
        ));
        report.push_str(&format!("{}\n", "-".repeat(56)));

        // Conflict content if available
        if let Some(content) = &conflict.conflict_content {
            // Indent the conflict content for readability
            for line in content.lines() {
                report.push_str(&format!("  {}\n", line));
            }
        } else {
            report.push_str("  (Conflict content not available)\n");
        }
        report.push('\n');
    }

    report.push_str(&format!("{}\n\n", "=".repeat(56)));

    // Resolution instructions
    report.push_str("To resolve:\n");
    report.push_str(&format!(
        "1. Checkout the conflicting branch and rebase onto {}:\n",
        base_branch
    ));
    report.push_str(&format!("   git checkout {}\n", source_branch));
    report.push_str(&format!("   git rebase {}\n\n", base_branch));
    report.push_str("2. Resolve conflicts in the files listed above\n\n");
    report.push_str("3. Commit the resolution:\n");
    report.push_str("   git add .\n");
    report.push_str(&format!(
        "   git commit -m \"Resolve merge conflicts with {}\"\n\n",
        base_branch
    ));
    report.push_str("4. Retry the operation:\n");
    report.push_str(&format!("   hitch rebuild {}\n", env_name));

    report
}

/// Format conflict type as a short code
fn format_conflict_type_short(conflict_type: &ConflictType) -> &'static str {
    match conflict_type {
        ConflictType::ModifyModify => "MM",
        ConflictType::ModifyDelete => "MD",
        ConflictType::DeleteModify => "DM",
        ConflictType::AddAdd => "AA",
        ConflictType::RenameRename => "RR",
        ConflictType::Unknown => "??",
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

    #[test]
    fn test_format_conflict_report() {
        let conflicts = vec![
            ConflictedFile::with_content(
                "src/config.rs".to_string(),
                ConflictType::ModifyModify,
                "<<<<<<< HEAD\nold_value\n=======\nnew_value\n>>>>>>> feature".to_string(),
            ),
            ConflictedFile::new("README.md".to_string(), ConflictType::ModifyDelete),
        ];

        let merge_base =
            MergeBaseInfo::new("abc1234567890".to_string()).with_date("2024-12-01".to_string());

        let report = format_conflict_report(
            "feature-branch",
            "hitch-tmp-main-123",
            "main",
            "dev",
            &conflicts,
            Some(&merge_base),
        );

        // Check report contains expected sections
        assert!(report.contains("Merge conflict detected when merging 'feature-branch'"));
        assert!(report.contains("Merge base: abc1234"));
        assert!(report.contains("Conflicting files (2)"));
        assert!(report.contains("src/config.rs"));
        assert!(report.contains("README.md"));
        assert!(report.contains("To resolve:"));
        assert!(report.contains("git checkout feature-branch"));
        assert!(report.contains("git rebase main"));
        assert!(report.contains("hitch rebuild dev"));
    }

    #[test]
    fn test_format_conflict_report_without_merge_base() {
        let conflicts = vec![ConflictedFile::new(
            "file.txt".to_string(),
            ConflictType::Unknown,
        )];

        let report =
            format_conflict_report("branch-a", "branch-b", "main", "staging", &conflicts, None);

        assert!(!report.contains("Merge base:"));
        assert!(report.contains("Conflicting files (1)"));
    }
}
