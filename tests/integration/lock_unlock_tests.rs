//! Integration tests for hitch lock/unlock commands

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn test_hitch_lock_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Lock the environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            // The command's own "Successfully locked 'dev'!" line is gone; the
            // receipt says it, and names the holder — which that line never did.
            result
                .assert_success()
                .assert_stdout_contains("lock 'dev' held by");

            // Verify environment is locked
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            assert!(dev_env.locked_by.is_some());
            assert!(dev_env.locked_at.is_some());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_basic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Verify it's locked
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_success()
                .assert_stdout_contains("release the lock on 'dev'");

            // Verify environment is unlocked
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());
            assert!(dev_env.locked_by.is_none());
            assert!(dev_env.locked_at.is_none());

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to lock without initializing hitch
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_without_init() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::None, |env| {
            // Try to unlock without initializing hitch
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("hitch-metadata branch does not exist locally");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to lock nonexistent environment
            let result = env.hitch.run().args(&["lock", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_nonexistent_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch but don't add environment
            // Hitch is already initialized by framework

            // Try to unlock nonexistent environment
            let result = env.hitch.run().args(&["unlock", "nonexistent"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("does not exist");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_already_locked() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Try to lock already locked environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stdout_contains("already locked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_already_unlocked() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment (but don't lock it)
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Try to unlock already unlocked environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stdout_contains("not currently locked");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add multiple environments
            // Hitch is already initialized by framework

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
            }

            // Lock all environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["lock", env_name]).execute()?;
                result
                    .assert_success()
                    .assert_stdout_contains(&format!("lock '{}' held by", env_name));
            }

            // Verify all environments are locked
            let config = env.read_hitch_config()?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(env.is_locked());
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_multiple_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environments, and lock them
            // Hitch is already initialized by framework

            for env_name in ["dev", "qa", "staging"] {
                env.hitch
                    .run()
                    .args(&["add", env_name])
                    .execute()?
                    .assert_success();
                env.hitch
                    .run()
                    .args(&["lock", env_name])
                    .execute()?
                    .assert_success();
            }

            // Verify all are locked
            let config = env.read_hitch_config()?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(env.is_locked());
            }

            // Unlock all environments
            for env_name in ["dev", "qa", "staging"] {
                let result = env.hitch.run().args(&["unlock", env_name]).execute()?;
                result
                    .assert_success()
                    .assert_stdout_contains(&format!("release the lock on '{}'", env_name));
            }

            // Verify all environments are unlocked
            let config = env.read_hitch_config()?;
            for env_name in ["dev", "qa", "staging"] {
                let env = config.environments.get(env_name).unwrap();
                assert!(!env.is_locked());
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_with_promoted_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and promote branches
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Create and promote feature branches
            for i in 1..=2 {
                let branch_name = format!("feature-{}", i);
                env.git.run(&["checkout", "-b", &branch_name])?;
                env.fs
                    .write_file(&format!("{}.txt", i), &format!("content {}", i))?;
                env.git.run(&["add", "."])?;
                env.git
                    .run(&["commit", "-m", &format!("Add feature {}", i)])?;
                env.git.run(&["checkout", "main"])?;

                let result = env
                    .hitch
                    .run()
                    .args(&["promote", &branch_name, "dev"])
                    .execute()?;
                result.assert_success();
            }

            // Lock environment with promoted branches
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            // The command's own "Successfully locked 'dev'!" line is gone; the
            // receipt says it, and names the holder — which that line never did.
            result
                .assert_success()
                .assert_stdout_contains("lock 'dev' held by");

            // Verify environment is locked and branches are preserved
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            assert_eq!(dev_env.branches.len(), 2);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_workflow() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch and add environment
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Initial state: unlocked
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());

            // Lock the environment
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result.assert_success();

            // Verify locked state
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            let lock_time = dev_env.locked_at.unwrap();

            // Wait a moment to ensure different timestamp
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result.assert_success();

            // Verify unlocked state
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(!dev_env.is_locked());
            assert!(dev_env.locked_by.is_none());
            assert!(dev_env.locked_at.is_none());

            // Lock again to ensure it can be re-locked
            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            result.assert_success();

            // Verify new lock time
            let config = env.read_hitch_config()?;
            let dev_env = config.environments.get("dev").unwrap();
            assert!(dev_env.is_locked());
            let new_lock_time = dev_env.locked_at.unwrap();
            assert!(new_lock_time > lock_time);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_lock_prevents_operations() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Try to promote branch to locked environment (should fail)
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_failure(); // Promotion should auto-unlock and succeed

            // Try to rebuild locked environment (should fail)
            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stderr_contains("is locked")
                .assert_stderr_contains("--force");

            // Try to remove locked environment (should fail). The refusal is
            // worded "is currently locked" now — it names the plan it refused
            // from, rather than being a bare `Err` with nothing above it, so the
            // reason reads as a statement about the plan.
            let result = env.hitch.run().args(&["remove", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stdout_contains("is currently locked")
                .assert_stderr_contains("--force");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_unlock_allows_operations() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Initialize hitch, add environment, and lock it
            // Hitch is already initialized by framework
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Create feature branch
            env.git.run(&["checkout", "-b", "feature-1"])?;
            env.fs.write_file("feature.txt", "new feature")?;
            env.git.run(&["add", "."])?;
            env.git.run(&["commit", "-m", "Add feature"])?;
            env.git.run(&["checkout", "main"])?;

            // Unlock the environment
            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result.assert_success();

            // Now operations should succeed
            let result = env
                .hitch
                .run()
                .args(&["promote", "feature-1", "dev"])
                .execute()?;
            result.assert_success();

            let result = env.hitch.run().args(&["rebuild", "dev"]).execute()?;
            result.assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // ── plan → apply → receipt ──────────────────────────────────────────
    //
    // A lock is the smallest operation in the CLI: one ref, one commit, no
    // composition, no push. It is therefore the sharpest test of whether the
    // three documents render *and mean* something at this size. The two
    // refusals are the other half — they used to print a bare error with no plan
    // above it, and a refusal whose plan the reader cannot see is a refusal
    // they have to reconstruct.

    #[test]
    fn a_lock_shows_a_plan_and_a_receipt_both_naming_the_locker() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            let out = result.stdout();
            result.assert_success();

            assert!(out.contains("Will change"), "the plan: {out}");
            assert!(out.contains("Applied"), "and the receipt: {out}");
            // Both halves name the ref, and nothing else may: a third mention
            // would be the command talking over its own receipt, and one fewer
            // would mean a document without its effect.
            assert_eq!(
                out.matches("hitch-metadata").count(),
                2,
                "once in the plan's 'Will change' and once in the receipt's \
                 effects, and nowhere else: {out}"
            );
            assert!(
                out.contains('@'),
                "a lock that does not name its holder is the one thing it \
                 exists to prevent: {out}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn a_lock_emits_a_json_document_with_its_plan_and_its_receipt() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["lock", "dev", "--json"]).execute()?;
            let stdout = result.stdout();
            result.assert_success();

            // Under `--json` the document is the only thing on stdout; a log line
            // ahead of it would be a channel split that regressed, so report the
            // whole body rather than digging the document out of it.
            let document: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
                panic!("`--json` stdout is not a JSON document ({e}):\n{stdout}")
            });
            assert_eq!(document["schema_version"], 1, "{document}");
            assert_eq!(document["plan"]["kind"], "Lock", "{document}");
            assert!(document["receipt"].is_object(), "{document}");
            assert_eq!(document["receipt"]["operation"], "Lock", "{document}");
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn a_lock_that_is_refused_still_shows_the_plan_it_refused() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["lock", "dev"]).execute()?;
            let out = result.stdout();
            let err = result.stderr();
            result.assert_failure();
            assert!(
                out.contains("already locked") && !err.contains("already locked"),
                "and it says why once, in the plan, not again in the error: {out} / {err}"
            );
            // The plan, so the reader can see what was refused instead of
            // reconstructing it. Asserted as the *refusal section* rather than
            // as the ref name, because a blocked plan has no effect rows: it
            // proposes nothing that will not happen, so a `Will change` line
            // reading "lock 'dev' held by …" above a refusal that it cannot lock
            // would be a plan claiming the very effect it is refusing. See
            // `plan_declaration_change`'s "Effects. Empty for a blocked plan".
            assert!(
                out.contains("Why this cannot apply"),
                "the refusal is in a plan, not in a bare error: {out}"
            );
            assert!(
                !out.contains("Will change"),
                "and the plan proposes nothing it will not do: {out}"
            );
            assert!(
                !out.contains("hitch-metadata"),
                "which means it names no ref to write, and does not pretend to: {out}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn an_unlock_by_a_stranger_writes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // The declared holder becomes someone else, so the unlock below is
            // by a stranger. The repository identity is unchanged, which is the
            // point: the *declared* holder decides, not who is at the keyboard.
            let mut config: serde_json::Value = serde_json::from_str(
                &env.git
                    .run(&["show", "hitch-metadata:hitch.json"])?
                    .stdout(),
            )?;
            config["environments"]["dev"]["locked_by"] = serde_json::json!("someone@else.com");
            env.git.run(&["checkout", "hitch-metadata"])?;
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git
                .run(&["commit", "-m", "test: hand the lock to someone else"])?;
            env.git.run(&["checkout", "main"])?;
            let before = env
                .git
                .run(&["rev-parse", "hitch-metadata"])?
                .stdout()
                .trim()
                .to_string();

            let result = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            result
                .assert_failure()
                .assert_stdout_contains("someone@else.com");
            assert_eq!(
                env.git
                    .run(&["rev-parse", "hitch-metadata"])?
                    .stdout()
                    .trim(),
                before,
                "a refused unlock must not spend a metadata commit"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
    /// A refusal names its cause once — in the plan, under "Why this cannot
    /// apply" — and the error that follows carries only what the plan does not:
    /// that nothing changed, and what to do next. And a refusal that is only
    /// "already so" has no "next", so it does not print a "To proceed:" with a
    /// non-action under it.
    #[test]
    fn a_refused_lock_states_its_cause_once_and_a_no_op_offers_no_way_to_proceed(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            let locked = env.hitch.run().args(&["lock", "dev"]).execute()?;
            let both = format!("{}{}", locked.stdout(), locked.stderr());
            let locked = locked.assert_failure();
            assert_eq!(
                both.matches("is already locked").count(),
                1,
                "the cause is said once: {both}"
            );
            locked
                .assert_stderr_contains("To proceed:")
                .assert_stderr_contains("hitch unlock dev");

            env.hitch
                .run()
                .args(&["unlock", "dev"])
                .execute()?
                .assert_success();
            let unlocked = env.hitch.run().args(&["unlock", "dev"]).execute()?;
            let both = format!("{}{}", unlocked.stdout(), unlocked.stderr());
            let unlocked = unlocked.assert_failure();
            assert_eq!(
                both.matches("is not currently locked").count(),
                1,
                "the cause is said once: {both}"
            );
            assert!(
                !unlocked.stderr().contains("To proceed"),
                "there is nothing to proceed with: {}",
                unlocked.stderr()
            );
            assert!(
                !both.contains("nothing to undo"),
                "a non-action does not go under a proceed heading: {both}"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
