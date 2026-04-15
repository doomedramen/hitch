#[cfg(test)]
mod tests {
    use hitch::utils::conflict_report::{
        format_conflict_report, parse_conflict_type, ConflictType, ConflictedFile, MergeBaseInfo,
    };

    #[test]
    fn test_parse_conflict_type() {
        // Test all standard conflict types
        assert_eq!(parse_conflict_type("UU"), ConflictType::ModifyModify);
        assert_eq!(parse_conflict_type("AA"), ConflictType::AddAdd);
        assert_eq!(parse_conflict_type("UD"), ConflictType::ModifyDelete);
        assert_eq!(parse_conflict_type("DU"), ConflictType::DeleteModify);
        assert_eq!(parse_conflict_type("AU"), ConflictType::AddAdd);
        assert_eq!(parse_conflict_type("UA"), ConflictType::AddAdd);

        // Test unknown types
        assert_eq!(parse_conflict_type("DD"), ConflictType::Unknown);
        assert_eq!(parse_conflict_type("??"), ConflictType::Unknown);
        assert_eq!(parse_conflict_type("ZZ"), ConflictType::Unknown);

        // Test with extra whitespace
        assert_eq!(parse_conflict_type(" UU "), ConflictType::ModifyModify);
        assert_eq!(parse_conflict_type("AA\n"), ConflictType::AddAdd);
    }

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
            ConflictType::DeleteModify.to_string(),
            "deleted in source, modified in target"
        );
        assert_eq!(
            ConflictType::AddAdd.to_string(),
            "both branches added with different content"
        );
        assert_eq!(
            ConflictType::RenameRename.to_string(),
            "both branches renamed differently"
        );
        assert_eq!(ConflictType::Unknown.to_string(), "unknown conflict type");
    }

    #[test]
    fn test_conflicted_file_creation() {
        // Test basic creation
        let file = ConflictedFile::new("src/main.rs".to_string(), ConflictType::ModifyModify);
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.conflict_type, ConflictType::ModifyModify);
        assert!(file.conflict_content.is_none());

        // Test creation with content
        let content = "<<<<<<< HEAD\nold line\n=======\nnew line\n>>>>>>> feature";
        let file_with_content = ConflictedFile::with_content(
            "Cargo.toml".to_string(),
            ConflictType::AddAdd,
            content.to_string(),
        );
        assert_eq!(file_with_content.path, "Cargo.toml");
        assert_eq!(file_with_content.conflict_type, ConflictType::AddAdd);
        assert_eq!(file_with_content.conflict_content.unwrap(), content);
    }

    #[test]
    fn test_merge_base_info() {
        let base = MergeBaseInfo::new("abc123def4567890".to_string());
        assert_eq!(base.short_hash, "abc123d");
        assert_eq!(base.commit_hash, "abc123def4567890");
        assert!(base.date.is_none());

        let base_with_date =
            MergeBaseInfo::new("abc123def4567890".to_string()).with_date("2024-12-04".to_string());
        assert_eq!(base_with_date.date.unwrap(), "2024-12-04");

        // Test with short hash
        let short_base = MergeBaseInfo::new("abc123".to_string());
        assert_eq!(short_base.short_hash, "abc123");
    }

    #[test]
    fn test_format_conflict_report_empty() {
        let report = format_conflict_report("feature-branch", "main", "main", "staging", &[], None);

        assert!(
            report.contains("Merge conflict detected when merging 'feature-branch' into 'main'")
        );
        assert!(report.contains("Conflicting files (0):"));
        assert!(report.contains("To resolve:"));
        assert!(report.contains("hitch rebuild staging"));
    }

    #[test]
    fn test_format_conflict_report_single_file() {
        let conflict = ConflictedFile::with_content(
        "src/config.rs".to_string(),
        ConflictType::ModifyModify,
        "<<<<<<< HEAD\n    pub fn timeout() -> u64 {\n        30\n    }\n=======\n    pub fn timeout() -> u64 {\n        60\n    }\n>>>>>>> feature-branch".to_string(),
    );

        let merge_base =
            MergeBaseInfo::new("abc123def".to_string()).with_date("2024-12-04".to_string());

        let report = format_conflict_report(
            "feature-branch",
            "hitch-tmp-main-20241204",
            "main",
            "dev",
            &[conflict],
            Some(&merge_base),
        );

        // Check all expected sections are present
        assert!(report.contains(
            "Merge conflict detected when merging 'feature-branch' into 'hitch-tmp-main-20241204'"
        ));
        assert!(report.contains("Merge base: abc123d (2024-12-04)"));
        assert!(report.contains("Conflicting files (1):"));
        assert!(report.contains("src/config.rs"));
        assert!(report.contains("MM"));
        assert!(report.contains("both branches modified"));
        assert!(report.contains("<<<<<<< HEAD"));
        assert!(report.contains("30"));
        assert!(report.contains("60"));
        assert!(report.contains(">>>>>>> feature-branch"));
        assert!(report.contains("To resolve:"));
        assert!(report.contains("git checkout feature-branch"));
        assert!(report.contains("git rebase main"));
        assert!(report.contains("hitch rebuild dev"));
    }

    #[test]
    fn test_format_conflict_report_multiple_files() {
        let conflicts = vec![
            ConflictedFile::new("Cargo.toml".to_string(), ConflictType::ModifyModify),
            ConflictedFile::with_content(
                "src/lib.rs".to_string(),
                ConflictType::ModifyDelete,
                "<<<<<<< HEAD\nuse std::collections::HashMap;\n=======\n>>>>>>> feature"
                    .to_string(),
            ),
            ConflictedFile::new("README.md".to_string(), ConflictType::DeleteModify),
        ];

        let report = format_conflict_report(
            "feature-branch",
            "hitch-tmp-main-20241204",
            "main",
            "staging",
            &conflicts,
            None,
        );

        assert!(report.contains("Conflicting files (3):"));
        assert!(report.contains("Cargo.toml"));
        assert!(report.contains("src/lib.rs"));
        assert!(report.contains("README.md"));
        assert!(report.contains("MM"));
        assert!(report.contains("MD"));
        assert!(report.contains("DM"));
        assert!(report.contains("modified in source, deleted in target"));
        assert!(report.contains("deleted in source, modified in target"));
    }

    #[test]
    fn test_format_conflict_report_with_merge_base() {
        let conflict = ConflictedFile::new("file.txt".to_string(), ConflictType::AddAdd);

        let merge_base = MergeBaseInfo::new("abc123def".to_string());

        let report = format_conflict_report(
            "branch-a",
            "branch-b",
            "main",
            "dev",
            &[conflict],
            Some(&merge_base),
        );

        assert!(report.contains("Merge base: abc123d"));
        assert!(!report.contains("Merge base: abc123d ()"));
    }

    #[test]
    fn test_format_conflict_report_without_merge_base() {
        let conflict = ConflictedFile::new("file.txt".to_string(), ConflictType::AddAdd);

        let report =
            format_conflict_report("branch-a", "branch-b", "main", "dev", &[conflict], None);

        assert!(!report.contains("Merge base:"));
        assert!(report.contains("Merge conflict detected"));
    }

    #[test]
    fn test_format_conflict_report_with_binary_file() {
        let conflict = ConflictedFile::with_content(
            "image.png".to_string(),
            ConflictType::AddAdd,
            "(Binary file or conflict markers not available)".to_string(),
        );

        let report = format_conflict_report(
            "feature-branch",
            "main",
            "main",
            "staging",
            &[conflict],
            None,
        );

        assert!(report.contains("image.png"));
        assert!(report.contains("Binary file or conflict markers not available"));
    }

    #[test]
    fn test_format_conflict_report_multiple_conflicts_in_single_file() {
        let conflict = ConflictedFile::with_content(
        "src/main.rs".to_string(),
        ConflictType::ModifyModify,
        "<<<<<<< HEAD\nlet x = 1;\n=======\nlet x = 2;\n>>>>>>> branch\n---\n\n<<<<<<< HEAD\nlet y = 3;\n=======\nlet y = 4;\n>>>>>>> branch".to_string(),
    );

        let report =
            format_conflict_report("branch-a", "branch-b", "main", "dev", &[conflict], None);

        assert!(report.contains("let x = 1;"));
        assert!(report.contains("let x = 2;"));
        assert!(report.contains("let y = 3;"));
        assert!(report.contains("let y = 4;"));
        assert!(report.contains("---"));
    }

    #[test]
    fn test_conflict_type_short_format() {
        // The internal helper function formats conflict types as 2-letter codes
        // This is tested indirectly through format_conflict_report which shows the codes
        let conflict = ConflictedFile::new("test.rs".to_string(), ConflictType::ModifyModify);

        let report = format_conflict_report("feature", "main", "main", "dev", &[conflict], None);

        assert!(report.contains("MM")); // Short code for ModifyModify
    }
}
