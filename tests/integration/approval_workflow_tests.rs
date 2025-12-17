//! Integration tests for hitch approval workflow system
//!
//! Tests the complete approval workflow including:
//! - Approval request creation
//! - Authorization and validation
//! - Multi-approval thresholds
//! - Automatic operation execution
//! - Status tracking and listing

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// Helper to create environment with approval configuration
    fn create_approval_environment(
        env: &TestEnvironment,
        env_name: &str,
        approvers: &[&str],
        min_approvals: usize,
    ) -> anyhow::Result<()> {
        // Add environment normally first
        env.hitch
            .run()
            .args(&["add", env_name])
            .execute()?
            .assert_success();

        // Modify hitch.json to add approval configuration
        env.git.run(&["checkout", "hitch-metadata"])?;
        let mut config = env.read_hitch_config()?;
        let environment = config.environments.get_mut(env_name).unwrap();
        environment.requires_approval = true;
        environment.approvers = approvers.iter().map(|s| s.to_string()).collect();
        environment.min_approvals = min_approvals;

        // Write back the configuration
        env.fs.write_json("hitch.json", &config)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git.run(&[
            "commit",
            "-m",
            &format!("Enable approval workflow for {}", env_name),
        ])?;
        env.git.run(&["checkout", "main"])?;

        Ok(())
    }

    #[test]
    fn test_approval_request_creation() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with approval requirements
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create feature branch with changes
            env.git.run(&["checkout", "-b", "feature/auth"])?;
            env.fs.write_file("auth.js", "// Authentication logic")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add authentication"])?;

            // Try to promote - should create approval request instead
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature/auth", "production"])
                .execute()?;

            result
                .assert_success()
                .assert_stdout_contains(
                    "Environment 'production' requires approval before promotion",
                )
                .assert_stdout_contains("Approval request created")
                .assert_stdout_contains("Waiting for 2 approval(s)")
                .assert_stdout_contains("alice@example.com")
                .assert_stdout_contains("bob@example.com");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_approval_authorization() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with approval requirements
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create and submit an approval request as Carol (not an approver)
            env.git.config_user("Carol", "carol@example.com")?;
            env.git.run(&["checkout", "-b", "feature/payment"])?;
            env.fs.write_file("payment.js", "// Payment processing")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add payment processing"])?;
            env.git.run(&["checkout", "main"])?;

            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/payment", "production"])
                .execute()?;
            promote_result.assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();

            // Extract request ID (first UUID in output)
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/payment"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // Test 1: Unauthorized user cannot approve
            env.git.config_user("Charlie", "charlie@example.com")?;
            let unauthorized_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            unauthorized_result
                .assert_failure()
                .assert_stderr_contains("You are not authorized to approve requests");

            // Test 2: Self-approval is prevented (Carol is the requester but not an approver)
            env.git.config_user("Carol", "carol@example.com")?;
            let self_approval_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            self_approval_result
                .assert_failure()
                .assert_stderr_contains("You are not authorized to approve requests");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_multi_approval_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment requiring 2 approvals
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/dashboard"])?;
            env.fs
                .write_file("dashboard.js", "// Dashboard component")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add dashboard"])?;
            env.git.run(&["checkout", "main"])?;

            // Create approval request
            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/dashboard", "production"])
                .execute()?;
            promote_result.assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/dashboard"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // First approval by Alice
            env.git.config_user("Alice", "alice@example.com")?;
            let approve1_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            approve1_result
                .assert_success()
                .assert_stdout_contains("Approval recorded")
                .assert_stdout_contains("1/2")
                .assert_stdout_contains("Waiting for 1 more approval(s)");

            // Second approval should trigger execution
            env.git.config_user("Bob", "bob@example.com")?;
            let approve2_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            approve2_result
                .assert_success()
                .assert_stdout_contains("Approval threshold met")
                .assert_stdout_contains("executing operation")
                .assert_stdout_contains("approved and operation executed successfully");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_approval_request_rejection() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with approval requirements
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/bugfix"])?;
            env.fs
                .write_file("bugfix.js", "// Bug fix implementation")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Fix critical bug"])?;
            env.git.run(&["checkout", "main"])?;

            // Create approval request
            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/bugfix", "production"])
                .execute()?;
            promote_result.assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/bugfix"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // Reject the request with detailed reason
            env.git.config_user("Alice", "alice@example.com")?;
            let reject_result = env
                .hitch
                .run()
                .args(&[
                    "approvals",
                    "reject",
                    request_id,
                    "Tests are failing for this bug fix, please add unit tests before merging",
                ])
                .execute()?;
            reject_result
                .assert_success()
                .assert_stdout_contains("rejected successfully")
                .assert_stdout_contains("Reason: Tests are failing for this bug fix");

            // Verify status shows as Rejected with reason
            let status_result = env
                .hitch
                .run()
                .args(&["approvals", "status", request_id])
                .execute()?;
            status_result
                .assert_success()
                .assert_stdout_contains("Status: ✗ Rejected")
                .assert_stdout_contains("Rejected by: alice@example.com")
                .assert_stdout_contains("Reason: Tests are failing for this bug fix");

            // Verify no further approvals can be added
            let attempt_approve = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            attempt_approve
                .assert_failure()
                .assert_stderr_contains("Cannot approve request with status: Rejected");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_backward_compatibility() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create environment WITHOUT approval requirements (default behavior)
            env.hitch
                .run()
                .args(&["add", "development"])
                .execute()?
                .assert_success();

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/legacy"])?;
            env.fs.write_file("legacy.js", "// Legacy feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add legacy feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Promotion should work normally without requiring approval
            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/legacy", "development"])
                .execute()?;
            promote_result.assert_success().assert_stdout_contains(
                "Successfully promoted 'feature/legacy' to environment 'development'",
            );

            // Verify no approval requests were created
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            list_result
                .assert_success()
                .assert_stdout_contains("No approval requests found");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_cleanup_command_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with approval requirements
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create and complete several approval requests
            for i in 1..=3 {
                env.git
                    .run(&["checkout", "-b", &format!("feature/old-{}", i)])?;
                env.fs.write_file(&format!("old-{}.js", i), "// Old code")?;
                env.git.run(&["add", "."])?;
                env.git.run(&["commit", "-m", &format!("Old feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                // Create approval request
                env.hitch
                    .run()
                    .args(&["promote", &format!("feature/old-{}", i), "production"])
                    .execute()?
                    .assert_success();
            }

            // Get all request IDs
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();

            // Verify success
            env.hitch.run().args(&["approvals", "list"]).execute()?.assert_success();

            // Extract first request ID for rejection
            let first_request_id = output
                .lines()
                .find(|line| line.contains("feature/old-1"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find first request ID");

            // Reject one request
            env.git.config_user("Alice", "alice@example.com")?;
            env.hitch
                .run()
                .args(&[
                    "approvals",
                    "reject",
                    first_request_id,
                    "Not needed anymore, requirements changed",
                ])
                .execute()?
                .assert_success();

            // Verify we have approval requests (at least the rejected one should exist)
            let list_all = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let count = list_all
                .stdout()
                .lines()
                .filter(|line| line.contains("feature/old-"))
                .count();
            assert!(
                count >= 2,
                "Should have at least 2 approval requests, got {}",
                count
            );

            // Cleanup with older-than=90 (should NOT delete recent requests)
            let cleanup_result = env
                .hitch
                .run()
                .args(&["approvals", "cleanup", "--older-than", "90", "--force"])
                .execute()?;
            cleanup_result
                .assert_success()
                .assert_stdout_contains("No approval requests found older than 90 days");

            // Verify all requests still exist (they're too recent)
            let list_after = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let after_output = list_after.stdout();
            let remaining = after_output
                .lines()
                .filter(|line| line.contains("feature/old-"))
                .count();
            assert!(
                remaining >= 2,
                "All requests should still exist (too recent to cleanup)"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_cleanup_command_with_pending() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create approval requests
            for i in 1..=2 {
                env.git
                    .run(&["checkout", "-b", &format!("feature/pending-{}", i)])?;
                env.fs
                    .write_file(&format!("pending-{}.js", i), "// Pending code")?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Pending feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                env.hitch
                    .run()
                    .args(&["promote", &format!("feature/pending-{}", i), "production"])
                    .execute()?
                    .assert_success();
            }

            // Cleanup without --include-pending (should not delete recent pending requests)
            let cleanup_no_pending = env
                .hitch
                .run()
                .args(&["approvals", "cleanup", "--older-than", "90", "--force"])
                .execute()?;
            cleanup_no_pending
                .assert_success()
                .assert_stdout_contains("No approval requests found older than 90 days");

            // Cleanup WITH --include-pending (should still not delete recent requests)
            let cleanup_with_pending = env
                .hitch
                .run()
                .args(&[
                    "approvals",
                    "cleanup",
                    "--older-than",
                    "90",
                    "--include-pending",
                    "--force",
                ])
                .execute()?;
            cleanup_with_pending
                .assert_success()
                .assert_stdout_contains("No approval requests found older than 90 days");

            // Verify all requests still exist (too recent to cleanup)
            let list_after = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let after_output = list_after.stdout();
            let remaining = after_output
                .lines()
                .filter(|line| line.contains("feature/pending-"))
                .count();
            assert_eq!(remaining, 2, "Both requests should still exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_cleanup_command_dry_run() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create approval request
            env.git.run(&["checkout", "-b", "feature/dryrun-test"])?;
            env.fs.write_file("dryrun.js", "// Dry run test")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Dry run feature"])?;
            env.git.run(&["checkout", "main"])?;

            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/dryrun-test", "production"])
                .execute()?;
            promote_result.assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/dryrun-test"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // Reject it so it can be cleaned up
            env.git.config_user("Alice", "alice@example.com")?;
            env.hitch
                .run()
                .args(&[
                    "approvals",
                    "reject",
                    request_id,
                    "Testing dry run cleanup functionality",
                ])
                .execute()?
                .assert_success();

            // Dry run cleanup (request is too recent, so nothing to delete)
            let dry_run_result = env
                .hitch
                .run()
                .args(&[
                    "approvals",
                    "cleanup",
                    "--older-than",
                    "90",
                    "--dry-run",
                ])
                .execute()?;
            dry_run_result
                .assert_success()
                .assert_stdout_contains("No approval requests found older than 90 days");

            // Verify request still exists
            let list_after = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let after_count = list_after
                .stdout()
                .lines()
                .filter(|line| line.contains("feature/dryrun-test"))
                .count();
            assert_eq!(
                after_count, 1,
                "Request should still exist (too recent for cleanup)"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_automatic_application_on_threshold() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with approval requirements
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com", "charlie@example.com"],
                2,
            )?;

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/auto-apply"])?;
            env.fs
                .write_file("auto-apply.js", "console.log('Auto apply test');")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add auto-apply feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Create approval request
            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/auto-apply", "production"])
                .execute()?;
            promote_result
                .assert_success()
                .assert_stdout_contains("Approval request")
                .assert_stdout_contains("Waiting for 2 approval(s)");

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/auto-apply"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // First approval (should not trigger execution)
            env.git.config_user("Alice", "alice@example.com")?;
            let approve1_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id, "--comment", "LGTM, ready for prod"])
                .execute()?;
            approve1_result
                .assert_success()
                .assert_stdout_contains("Approval recorded")
                .assert_stdout_contains("1/2");

            // Second approval (should trigger automatic execution)
            env.git.config_user("Bob", "bob@example.com")?;
            let approve2_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id, "--comment", "Approved, looks good"])
                .execute()?;
            approve2_result
                .assert_success()
                .assert_stdout_contains("Approval threshold met")
                .assert_stdout_contains("executing operation")
                .assert_stdout_contains("Successfully promoted");

            // Verify branch is now promoted
            let status_after = env.hitch.run().args(&["status"]).execute()?;
            status_after
                .assert_success()
                .assert_stdout_contains("feature/auto-apply");

            // Verify request status is "Applied"
            let status_result = env
                .hitch
                .run()
                .args(&["approvals", "status", request_id])
                .execute()?;
            status_result
                .assert_success()
                .assert_stdout_contains("Status: 🚀 Applied");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_snapshot_staleness_detection() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment
            create_approval_environment(
                &env,
                "production",
                &["alice@example.com", "bob@example.com"],
                2,
            )?;

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/stale-test"])?;
            env.fs
                .write_file("stale.js", "console.log('Initial version');")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Initial version"])?;
            env.git.run(&["checkout", "main"])?;

            // Create approval request (captures snapshot)
            let promote_result = env
                .hitch
                .run()
                .args(&["promote", "feature/stale-test", "production"])
                .execute()?;
            promote_result.assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/stale-test"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // Modify the main branch (base branch) - making snapshot stale
            env.git.run(&["checkout", "main"])?;
            env.fs
                .write_file("base-change.js", "// Change to base branch")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Update base branch after approval request"])?;

            // Try to approve - should fail due to stale snapshot (base branch changed)
            env.git.config_user("Alice", "alice@example.com")?;
            let approve_result = env
                .hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?;
            approve_result
                .assert_failure()
                .assert_stderr_contains("has changed")
                .assert_stderr_contains("A new approval request is required");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_approval_count_display_accuracy() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Setup production environment with min_approvals=3
            create_approval_environment(
                &env,
                "production",
                &[
                    "alice@example.com",
                    "bob@example.com",
                    "charlie@example.com",
                    "david@example.com",
                ],
                3,
            )?;

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature/count-test"])?;
            env.fs.write_file("count.js", "// Count test")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Count test feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Create approval request
            env.hitch
                .run()
                .args(&["promote", "feature/count-test", "production"])
                .execute()?
                .assert_success();

            // Get request ID
            let list_result = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let output = list_result.stdout();
            let request_id = output
                .lines()
                .find(|line| line.contains("feature/count-test"))
                .and_then(|line| line.split_whitespace().next())
                .expect("Should find request ID");

            // Initial state: should show 0/3
            let list_initial = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let initial_output = list_initial.stdout();
            let initial_line = initial_output
                .lines()
                .find(|line| line.contains(request_id))
                .expect("Should find request in list");
            assert!(
                initial_line.contains("0/3"),
                "Should display 0/3, got: {}",
                initial_line
            );

            // Add first approval
            env.git.config_user("Alice", "alice@example.com")?;
            env.hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?
                .assert_success()
                .assert_stdout_contains("1/3");

            // Verify list shows 1/3 correctly
            let list_one = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let list_one_output = list_one.stdout();
            let approval_line = list_one_output
                .lines()
                .find(|line| line.contains(request_id))
                .expect("Should find request in list");
            assert!(
                approval_line.contains("1/3"),
                "Should display 1/3, got: {}",
                approval_line
            );

            // Add second approval
            env.git.config_user("Bob", "bob@example.com")?;
            env.hitch
                .run()
                .args(&["approvals", "approve", request_id])
                .execute()?
                .assert_success()
                .assert_stdout_contains("2/3");

            // Verify list shows 2/3 correctly
            let list_two = env.hitch.run().args(&["approvals", "list"]).execute()?;
            let list_two_output = list_two.stdout();
            let approval_line_two = list_two_output
                .lines()
                .find(|line| line.contains(request_id))
                .expect("Should find request in list");
            assert!(
                approval_line_two.contains("2/3"),
                "Should display 2/3, got: {}",
                approval_line_two
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
