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
}
