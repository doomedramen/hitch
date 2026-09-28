//! Integration tests for hitch set command

#[cfg(test)]
mod tests {
    use crate::test_framework::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_set_base_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with main as base
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Change base branch to develop
            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "develop"])
                .execute()?;
            // The receipt names the field it wrote, not just the environment.
            result
                .assert_success()
                .assert_stdout_contains("update base of 'dev'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "develop");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_requires_approval() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Enable approval requirement WITH an approver (required for valid config)
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update approval requirement, approvers of 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert!(prod_env.requires_approval);
            assert_eq!(prod_env.approvers.len(), 1);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_min_approvals() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and two approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Set minimum approvals
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--min-approvals", "2"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update approval threshold of 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.min_approvals, 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_add_approvers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and an initial approver
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "initial@example.com",
                ])
                .execute()?
                .assert_success();

            // Add more approvers
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update approvers of 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 3);
            assert!(prod_env
                .approvers
                .contains(&"initial@example.com".to_string()));
            assert!(prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));
            assert!(prod_env.approvers.contains(&"bob@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_remove_approver() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Remove an approver (leave at least one to keep config valid)
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--remove-approver", "bob@example.com"])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update approvers of 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 1);
            assert!(prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));
            assert!(!prod_env.approvers.contains(&"bob@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_approvers() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with approval enabled and initial approvers
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "1",
                    "--set-approvers",
                    "alice@example.com",
                ])
                .execute()?
                .assert_success();

            // Set complete list of approvers (replaces existing)
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--set-approvers",
                    "charlie@example.com",
                    "--set-approvers",
                    "dave@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update approvers of 'production'");

            // Verify environment was updated
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert_eq!(prod_env.approvers.len(), 2);
            assert!(prod_env
                .approvers
                .contains(&"charlie@example.com".to_string()));
            assert!(prod_env.approvers.contains(&"dave@example.com".to_string()));
            assert!(!prod_env
                .approvers
                .contains(&"alice@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_no_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to set without any changes
            let result = env.hitch.run().args(&["set", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("No changes specified");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Try to update nonexistent environment
            let result = env
                .hitch
                .run()
                .args(&["set", "nonexistent", "--base", "main"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_invalid_base_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to set nonexistent base branch
            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "nonexistent"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_invalid_email_format() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Try to add approver with invalid email
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--add-approver", "invalid-email"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Invalid email format");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_min_approvals_zero() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Try to set min_approvals to 0
            let result = env
                .hitch
                .run()
                .args(&["set", "production", "--min-approvals", "0"])
                .execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("Minimum approvals must be at least 1");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_multiple_changes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add an environment with main as base
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create a develop branch
            env.git.run(&["checkout", "-b", "develop"])?;
            env.git.run(&["checkout", "main"])?;

            // Apply multiple changes at once
            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--base",
                    "develop",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "1",
                    "--add-approver",
                    "alice@example.com",
                ])
                .execute()?;
            result
                .assert_success()
                .assert_stdout_contains("update base, approval requirement, approvers of 'dev'");

            // Verify all changes were applied
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert_eq!(dev_env.base, "develop");
            assert!(dev_env.requires_approval);
            assert_eq!(dev_env.min_approvals, 1);
            assert_eq!(dev_env.approvers.len(), 1);
            assert!(dev_env.approvers.contains(&"alice@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_set_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Add production environment
            env.hitch
                .run()
                .args(&["add", "production"])
                .execute()?
                .assert_success();

            // Configure approval workflow (all in one command)
            env.hitch
                .run()
                .args(&[
                    "set",
                    "production",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "2",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // Verify configuration
            let config = env.read_hitch_config()?;
            let prod_env = config.environments.get("production").unwrap();
            assert!(prod_env.requires_approval);
            assert_eq!(prod_env.min_approvals, 2);
            assert_eq!(prod_env.approvers.len(), 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // ── plan → apply → receipt ──────────────────────────────────────────
    //
    // `hitch set` is the widest metadata edit: seven flags resolving to four
    // fields, and — uniquely among P8's commands — an edit whose *resolution* is
    // not its inputs. `--add-approver` for someone already on the list, and
    // `--base` for a branch already promoted, both name a flag and move
    // nothing. These four tests are about that gap: what a plan says when the
    // flags are not the changes, and what a commit is worth.

    #[test]
    fn a_set_shows_the_resolved_edit_and_its_collateral_not_the_flags() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            // Promote a real feature branch before re-basing onto it, so the
            // base change has collateral the user did not ask for and no flag
            // names.
            env.git.run(&["checkout", "-b", "feature"])?;
            env.fs.write_file("feature.txt", "x")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "feature"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feature", "dev"])
                .execute()?
                .assert_success();
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            // The collateral is the branch that just stopped being promoted
            // because it became the base. It is in the *plan*, not only in the
            // receipt: a reader who does not see it will think they promoted a
            // branch into a base and lost it. Checked on a dry run so the two
            // halves are separable — a plan that only ever appears alongside its
            // own receipt cannot be shown to be write-free.
            let preview = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "feature", "--dry-run"])
                .execute()?;
            let plan = preview.stdout();
            preview
                .assert_success()
                .assert_stdout_contains("absorbing promoted branch 'feature'");
            assert!(
                !plan.contains("Applied"),
                "a preview has no receipt half — there was no apply to record: {plan}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                before,
                "a plan is a decision, not a recipe: previewing it wrote nothing"
            );

            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--base", "feature"])
                .execute()?;
            let out = result.stdout();
            result
                .assert_success()
                .assert_stdout_contains("absorbing promoted branch 'feature'");
            // Both documents name the absorbed branch and the plan says it once.
            // Three mentions would be the plan and the receipt plus a third
            // voice, and one would be a receipt that forgot the fact the plan
            // was careful to state.
            assert_eq!(
                out.matches("absorbing promoted branch 'feature'").count(),
                2,
                "once in the plan and once in the receipt: {out}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn a_set_that_resolves_to_nothing_spends_no_metadata_commit() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["set", "dev", "--min-approvals", "2"])
                .execute()?
                .assert_success();
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            // Names a flag. Changes nothing.
            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--min-approvals", "2"])
                .execute()?;
            let out = result.stdout();
            result
                .assert_success()
                .assert_stdout_contains("Already up to date");
            assert!(
                !out.contains("Will change\n  hitch-metadata   update"),
                "and proposes no effect it will not make: {out}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                before,
                "a commit recording that hitch did nothing would make the *next* \
                 plan stale for a reason no reader could see"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn a_dry_run_previews_a_set_and_writes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            let result = env
                .hitch
                .run()
                .args(&["set", "dev", "--min-approvals", "3", "--dry-run"])
                .execute()?;
            let out = result.stdout();
            result
                .assert_success()
                .assert_stdout_contains("update approval threshold of 'dev'");
            assert!(
                !out.contains("Applied"),
                "a preview has no receipt half — there was no apply to record: {out}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                before
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn an_invalid_approval_combination_is_refused_in_a_plan() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "a@x.com",
                ])
                .execute()?
                .assert_success();
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            // Two approvers on the list, a threshold of one, so this is valid —
            // except the threshold is being raised past the approver count.
            env.hitch
                .run()
                .args(&["set", "dev", "--add-approver", "b@x.com"])
                .execute()?
                .assert_success();
            let armed = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            let result = env
                .hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--remove-approver",
                    "a@x.com",
                    "--min-approvals",
                    "2",
                ])
                .execute()?;
            let out = result.stdout();
            let err = result.stderr();
            result.assert_failure();
            assert!(
                out.contains("Why this cannot apply"),
                "the refusal is in a plan, so the reader sees the edit it refused: {out}"
            );
            assert!(
                !out.contains("Will change"),
                "and that plan proposes nothing it will not do: {out}"
            );
            assert!(
                err.contains("cannot be greater than number of approvers"),
                "and says which of the two settings disagrees with the other: {err}"
            );
            assert!(
                err.contains("hitch set dev --add-approver"),
                "and names the move that unblocks it, rather than the command that \
                 just failed: {err}"
            );
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                armed,
                "a refused set must not spend a metadata commit"
            );
            let _ = before;
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
