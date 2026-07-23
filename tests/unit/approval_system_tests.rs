//! Unit tests for the approval system core functionality
//!
//! Tests the approval logic, data structures, and utility functions
//! in isolation from the CLI interface.

#[cfg(test)]
mod tests {
    use hitch::types::*;
    use std::collections::HashMap;

    fn create_test_approval_request() -> ApprovalRequest {
        let mut branch_shas = HashMap::new();
        branch_shas.insert("feature/test".to_string(), "abc123".to_string());

        ApprovalRequest::new(
            "production".to_string(),
            "feature/test".to_string(),
            Operation::Promote,
            "user@example.com".to_string(),
            RebuildSnapshot {
                base_branch: "main".to_string(),
                base_sha: "def456".to_string(),
                branch_shas,
                merge_conflicts: false,
            },
        )
    }

    fn create_test_environment() -> Environment {
        Environment {
            base: "main".to_string(),
            branches: vec![],
            locked: false,
            locked_by: None,
            locked_at: None,
            rebuilt_at: None,
            released_at: None,
            requires_approval: true,
            approvers: vec![
                "alice@example.com".to_string(),
                "bob@example.com".to_string(),
                "carol@example.com".to_string(),
            ],
            min_approvals: 2,
            on_conflict: OnConflict::default(),
        }
    }

    #[test]
    fn test_approval_request_creation() {
        let request = create_test_approval_request();

        assert_eq!(request.environment, "production");
        assert_eq!(request.branch, "feature/test");
        assert_eq!(request.operation, Operation::Promote);
        assert_eq!(request.requested_by, "user@example.com");
        assert_eq!(request.status, ApprovalStatus::Pending);
        assert_eq!(request.approvals.len(), 0);
        assert!(!request.has_approved("alice@example.com"));
        assert_eq!(request.approval_count(), 0);
        assert!(!request.threshold_met(2));
    }

    #[test]
    fn test_approval_request_approval_process() {
        let mut request = create_test_approval_request();

        // Test adding first approval
        request.add_approval(
            "alice@example.com".to_string(),
            Some("Looks good to me".to_string()),
        );

        assert_eq!(request.approvals.len(), 1);
        assert!(request.has_approved("alice@example.com"));
        assert!(!request.has_approved("bob@example.com"));
        assert_eq!(request.approval_count(), 1);
        assert!(!request.threshold_met(2));

        // Test adding second approval
        request.add_approval("bob@example.com".to_string(), None);

        assert_eq!(request.approvals.len(), 2);
        assert!(request.has_approved("alice@example.com"));
        assert!(request.has_approved("bob@example.com"));
        assert_eq!(request.approval_count(), 2);
        assert!(request.threshold_met(2));

        // Test approval details
        let alice_approval = &request.approvals[0];
        assert_eq!(alice_approval.approved_by, "alice@example.com");
        assert_eq!(alice_approval.comment, Some("Looks good to me".to_string()));
    }

    #[test]
    fn test_approval_request_rejection() {
        let mut request = create_test_approval_request();

        request.reject(
            "alice@example.com".to_string(),
            "Not ready for production".to_string(),
        );

        assert_eq!(request.status, ApprovalStatus::Rejected);
        assert!(request.rejection.is_some());
        let rejection = request.rejection.as_ref().unwrap();
        assert_eq!(rejection.rejected_by, "alice@example.com");
        assert_eq!(rejection.reason, "Not ready for production");
    }

    #[test]
    fn test_approval_request_cancellation() {
        let mut request = create_test_approval_request();

        request.cancel();

        assert_eq!(request.status, ApprovalStatus::Cancelled);
        assert!(request.rejection.is_none());
    }

    #[test]
    fn test_approval_request_lifecycle() {
        let mut request = create_test_approval_request();

        // Initial state
        assert_eq!(request.status, ApprovalStatus::Pending);

        // Mark as approved
        request.mark_approved();
        assert_eq!(request.status, ApprovalStatus::Approved);

        // Mark as applied
        request.mark_applied();
        assert_eq!(request.status, ApprovalStatus::Applied);
    }

    #[test]
    fn test_environment_approval_configuration() {
        let mut env = create_test_environment();

        // Test approval requirement check
        assert!(env.requires_approval_check());

        // Test approver validation
        assert!(env.is_approver("alice@example.com"));
        assert!(env.is_approver("bob@example.com"));
        assert!(env.is_approver("carol@example.com"));
        assert!(!env.is_approver("unauthorized@example.com"));

        // Test branch management
        assert!(env.branches.is_empty());
        env.add_branch("feature/test".to_string());
        assert!(env.branches.contains(&"feature/test".to_string()));
        assert_eq!(env.branches.len(), 1);

        env.remove_branch("feature/test");
        assert!(!env.branches.contains(&"feature/test".to_string()));
        assert_eq!(env.branches.len(), 0);
    }

