#[cfg(test)]
mod tests {
    use hitch::utils::conflict_report::{
        parse_conflict_type, ConflictType, ConflictedFile, MergeBaseInfo,
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
}
