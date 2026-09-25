//! The plan → apply contract, for one operation end to end.
//!
//! These drive `hitch::operations::rebuild` directly rather than asserting on
//! CLI prose, for the same reason `state_model_tests.rs` does: the thing under
//! test is a *structured* claim — that what a plan predicted is what the apply
//! did and what the repository now holds — and asserting on rendered text would
//! only pin whichever renderer happens to exist. The CLI's own wording is
//! covered in `rebuild_tests.rs`; the two exit-code cases here deliberately
//! re-check it from the library side, because the exit code is a CI contract
//! and a refactor that kept the library honest while breaking the contract
//! would be easy to write.
//!
//! Every test builds a real repository through the integration harness, so a
//! real `hitch-metadata` branch, a real origin, and a real build record are all
//! in play.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::commands::global_context::GlobalContext;
    use hitch::operations::model::{
        AppliedEffect, OperationOutcome, OperationPlan, PlanApplyError, PlannedEffect,
    };
    use hitch::operations::rebuild::{
        apply_rebuild_plan, discard_plan, plan_rebuild, validate_plan, PlanPurpose,
        RebuildPlanDetail, RebuildPlanOptions,
    };
    use hitch::utils::git_operations::RefEdit;
    use hitch::utils::logging::Logger;
    use std::sync::Arc;

    type Plan = OperationPlan<RebuildPlanDetail>;

    /// A context pointed at the test repo, matching a real `hitch rebuild`:
    /// pushing enabled, and assuming-yes so nothing ever blocks on a prompt.
    ///
    /// `GlobalContext::new_at_path` reports a boxed non-Send error, so the
    /// conversion is spelled out rather than left to `?`.
    fn context_for(env: &TestEnvironment, push: bool) -> anyhow::Result<GlobalContext> {
        let logger = Arc::new(Logger::new());
        GlobalContext::new_at_path(
            env.temp_dir.to_str().expect("utf-8 temp dir"),
            false,
            !push,
            true,
            logger,
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))
    }

    fn plan(env: &TestEnvironment, purpose: PlanPurpose, push: bool) -> anyhow::Result<Plan> {
        plan_rebuild(
            &context_for(env, push)?,
            "dev",
            RebuildPlanOptions::default(),
            purpose,
            &mut |_| {},
        )
    }

    fn apply(env: &TestEnvironment, plan: &Plan, push: bool) -> anyhow::Result<Receipt> {
        apply_rebuild_plan(&context_for(env, push)?, plan, &mut |_| {})
    }

    type Receipt = hitch::operations::model::ExecutionReceipt;

    /// A path beside the test repo, never inside it — a bare repo inside the
    /// repo shows up as untracked content in its own `git status`.
    fn sibling_path(env: &TestEnvironment, name: &str) -> std::path::PathBuf {
        let repo_name = env
            .temp_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());
        env.temp_dir
            .parent()
            .expect("test repo has no parent directory")
            .join(format!("{}-{}", repo_name, name))
    }

    /// A bare `origin`, with pushing *denied* when `deny_push` is set.
    ///
    /// The denial is a `pre-receive` hook in the bare repo, which is how a real
    /// protected branch refuses a push: `fetch` still works, so a rebuild can
    /// synchronise and the failure is unambiguously a *push* failure rather
    /// than "no remote" or "user declined". Anything softer — a bad URL, a
    /// missing directory — would fail earlier, at a step that is not the one
    /// under test.
    fn init_bare_origin(
        env: &TestEnvironment,
        name: &str,
        deny_push: bool,
    ) -> anyhow::Result<std::path::PathBuf> {
        let bare_path = sibling_path(env, name);
        std::fs::create_dir_all(&bare_path)?;
        git_plain(
            std::path::Path::new("/"),
            &["init", "--bare", bare_path.to_str().expect("utf-8 path")],
        )?;
        if deny_push {
            let hook = bare_path.join("hooks").join("pre-receive");
            std::fs::write(&hook, "#!/bin/sh\nexit 1\n")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        env.git
            .run(&["remote", "add", "origin", &bare_path.to_string_lossy()])?;
        Ok(bare_path)
    }

    /// Plain git with the harness's own null-stdin handling, for the handful
    /// of reads the `GitCommandRunner` has no method for.
    fn git_plain(dir: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
        #[allow(clippy::disallowed_methods)]
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn rev(env: &TestEnvironment, reference: &str) -> anyhow::Result<Option<String>> {
        match git_plain(
            &env.temp_dir,
            &["rev-parse", "--verify", "--quiet", reference],
        ) {
            Ok(sha) => Ok(Some(sha.trim().to_string())),
            Err(_) => Ok(None),
        }
    }

    /// Every ref in the repository, so "nothing moved" can be asserted as a
    /// whole rather than ref by ref.
    fn all_refs(env: &TestEnvironment) -> anyhow::Result<Vec<String>> {
        let mut refs: Vec<String> = git_plain(&env.temp_dir, &["show-ref"])?
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(sha, name)| format!("{name} {sha}"))
            .collect();
        refs.sort();
        Ok(refs)
    }

    fn make_feature(env: &TestEnvironment, name: &str) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", name])?;
        // `-f` because hitch-metadata ships a broad .gitignore.
        env.fs.write_file(&format!("{name}.txt"), "v1")?;
        env.git.run(&["add", "-f", &format!("{name}.txt")])?;
        env.git
            .run(&["commit", "-m", &format!("{name}: initial")])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    fn add_commit(
        env: &TestEnvironment,
        branch: &str,
        file: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", branch])?;
        env.fs.write_file(file, body)?;
        env.git.run(&["add", "-f", file])?;
        env.git.run(&["commit", "-m", "later work"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// Declare branches into `hitch.json`, bypassing `promote`'s own conflict
    /// gate — needed wherever a *feature* is supposed to be in conflict, since
    /// `promote` would refuse it.
    ///
    /// Creates the environment with `hitch add` first if it is missing, because
    /// assigning into `config["environments"][dev]["branches"]` on a config with
    /// no `dev` key silently manufactures a `dev` with no `base`, which then
    /// fails to deserialize — a broken fixture rather than a test of anything.
    fn declare_branches(
        env: &TestEnvironment,
        environment: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "hitch-metadata"])?;
        let mut config: serde_json::Value = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        if config["environments"].get(environment).is_none() {
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["add", environment])
                .execute()?
                .assert_success();
            env.git.run(&["checkout", "hitch-metadata"])?;
            config = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        }
        config["environments"][environment]["branches"] = serde_json::to_value(branches)?;
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: declare branches directly"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// The tree OID of a commit, which is the timestamp-independent way to ask
    /// "did these two compositions produce the same content?".
    ///
    /// `commit_tree` stamps the ambient wall clock, so two compositions of
    /// identical inputs a second apart get different commit OIDs. Comparing
    /// commits here would be comparing clock reads, not content — and would
    /// flake for a reason unrelated to what is under test.
    fn tree_of(env: &TestEnvironment, commit: &str) -> anyhow::Result<String> {
        let sha = git_plain(&env.temp_dir, &["rev-parse", &format!("{commit}^{{tree}}")])?;
        Ok(sha.trim().to_string())
    }

    /// Two features touching the same line, so the composition holds one.
    fn conflicting_pair(env: &TestEnvironment) -> anyhow::Result<()> {
        for name in ["feat-a", "feat-b"] {
            env.git.run(&["checkout", "-b", name, "main"])?;
            env.fs.write_file("shared.txt", name)?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", &format!("{name}: initial")])?;
            env.git.run(&["checkout", "main"])?;
        }
        Ok(())
    }

    fn planned_for<'a>(effects: &'a [PlannedEffect], refname: &str) -> &'a PlannedEffect {
        effects
            .iter()
            .find(|e| e.refname() == refname)
            .unwrap_or_else(|| panic!("no planned effect for {refname}"))
    }

    fn applied_for<'a>(effects: &'a [AppliedEffect], refname: &str) -> &'a AppliedEffect {
        effects
            .iter()
            .find(|e| e.refname() == refname)
            .unwrap_or_else(|| panic!("no applied effect for {refname}"))
    }

    /// A refused apply that is specifically a staleness refusal, as opposed to
    /// some other error. The conversion to `anyhow` is a boundary, not an
    /// erasure: the typed error is still there to downcast, which is what
    /// makes "refused *because it went stale*" a distinguishable claim rather
    /// than "something failed".
    fn as_stale(error: &anyhow::Error) -> bool {
        error
            .downcast_ref::<PlanApplyError>()
            .is_some_and(|e| matches!(e, PlanApplyError::StalePlan { .. }))
    }

    fn refnames(effects: &[PlannedEffect]) -> Vec<&str> {
        effects.iter().map(|e| e.refname()).collect()
    }

    // ── the plan describes what it will do ───────────────────────────

    #[test]
    fn a_plan_lists_every_promoted_branch_in_declaration_order() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &["feat-b", "feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, false)?;
            let composition = &plan.compositions[0];
            let names: Vec<&str> = composition
                .branches
                .iter()
                .map(|b| b.branch.as_str())
                .collect();
            assert_eq!(
                names,
                vec!["feat-b", "feat-a"],
                "declaration order is composition order and is load-bearing"
            );
            discard_plan(&context_for(env, false)?, &plan);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_plan_predicts_the_branch_move_and_both_metadata_writes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, false)?;
            let names = refnames(&plan.effects);
            assert!(names.contains(&"refs/heads/dev"), "{names:?}");
            assert!(names.contains(&"refs/hitch/state/dev"), "{names:?}");
            assert!(names.contains(&"refs/heads/hitch-metadata"), "{names:?}");
            // The anchor is a real ref hitch writes; hiding it would be hiding
            // a write from the reader.
            assert!(
                names.iter().any(|r| r.starts_with("refs/hitch/build/dev/")),
                "{names:?}"
            );

            // `dev` does not exist yet, so this is a first build and the effect
            // must say the ref is being created rather than updated.
            match planned_for(&plan.effects, "refs/heads/dev") {
                PlannedEffect::LocalRefUpdate { old, new, .. } => {
                    assert_eq!(old, &None, "a first build creates the ref");
                    assert_eq!(new, &plan.compositions[0].result_sha);
                }
                other => panic!("expected a LocalRefUpdate, got {other:?}"),
            }
            discard_plan(&context_for(env, false)?, &plan);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_preview_anchors_nothing_and_moves_no_ref() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            let before = all_refs(env)?;

            let preview = plan(env, PlanPurpose::Preview, false)?;
            assert_eq!(
                preview.detail.anchor_ref, None,
                "a preview must not leave a ref behind"
            );
            assert_eq!(all_refs(env)?, before, "a preview moved a ref");

            // And the counterpart: a `Confirm` plan really does anchor, so the
            // assertion above is not passing because the anchor is never
            // created in the first place.
            let confirm = plan(env, PlanPurpose::Confirm, false)?;
            let anchor = confirm
                .detail
                .anchor_ref
                .clone()
                .expect("a confirm plan anchors the composed commit");
            assert!(
                rev(env, &anchor)?.is_some(),
                "the anchor ref must exist while the plan waits"
            );
            assert_ne!(all_refs(env)?, before, "a confirm plan did not write a ref");
            discard_plan(&context_for(env, false)?, &confirm);
            assert_eq!(rev(env, &anchor)?, None, "discard must drop the anchor");
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn the_same_inputs_produce_the_same_fingerprint() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let first = plan(env, PlanPurpose::Preview, false)?;
            let second = plan(env, PlanPurpose::Preview, false)?;
            assert_eq!(
                first.fingerprint, second.fingerprint,
                "identical inputs must fingerprint identically"
            );
            assert_eq!(first.id, second.id);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── staleness: the refusal is the point ──────────────────────────

    #[test]
    fn a_plan_is_refused_once_the_declaration_moves() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, false)?;
            let anchor = plan.detail.anchor_ref.clone();

            // Promote a second feature *after* the plan was built. This moves
            // `hitch-metadata`, which is where the plan's declaration lives.
            env.hitch
                .run()
                .args(&["promote", "feat-b", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let refusal = apply(env, &plan, false).expect_err("a stale plan must not apply");
            assert!(as_stale(&refusal), "not a StalePlan: {refusal:?}");
            let message = refusal.to_string();
            assert!(
                message.contains("hitch-metadata"),
                "the refusal must name what changed:\n{message}"
            );
            assert!(
                message.contains("hitch rebuild dev"),
                "the refusal must name the next command:\n{message}"
            );
            // Nothing was published...
            assert_eq!(rev(env, "refs/heads/dev")?, None);
            // ...and the anchor was released, not leaked.
            for refname in anchor.into_iter().collect::<Vec<_>>() {
                assert_eq!(
                    rev(env, &refname)?,
                    None,
                    "a refused plan must not leave its anchor behind"
                );
            }
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_plan_is_refused_once_a_promoted_branch_moves_and_names_both_shas() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Preview, false)?;
            let before = rev(env, "refs/heads/feat-a")?.expect("feature exists");
            add_commit(env, "feat-a", "feat-a.txt", "v2")?;
            let after = rev(env, "refs/heads/feat-a")?.expect("feature exists");

            let refusal = validate_plan(&context_for(env, false)?, &plan)
                .expect_err("a stale plan must not validate");
            let message = refusal.to_string();
            assert!(
                message.contains("refs/heads/feat-a"),
                "the refusal must name the ref that moved:\n{message}"
            );
            assert!(
                message.contains(&before[..7]),
                "missing the old sha:\n{message}"
            );
            assert!(
                message.contains(&after[..7]),
                "missing the new sha:\n{message}"
            );

            // And the whole apply refuses, not just the validator.
            let refusal = apply(env, &plan, false).expect_err("a stale plan must not apply");
            assert!(as_stale(&refusal), "not a StalePlan: {refusal:?}");
            assert_eq!(rev(env, "refs/heads/dev")?, None);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_plan_is_refused_once_the_environment_branch_moves() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let plan = plan(env, PlanPurpose::Preview, false)?;
            // Another publisher moves the environment branch. The repo lock
            // would stop a concurrent `hitch`, but not a plain `git` — which is
            // exactly the case the fingerprint exists for.
            add_commit(env, "dev", "dev.txt", "moved")?;

            let refusal = apply(env, &plan, false).expect_err("a stale plan must not apply");
            assert!(as_stale(&refusal), "not a StalePlan: {refusal:?}");
            assert!(refusal.to_string().contains("refs/heads/dev"));
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn an_unchanged_plan_still_validates() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            let plan = plan(env, PlanPurpose::Preview, false)?;
            validate_plan(&context_for(env, false)?, &plan).expect("a fresh plan must validate");
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── the §30.3 invariant: plan predicts, apply does, repository holds ──

    #[test]
    fn an_unchanged_plan_applies_exactly_the_predicted_effects() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, false)?;
            let predicted = plan.compositions[0].result_sha.clone();
            let receipt = apply(env, &plan, false)?;

            // 1. the plan's prediction, 2. the receipt's report, 3. what the
            // repository actually holds.
            let applied = applied_for(&receipt.effects, "refs/heads/dev");
            let AppliedEffect::LocalRefUpdate { new, .. } = applied else {
                panic!("expected a LocalRefUpdate, got {applied:?}")
            };
            assert_eq!(
                *new, predicted,
                "the receipt must report the commit the plan named"
            );
            assert_eq!(
                rev(env, "refs/heads/dev")?.as_deref(),
                Some(predicted.as_str()),
                "the repository must hold what both the plan and the receipt said"
            );

            // Every planned effect is either applied or explained. A plan
            // claiming a change the receipt cannot account for is the failure
            // this whole architecture exists to make impossible.
            for planned in &plan.effects {
                let accounted = match planned {
                    // The anchor is temporary: the plan names it, and its being
                    // gone afterwards *is* the applied effect.
                    PlannedEffect::MetadataChange { refname, .. }
                        if refname.starts_with("refs/hitch/build/") =>
                    {
                        rev(env, refname)?.is_none()
                    }
                    other => receipt
                        .effects
                        .iter()
                        .any(|applied| applied.refname() == other.refname()),
                };
                assert!(
                    accounted,
                    "planned effect {} was neither applied nor explained",
                    planned.refname()
                );
            }
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_publish_failure_leaves_no_anchor_behind() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let mut plan = plan(env, PlanPurpose::Confirm, false)?;
            let anchor = plan
                .detail
                .anchor_ref
                .clone()
                .expect("a confirm plan anchors the composed commit");
            assert!(rev(env, &anchor)?.is_some());

            // Make the publish transaction itself fail, deterministically and
            // without touching the planner: a refname containing `..` is
            // rejected by `update-ref` before any part of the batch runs, so
            // the *whole* transaction aborts and the branch does not move.
            //
            // The first attempt at this test made `.git/refs/hitch/state/dev` a
            // directory instead, on the theory that git could not write a file
            // there. It can — git removes the empty directory and writes the
            // loose ref anyway, so the transaction succeeded and the test was
            // asserting nothing. `..` is rejected outright, which makes the
            // failure certain rather than merely likely.
            let RefEdit::Update {
                new_oid,
                expected_old,
                ..
            } = plan.detail.state_edit.clone()
            else {
                unreachable!("the build record is written with an unconditional update")
            };
            plan.detail.state_edit = RefEdit::Update {
                refname: "refs/hitch/state/../escape".to_string(),
                new_oid,
                expected_old,
            };

            apply(env, &plan, false).expect_err("the publish must fail");
            assert_eq!(
                rev(env, &anchor)?,
                None,
                "a failed publish must not leak the anchor"
            );
            // And the branch really did not move.
            assert_eq!(rev(env, "refs/heads/dev")?, None);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn the_build_record_and_the_rebuilt_stamp_both_land() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let receipt = apply(env, &plan(env, PlanPurpose::Preview, false)?, false)?;
            assert!(
                rev(env, "refs/hitch/state/dev")?.is_some(),
                "the build record must exist after an apply"
            );
            assert_eq!(receipt.outcome, OperationOutcome::Applied);

            // A second rebuild's `rebuilt_at` stamp is a *new* commit on
            // hitch-metadata, written by `publish_environment_build` for every
            // caller rather than by this one.
            let metadata_before = rev(env, "refs/heads/hitch-metadata")?;
            let plan = plan(env, PlanPurpose::Preview, false)?;
            apply(env, &plan, false)?;
            assert_ne!(
                rev(env, "refs/heads/hitch-metadata")?,
                metadata_before,
                "the rebuilt_at stamp must move hitch-metadata"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_plan_that_will_not_push_predicts_no_remote_effect() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            init_bare_origin(env, "origin", false)?;
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Preview, false)?;
            // A plan that predicts a push the apply will not attempt is
            // exactly the false "fully synced" a receipt must not produce.
            assert!(
                !plan
                    .effects
                    .iter()
                    .any(|e| matches!(e, PlannedEffect::RemoteRefUpdate { .. })),
                "a --no-push plan must not predict a remote effect"
            );
            assert!(!plan.confirmation.required);

            let receipt = apply(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert!(
                !receipt.has_owed_effects(),
                "nothing is owed when no push was attempted: {:?}",
                receipt.warnings
            );
            assert_eq!(
                rev(env, "refs/remotes/origin/dev")?,
                None,
                "a --no-push apply must not reach the remote"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── the exit-code contract, checked from the library side ────────

    #[test]
    fn a_held_rebuild_reports_applied_with_holds_and_still_exits_two() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            conflicting_pair(env)?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;

            let plan = plan(env, PlanPurpose::Preview, false)?;
            assert_eq!(
                plan.compositions[0].held().count(),
                1,
                "one of the pair must be held"
            );

            let receipt = apply(env, &plan, false)?;
            assert_eq!(
                receipt.outcome,
                OperationOutcome::AppliedWithHolds,
                "holds must never be collapsed into Applied"
            );
            assert!(
                receipt
                    .warnings
                    .iter()
                    .any(|w| w.message.contains("held out")),
                "a hold must be reported as a warning: {:?}",
                receipt.warnings
            );
            assert!(
                !receipt.warnings.iter().any(|w| w.owes_effect),
                "a hold is not an owed effect"
            );

            // And the CLI contract: exit 2 means "succeeded, with holds".
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_clean_rebuild_reports_applied_and_exits_zero() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Preview, false)?;
            assert_eq!(plan.compositions[0].held().count(), 0);
            let receipt = apply(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(0);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── a push failure is a warning with an owed effect, not a success ──

    #[test]
    fn a_landed_push_reports_a_remote_effect_and_owes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            init_bare_origin(env, "origin", false)?;
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, true)?;
            assert!(
                plan.effects
                    .iter()
                    .any(|e| matches!(e, PlannedEffect::RemoteRefUpdate { .. })),
                "a push-enabled plan must predict the remote effect"
            );
            let predicted = plan.compositions[0].result_sha.clone();
            let receipt = apply(env, &plan, true)?;

            assert!(
                matches!(
                    applied_for(&receipt.effects, "refs/remotes/origin/dev"),
                    AppliedEffect::RemoteRefUpdate { .. }
                ),
                "a landed push must be reported as a remote effect: {:?}",
                receipt.effects
            );
            assert_eq!(
                rev(env, "refs/remotes/origin/dev")?.as_deref(),
                Some(predicted.as_str()),
                "record_pushed_tip must have moved the remote-tracking ref"
            );
            assert!(
                !receipt.has_owed_effects(),
                "a landed push owes nothing: {:?}",
                receipt.warnings
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_failed_push_is_reported_as_owed_rather_than_as_fully_synced() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            // `origin` exists and can be fetched from, but refuses every push.
            init_bare_origin(env, "origin", true)?;
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = plan(env, PlanPurpose::Confirm, true)?;
            assert!(
                plan.effects
                    .iter()
                    .any(|e| matches!(e, PlannedEffect::RemoteRefUpdate { .. })),
                "the plan must have predicted the push that will fail"
            );
            let receipt = apply(env, &plan, true)?;

            // The local publish really happened — a failed push never undoes
            // it, and conflating the two would throw away a real rebuild.
            assert_eq!(
                rev(env, "refs/heads/dev")?.as_deref(),
                Some(plan.compositions[0].result_sha.as_str()),
                "a failed push must not un-publish the local branch"
            );
            // And the receipt is honest about the debt.
            assert!(
                receipt.has_owed_effects(),
                "a failed push must be reported as an owed effect: {:?}",
                receipt.warnings
            );
            assert!(
                !receipt
                    .effects
                    .iter()
                    .any(|e| matches!(e, AppliedEffect::RemoteRefUpdate { .. })),
                "a push that did not land must not be reported as applied: {:?}",
                receipt.effects
            );
            assert_eq!(
                rev(env, "refs/remotes/origin/dev")?,
                None,
                "the remote must not have moved"
            );

            // The command still succeeds: a failed push is not a failed
            // rebuild. The debt is reported, not turned into an error.
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(0);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── preview and apply agree ──────────────────────────────────────

    #[test]
    fn a_preview_and_a_confirm_plan_agree_about_every_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            conflicting_pair(env)?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;

            let preview = plan(env, PlanPurpose::Preview, false)?;
            let confirm = plan(env, PlanPurpose::Confirm, false)?;

            let states = |plan: &Plan| -> Vec<String> {
                plan.compositions[0]
                    .branches
                    .iter()
                    .map(|b| format!("{}: {}", b.branch, describe(&b.state)))
                    .collect()
            };
            assert_eq!(
                states(&preview),
                states(&confirm),
                "the preview and the build must reach the same verdict, per branch"
            );
            // P1's invariant restated at the plan level: the same composition,
            // twice, through one implementation. Compared as *trees*, because
            // `commit_tree` stamps wall-clock time — and that is exactly why
            // the plan carries its own commit rather than recomputing one at
            // apply time: recomputing would silently land a different SHA than
            // the one the receipt promised.
            assert_eq!(
                tree_of(env, &preview.compositions[0].result_sha)?,
                tree_of(env, &confirm.compositions[0].result_sha)?,
                "identical inputs must compose to identical content"
            );
            discard_plan(&context_for(env, false)?, &confirm);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    fn describe(state: &hitch::operations::model::PlannedBranchState) -> String {
        use hitch::operations::model::PlannedBranchState::*;
        match state {
            Included => "included".into(),
            Held { conflicts_with, .. } => format!("held against {conflicts_with}"),
            ReplayedResolution { resolution_id } => format!("replayed {resolution_id}"),
            AlreadyInBase => "already in base".into(),
            Missing => "missing".into(),
        }
    }

    // ── locking is untouched by any of this ──────────────────────────

    #[test]
    fn a_locked_environment_still_refuses_without_force() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_failure()
                .assert_stderr_contains("locked");
            // And `--force` still gets through.
            env.hitch
                .run()
                .args(&["rebuild", "dev", "--force"])
                .execute()?
                .assert_success();
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}