    #[test]
    fn test_environment_approval_configuration_validation() {
        // Valid configuration
        let valid_env = Environment {
            base: "main".to_string(),
            branches: vec![],
            locked: false,
            locked_by: None,
            locked_at: None,
            rebuilt_at: None,
            released_at: None,
            requires_approval: true,
            approvers: vec![
                "alice@example.com".to_string(),
                "bob@example.com".to_string(),
            ],
            min_approvals: 2,
            on_conflict: OnConflict::default(),
        };
        assert!(valid_env.validate_approval_config().is_ok());

        // Test empty approvers list
        let mut invalid_env = valid_env.clone();
        invalid_env.approvers.clear();
        assert!(invalid_env.validate_approval_config().is_err());

        // Test min_approvals = 0
        let mut invalid_env = valid_env.clone();
        invalid_env.min_approvals = 0;
        assert!(invalid_env.validate_approval_config().is_err());

        // Test min_approvals > approvers
        let mut invalid_env = valid_env.clone();
        invalid_env.min_approvals = 3;
        assert!(invalid_env.validate_approval_config().is_err());

        // Test invalid email format
        let mut invalid_env = valid_env.clone();
        invalid_env.approvers = vec!["invalid-email".to_string()];
        assert!(invalid_env.validate_approval_config().is_err());

        // Test no approval required (should be valid)
        let mut no_approval_env = valid_env.clone();
        no_approval_env.requires_approval = false;
        no_approval_env.approvers.clear();
        no_approval_env.min_approvals = 1;
        assert!(no_approval_env.validate_approval_config().is_ok());
    }

    #[test]
    fn test_hitch_config_approval_request_management() {
        let mut config = HitchConfig::new();

        // Test adding approval request
        let request = create_test_approval_request();
        let request_id = request.id.clone();
        config.add_approval_request(request);

        assert_eq!(config.approval_requests.len(), 1);

        // Test finding approval request
        let found = config.get_approval_request(&request_id);
        assert!(found.is_some());
        let found_request = found.unwrap();
        assert_eq!(found_request.id, request_id);
        assert_eq!(found_request.environment, "production");

        // Test finding approval request by environment
        let env_requests = config.get_approval_requests_for_env("production");
        assert_eq!(env_requests.len(), 1);
        assert_eq!(env_requests[0].id, request_id);

        let dev_requests = config.get_approval_requests_for_env("dev");
        assert_eq!(dev_requests.len(), 0);

        // Test getting all approval requests
        let all_requests = config.get_approval_requests();
        assert_eq!(all_requests.len(), 1);
    }

    #[test]
    fn test_approval_status_serialization() {
        // Test all approval statuses can be serialized/deserialized
        let statuses = [
            ApprovalStatus::Pending,
            ApprovalStatus::Approved,
            ApprovalStatus::Applied,
            ApprovalStatus::Rejected,
            ApprovalStatus::Cancelled,
        ];

        for status in &statuses {
            let serialized = serde_json::to_string(status).unwrap();
            let deserialized: ApprovalStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn test_operation_serialization() {
        // Test all operations can be serialized/deserialized
        let operations = [Operation::Promote, Operation::Demote];

        for op in &operations {
            let serialized = serde_json::to_string(op).unwrap();
            let deserialized: Operation = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*op, deserialized);
        }
    }

    #[test]
    fn test_approval_request_full_serialization() {
        let mut request = create_test_approval_request();

        // Add some approvals
        request.add_approval("alice@example.com".to_string(), Some("LGTM".to_string()));
        request.add_approval("bob@example.com".to_string(), None);

        // Serialize and deserialize
        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: ApprovalRequest = serde_json::from_str(&serialized).unwrap();

        // Verify all fields are preserved
        assert_eq!(deserialized.id, request.id);
        assert_eq!(deserialized.environment, request.environment);
        assert_eq!(deserialized.branch, request.branch);
        assert_eq!(deserialized.operation, request.operation);
        assert_eq!(deserialized.requested_by, request.requested_by);
        assert_eq!(deserialized.status, request.status);
        assert_eq!(deserialized.approvals.len(), request.approvals.len());
        assert_eq!(deserialized.approvals[0].approved_by, "alice@example.com");
        assert_eq!(deserialized.approvals[0].comment, Some("LGTM".to_string()));
        assert_eq!(deserialized.approvals[1].approved_by, "bob@example.com");
        assert_eq!(deserialized.approvals[1].comment, None);
        assert_eq!(
            deserialized.rebuild_snapshot.base_sha,
            request.rebuild_snapshot.base_sha
        );
    }

    #[test]
    fn test_rebuild_snapshot_functionality() {
        let mut branch_shas = HashMap::new();
        branch_shas.insert("feature/branch1".to_string(), "abc123".to_string());
        branch_shas.insert("feature/branch2".to_string(), "def456".to_string());

        let snapshot = RebuildSnapshot {
            base_branch: "main".to_string(),
            base_sha: "base123".to_string(),
            branch_shas: branch_shas.clone(),
            merge_conflicts: false,
        };

        assert_eq!(snapshot.base_branch, "main");
        assert_eq!(snapshot.base_sha, "base123");
        assert_eq!(snapshot.branch_shas.len(), 2);
        assert!(!snapshot.merge_conflicts);
        assert_eq!(
            snapshot.branch_shas.get("feature/branch1"),
            Some(&"abc123".to_string())
        );
    }

    #[test]
    fn test_approval_status_display() {
        let statuses = [
            (ApprovalStatus::Pending, "Pending"),
            (ApprovalStatus::Approved, "Approved"),
            (ApprovalStatus::Applied, "Applied"),
            (ApprovalStatus::Rejected, "Rejected"),
            (ApprovalStatus::Cancelled, "Cancelled"),
        ];

        for (status, expected_display) in &statuses {
            let display = format!("{}", status);
            assert_eq!(display, *expected_display);
        }
    }

    #[test]
    fn test_operation_display() {
        let operations = [
            (Operation::Promote, "Promote"),
            (Operation::Demote, "Demote"),
        ];

        for (op, expected_display) in &operations {
            let display = format!("{}", op);
            assert_eq!(display, *expected_display);
        }
    }

    // Tests for Bug #1/#2: Snapshot with custom base branch
    #[test]
    fn test_rebuild_snapshot_custom_base_branch() {
        let mut branch_shas = HashMap::new();
        branch_shas.insert("feature/auth".to_string(), "abc123def456".to_string());

        // Test with "develop" as base branch (not "main")
        let snapshot = RebuildSnapshot {
            base_branch: "develop".to_string(),
            base_sha: "fedcba987654".to_string(),
            branch_shas,
            merge_conflicts: false,
        };

        assert_eq!(snapshot.base_branch, "develop");
        assert_eq!(snapshot.base_sha, "fedcba987654");

        // Test with "staging" as base branch
        let staging_snapshot = RebuildSnapshot {
            base_branch: "staging".to_string(),
            base_sha: "111222333444".to_string(),
            branch_shas: HashMap::new(),
            merge_conflicts: false,
        };

        assert_eq!(staging_snapshot.base_branch, "staging");
        assert_eq!(staging_snapshot.base_sha, "111222333444");
    }

    // Tests for Issue #5: Rejected requests can have approvals
    #[test]
    fn test_rejected_request_with_approvals() {
        let mut request = create_test_approval_request();

        // Add one approval
        request.add_approval(
            "alice@example.com".to_string(),
            Some("Looks good".to_string()),
        );
        assert_eq!(request.approval_count(), 1);
        assert_eq!(request.status, ApprovalStatus::Pending);

        // Reject the request (before reaching threshold of 2)
        request.reject(
            "bob@example.com".to_string(),
            "Actually, needs more testing".to_string(),
        );

        // Verify the request is rejected
        assert_eq!(request.status, ApprovalStatus::Rejected);

        // IMPORTANT: The request should still have the approval that was recorded before rejection
        assert_eq!(request.approval_count(), 1);
        assert!(request.has_approved("alice@example.com"));

        // Verify rejection details
        assert!(request.rejection.is_some());
        let rejection = request.rejection.as_ref().unwrap();
        assert_eq!(rejection.rejected_by, "bob@example.com");
        assert_eq!(rejection.reason, "Actually, needs more testing");
    }

    #[test]
    fn test_rebuild_snapshot_serialization_with_base_branch() {
        let mut branch_shas = HashMap::new();
        branch_shas.insert("feature/test".to_string(), "abc123".to_string());

        let snapshot = RebuildSnapshot {
            base_branch: "develop".to_string(),
            base_sha: "def456".to_string(),
            branch_shas,
            merge_conflicts: true,
        };

        // Serialize
        let serialized = serde_json::to_string(&snapshot).unwrap();

        // Deserialize
        let deserialized: RebuildSnapshot = serde_json::from_str(&serialized).unwrap();

        // Verify all fields including base_branch
        assert_eq!(deserialized.base_branch, "develop");
        assert_eq!(deserialized.base_sha, "def456");
        assert_eq!(deserialized.branch_shas.len(), 1);
        assert!(deserialized.merge_conflicts);
    }

    #[test]
    fn test_approval_threshold_with_varying_min_approvals() {
        let mut request = create_test_approval_request();

        // No approvals yet
        assert!(!request.threshold_met(1));
        assert!(!request.threshold_met(2));
        assert!(!request.threshold_met(3));

        // Add first approval
        request.add_approval("alice@example.com".to_string(), None);
        assert!(request.threshold_met(1)); // Threshold of 1 is met
        assert!(!request.threshold_met(2)); // Threshold of 2 not met
        assert!(!request.threshold_met(3)); // Threshold of 3 not met

        // Add second approval
        request.add_approval("bob@example.com".to_string(), None);
        assert!(request.threshold_met(1)); // Still met
        assert!(request.threshold_met(2)); // Now threshold of 2 is met
        assert!(!request.threshold_met(3)); // Threshold of 3 not met

        // Add third approval
        request.add_approval("carol@example.com".to_string(), None);
        assert!(request.threshold_met(1)); // Still met
        assert!(request.threshold_met(2)); // Still met
        assert!(request.threshold_met(3)); // Now threshold of 3 is met
    }
}
