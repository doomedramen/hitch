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
    use hitch::commands::global_context::{GlobalContext, GlobalFlags};
    use hitch::core::state::EnvironmentHealth;
    use hitch::operations::declaration::{
        apply_declaration_plan, plan_demote, plan_promote, DeclarationPlanDetail,
        DeclarationPlanOptions,
    };
    use hitch::operations::model::{
        AppliedEffect, DependentRebuildOutcome, OperationOutcome, OperationPlan, PlanApplyError,
        PlanWarningKind, PlannedEffect,
    };
    use hitch::operations::rebuild::{
        apply_rebuild_plan, discard_plan, plan_rebuild, validate_plan, PlanPurpose,
        RebuildPlanDetail, RebuildPlanOptions,
    };
    use hitch::operations::release::{
        apply_release_plan, discard_release_plan, plan_release, ReleasePlanDetail,
        ReleasePlanOptions,
    };
    use hitch::utils::confirm::AlwaysNoConfirm;
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
            GlobalFlags {
                verbose: false,
                no_push: !push,
                assume_yes: true,
                json: false,
            },
            logger,
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))
    }

    /// A context whose gate will be consulted and will decline: interactive
    /// (`assume_yes` false) and willing to push, so `plan_rebuild` marks the
    /// plan as needing confirmation in the first place.
    fn context_declining(env: &TestEnvironment) -> anyhow::Result<GlobalContext> {
        let mut context = GlobalContext::new_at_path(
            env.temp_dir.to_str().expect("utf-8 temp dir"),
            GlobalFlags {
                verbose: false,
                no_push: false,
                assume_yes: false,
                json: false,
            },
            Arc::new(Logger::new()),
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))?;
        context.confirm = Arc::new(AlwaysNoConfirm);
        Ok(context)
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

    /// `Cow` because one variant's ref is derived from one of its own fields
    /// (`refs/tags/<name>`), so the model does not store it twice. Collecting
    /// into `String` here rather than `&str` is the cost of that, and the
    /// assertions below only ever compare or print.
    fn refnames(effects: &[PlannedEffect]) -> Vec<String> {
        effects.iter().map(|e| e.refname().into_owned()).collect()
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
            assert!(names.iter().any(|r| r == "refs/heads/dev"), "{names:?}");
            assert!(
                names.iter().any(|r| r == "refs/hitch/state/dev"),
                "{names:?}"
            );
            assert!(
                names.iter().any(|r| r == "refs/heads/hitch-metadata"),
                "{names:?}"
            );
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

            // The hold is a *prediction* and lives in the plan. It is decided by
            // `compose_environment` before the apply starts, so re-printing it
            // in the receipt would assert a future in a document about the
            // past — and the apply did not learn it, so by
            // `ExecutionReceipt::warnings` it does not belong there at all.
            assert!(
                plan.warnings.iter().any(|w| w.message.contains("held out")),
                "the hold belongs to the plan, as a prediction: {:?}",
                plan.warnings
            );
            assert!(
                !receipt
                    .warnings
                    .iter()
                    .any(|w| w.message.contains("held out")),
                "a receipt must not re-print the plan's prediction: {:?}",
                receipt.warnings
            );
            assert!(
                !receipt.has_owed_effects(),
                "a hold is not owed work: {:?}",
                receipt.warnings
            );

            // The *fact* is the authority's, not the plan's: the post-state says
            // the environment is partially realised and names what was held.
            // Without this the receipt would carry no trace of the hold beyond
            // the outcome enum, and the plan it quotes would be the only
            // account of it.
            let resulting = receipt
                .resulting_state
                .as_ref()
                .expect("a receipt for a rebuild reports the resulting state")
                .environments
                .iter()
                .find(|e| e.name == "dev")
                .expect("the resulting state names the rebuilt environment");
            assert!(
                matches!(
                    &resulting.health,
                    EnvironmentHealth::PartiallyRealised { held } if held.len() == 1
                ),
                "the hold must be readable from the resulting state, not only \
                 from the plan: {:?}",
                resulting.health
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
    // ── promote: the declaration and the build it forces ─────────────

    type DeclarationPlan = OperationPlan<DeclarationPlanDetail>;
    type ReleasePlan = OperationPlan<ReleasePlanDetail>;

    fn promote_plan(
        env: &TestEnvironment,
        argument: &str,
        environment: &str,
        no_rebuild: bool,
        push: bool,
    ) -> anyhow::Result<DeclarationPlan> {
        plan_promote(
            &context_for(env, push)?,
            argument,
            environment,
            DeclarationPlanOptions { no_rebuild },
            &mut |_| {},
        )
    }

    fn demote_plan(
        env: &TestEnvironment,
        argument: &str,
        environment: &str,
        no_rebuild: bool,
        push: bool,
    ) -> anyhow::Result<DeclarationPlan> {
        plan_demote(
            &context_for(env, push)?,
            argument,
            environment,
            DeclarationPlanOptions { no_rebuild },
            &mut |_| {},
        )
    }

    fn apply_declaration(
        env: &TestEnvironment,
        plan: &DeclarationPlan,
        push: bool,
    ) -> anyhow::Result<Receipt> {
        apply_declaration_plan(&context_for(env, push)?, plan, &mut |_| {})
    }

    fn release_plan(
        env: &TestEnvironment,
        environment: &str,
        target: &str,
        options: ReleasePlanOptions,
        push: bool,
    ) -> anyhow::Result<ReleasePlan> {
        plan_release(
            &context_for(env, push)?,
            environment,
            target,
            options,
            PlanPurpose::Confirm,
            &mut |_| {},
        )
    }

    fn apply_release(
        env: &TestEnvironment,
        plan: &ReleasePlan,
        push: bool,
    ) -> anyhow::Result<Receipt> {
        apply_release_plan(&context_for(env, push)?, plan, &mut |_| {})
    }

    fn declared(env: &TestEnvironment, environment: &str) -> anyhow::Result<Vec<String>> {
        let config = env.read_hitch_config()?;
        Ok(config
            .environments
            .get(environment)
            .map(|e| e.branches.clone())
            .unwrap_or_default())
    }

    /// The `AppliedEffect` for a dependent environment, so a test can assert on
    /// *how it went* rather than merely that something was recorded.
    fn dependent_outcome(
        effects: &[AppliedEffect],
        environment: &str,
    ) -> Option<DependentRebuildOutcome> {
        effects.iter().find_map(|e| match e {
            AppliedEffect::DependentEnvironmentRebuild {
                environment: e2,
                outcome,
                ..
            } if e2 == environment => Some(outcome.clone()),
            _ => None,
        })
    }

    /// Anchors are the one thing every planner writes that nothing prunes —
    /// `cleanup`'s prunable set is `["backup", "prev"]` — so "the anchor is
    /// gone" is asserted after every release path, not just the happy one.
    fn assert_no_release_anchors(env: &TestEnvironment) -> anyhow::Result<()> {
        let refs = all_refs(env)?;
        let leaked: Vec<&String> = refs
            .iter()
            .filter(|r| r.contains("refs/hitch/release/"))
            .collect();
        assert!(
            leaked.is_empty(),
            "release must not leave a live anchor behind, and nothing prunes one for it: {leaked:?}"
        );
        Ok(())
    }

    #[test]
    fn a_promote_plan_proposes_the_declaration_and_the_build_it_forces() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;

            let plan = promote_plan(env, "feat-a", "dev", false, false)?;

            // The proposal is the declaration *and* the build, because the
            // second is a consequence of the first that a reader has to be told
            // about — a promote that silently rebuilt nothing would look
            // identical to one that did the right thing.
            assert!(
                plan.current.branches.is_empty(),
                "current declaration should be empty: {:?}",
                plan.current.branches
            );
            let proposed: Vec<&str> = plan
                .proposed
                .branches
                .iter()
                .map(|p| p.branch.as_str())
                .collect();
            assert_eq!(proposed, vec!["feat-a"]);

            let names = refnames(&plan.effects);
            assert!(
                names.iter().any(|r| r == "refs/heads/hitch-metadata"),
                "the declaration edit is an effect: {names:?}"
            );
            assert!(
                names.iter().any(|r| r == "refs/heads/dev"),
                "the build is an effect, not a side effect: {names:?}"
            );
            match planned_for(&plan.effects, "refs/heads/dev") {
                PlannedEffect::DependentEnvironmentRebuild {
                    environment,
                    because,
                    ..
                } => {
                    assert_eq!(environment, "dev");
                    assert!(
                        !because.is_empty(),
                        "an effect that cannot say why is a list of side effects"
                    );
                }
                other => panic!("expected a DependentEnvironmentRebuild, got {other:?}"),
            }

            let receipt = apply_declaration(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert_eq!(
                dependent_outcome(&receipt.effects, "dev"),
                Some(DependentRebuildOutcome::Rebuilt)
            );
            assert_eq!(declared(env, "dev")?, vec!["feat-a".to_string()]);
            assert!(
                rev(env, "refs/heads/dev")?.is_some(),
                "the environment branch exists"
            );
            assert!(
                rev(env, "dev:feat-a.txt")?.is_some(),
                "and it was built from the declaration that was just written"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_promote_receipt_describes_the_declaration_by_reading_it_back() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;

            let plan = promote_plan(env, "feat-a", "dev", false, false)?;
            let receipt = apply_declaration(env, &plan, false)?;

            // The description is computed from a fresh read of `hitch-metadata`,
            // not from `plan.detail.added`. A description built from the plan
            // would report success for an edit a later `hitch set` had already
            // undone — the receipt's whole job is saying what is there.
            let description = match applied_for(&receipt.effects, "refs/heads/hitch-metadata") {
                AppliedEffect::MetadataChange { description, .. } => description.clone(),
                other => panic!("expected a MetadataChange, got {other:?}"),
            };
            assert_eq!(
                description, "promote feat-a into 'dev' (now: feat-a)",
                "the description is built from the declaration as it now stands"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_stale_promote_plan_refuses_and_names_what_changed() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &[])?;

            let stale = promote_plan(env, "feat-a", "dev", false, false)?;

            // A second, unrelated declaration change is exactly the drift a
            // fingerprint exists to catch. It moves `hitch-metadata`, which the
            // first plan read.
            let other = promote_plan(env, "feat-b", "dev", false, false)?;
            apply_declaration(env, &other, false)?;

            let error = apply_declaration(env, &stale, false)
                .expect_err("a plan that predates another declaration edit must not apply");
            assert!(as_stale(&error), "expected a StalePlan, got: {error:#}");
            let detail = error
                .downcast_ref::<PlanApplyError>()
                .expect("typed error survives the anyhow boundary");
            let PlanApplyError::StalePlan { changed, .. } = detail else {
                panic!("expected StalePlan, got {detail:?}");
            };
            assert!(
                changed.contains("hitch-metadata"),
                "the refusal must name the ref that moved, not just say it is \
                 stale: {changed:?}"
            );

            // And the refusal changed nothing: feat-a is not in the declaration.
            let branches = declared(env, "dev")?;
            assert_eq!(
                branches,
                vec!["feat-b".to_string()],
                "a refused plan must not have applied its own edit"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn the_same_declaration_gives_the_same_plan_id_and_a_moved_tip_gives_a_different_one(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;

            let first = promote_plan(env, "feat-a", "dev", false, false)?;
            let second = promote_plan(env, "feat-a", "dev", false, false)?;
            assert_eq!(
                first.id, second.id,
                "the id is a digest of what the plan depends on, so identical inputs \
                 must give an identical id — otherwise it identifies nothing"
            );

            add_commit(env, "feat-a", "feat-a.txt", "v2")?;
            let third = promote_plan(env, "feat-a", "dev", false, false)?;
            assert_ne!(
                first.id, third.id,
                "a moved promoted-branch tip is a different plan, and the id has to say so"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_no_rebuild_promote_leaves_the_environment_absent_and_says_which_command_fixes_it(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;

            let plan = promote_plan(env, "feat-a", "dev", true, false)?;
            // The plan says the consequence out loud, before the apply, so a
            // preview is not a lie.
            assert!(
                plan.warnings
                    .iter()
                    .any(|w| w.message.contains("hitch rebuild dev")),
                "the plan must name the command that settles the skipped build: {:?}",
                plan.warnings
            );
            assert!(
                !refnames(&plan.effects)
                    .iter()
                    .any(|r| r == "refs/heads/dev"),
                "a plan that will not rebuild must not predict a build: {:?}",
                refnames(&plan.effects)
            );

            let receipt = apply_declaration(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert_eq!(declared(env, "dev")?, vec!["feat-a".to_string()]);
            assert!(
                rev(env, "dev")?.is_none(),
                "--no-rebuild means the environment branch is not created"
            );
            assert!(
                dependent_outcome(&receipt.effects, "dev").is_none(),
                "a build that was not attempted is not reported as one"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_gated_promote_asks_and_writes_no_declaration_and_no_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;
            // An approver is mandatory, not decoration: hitch refuses a gated
            // environment nobody could ever approve.
            env.hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                ])
                .execute()?
                .assert_success();

            let plan = promote_plan(env, "feat-a", "dev", false, false)?;
            let blocking = plan
                .blocked_by()
                .expect("an approval-gated environment must block its own plan");
            assert_eq!(blocking.kind, PlanWarningKind::ApprovalRequired);
            assert_eq!(
                plan.current.branches, plan.proposed.branches,
                "a plan that will not apply must not claim a different end state"
            );
            assert_eq!(plan.current.branch_sha, plan.proposed.branch_sha);

            let receipt = apply_declaration(env, &plan, false)?;
            assert_eq!(
                receipt.outcome,
                OperationOutcome::ApprovalRequested,
                "an approval gate asks; it does not refuse and it does not apply"
            );
            assert!(
                declared(env, "dev")?.is_empty(),
                "asking for approval must not promote the branch"
            );
            assert!(rev(env, "dev")?.is_none(), "and must not build it either");
            let config = env.read_hitch_config()?;
            assert_eq!(
                config
                    .approval_requests
                    .iter()
                    .filter(|r| r.branch == "feat-a")
                    .count(),
                1,
                "the apply's job here is to create the request"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_conflicting_sibling_refuses_the_promote_and_writes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            conflicting_pair(env)?;
            declare_branches(env, "dev", &["feat-a"])?;

            let before = all_refs(env)?;
            let plan = promote_plan(env, "feat-b", "dev", false, false)?;
            let blocking = plan
                .blocked_by()
                .expect("promoting a branch that conflicts with a promoted sibling must refuse");
            assert_eq!(blocking.kind, PlanWarningKind::PolicyRefusal);
            assert_eq!(plan.current.branches, plan.proposed.branches);

            let error =
                apply_declaration(env, &plan, false).expect_err("a refused plan must not apply");
            let detail = error
                .downcast_ref::<PlanApplyError>()
                .expect("typed error survives the anyhow boundary");
            assert!(
                matches!(detail, PlanApplyError::PolicyBlocked { .. }),
                "expected PolicyBlocked, got {detail:?}"
            );
            assert!(
                detail.to_string().contains("hitch promote"),
                "a refusal must end with the command to run next: {detail}"
            );

            assert_eq!(
                all_refs(env)?,
                before,
                "a refusal must move no ref — not even a build record or an anchor"
            );
            assert_eq!(
                declared(env, "dev")?,
                vec!["feat-a".to_string()],
                "and must not have edited the declaration"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── demote ─────────────────────────────────────────────────────

    #[test]
    fn a_demote_shrinks_the_declaration_and_rebuilds_from_the_shorter_list() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;

            let plan = demote_plan(env, "feat-a", "dev", false, false)?;
            let current: Vec<&str> = plan
                .current
                .branches
                .iter()
                .map(|p| p.branch.as_str())
                .collect();
            let proposed: Vec<&str> = plan
                .proposed
                .branches
                .iter()
                .map(|p| p.branch.as_str())
                .collect();
            assert_eq!(current, vec!["feat-a", "feat-b"]);
            assert_eq!(proposed, vec!["feat-b"], "the survivor keeps its order");

            let receipt = apply_declaration(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert_eq!(declared(env, "dev")?, vec!["feat-b".to_string()]);

            // The build has to match the *new* declaration, not the old one.
            // A rebuild that composed from the pre-demote list would leave
            // feat-a's content in the environment branch forever.
            assert!(
                rev(env, "dev:feat-a.txt")?.is_none(),
                "the demoted branch's content must be gone from the build"
            );
            assert!(
                rev(env, "dev:feat-b.txt")?.is_some(),
                "and the survivor's must still be there"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_stale_demote_plan_refuses_after_a_second_demote() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            make_feature(env, "feat-c")?;
            declare_branches(env, "dev", &["feat-a", "feat-b", "feat-c"])?;

            let stale = demote_plan(env, "feat-c", "dev", false, false)?;
            let other = demote_plan(env, "feat-b", "dev", false, false)?;
            apply_declaration(env, &other, false)?;

            let error = apply_declaration(env, &stale, false)
                .expect_err("a plan that predates another declaration edit must not apply");
            assert!(as_stale(&error), "expected a StalePlan, got: {error:#}");
            assert_eq!(
                declared(env, "dev")?,
                vec!["feat-a".to_string(), "feat-c".to_string()],
                "the stale demote must not have removed feat-c — the only edit \
                 that landed is the one its plan actually made"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    // ── release ────────────────────────────────────────────────────

    #[test]
    fn a_release_plan_names_the_tag_the_target_move_the_prunes_and_the_dependents(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &["feat-a"])?;
            declare_branches(env, "qa", &["feat-a", "feat-b"])?;

            let plan = release_plan(env, "dev", "main", ReleasePlanOptions::default(), false)?;

            // The tag is a name, not a value: the executor may land a
            // disambiguated variant of it, so the plan predicts and the
            // receipt reports. Both halves are tested.
            let tag_ref = format!("refs/tags/{}", plan.detail.tag_name);
            assert!(
                refnames(&plan.effects).contains(&tag_ref),
                "the tag is a write, so the plan says so: {:?}",
                refnames(&plan.effects)
            );
            let names = refnames(&plan.effects);
            assert!(names.iter().any(|r| r == "refs/heads/main"), "{names:?}");
            assert!(
                names.iter().any(|r| r == "refs/heads/qa"),
                "qa is rebuilt because its base is the released target: {names:?}"
            );
            assert!(
                !names.iter().any(|r| r.starts_with("refs/remotes/")),
                "with pushing off the plan predicts no remote effect: {names:?}"
            );

            // The prune predicate is evaluated against the commit about to be
            // published, so `dev` (whose base *is* the target) is pruned even
            // though the live `main` does not contain feat-a yet.
            let pruned: Vec<(&str, Vec<&str>)> = plan
                .detail
                .prunes
                .iter()
                .map(|p| {
                    (
                        p.environment.as_str(),
                        p.branches.iter().map(|b| b.as_str()).collect(),
                    )
                })
                .collect();
            assert_eq!(
                pruned,
                vec![("dev", vec!["feat-a"]), ("qa", vec!["feat-a"])],
                "prunes are in environment name order and name the branch each time"
            );
            let dependent_names: Vec<&str> = plan
                .detail
                .dependents
                .iter()
                .map(|d| d.environment.as_str())
                .collect();
            assert_eq!(dependent_names, vec!["dev", "qa"]);

            let receipt = apply_release(env, &plan, false)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            match applied_for(&receipt.effects, &tag_ref) {
                // The receipt names the tag that exists, which here is the one
                // predicted. The disambiguation arm is covered by the unit tests
                // on `create_release_tag`.
                AppliedEffect::TagCreation { name, target_sha } => {
                    assert_eq!(name, &plan.detail.tag_name);
                    assert_eq!(target_sha, &plan.detail.result_sha);
                }
                other => panic!("expected a TagCreation, got {other:?}"),
            }
            assert_eq!(declared(env, "qa")?, vec!["feat-b".to_string()]);
            assert_no_release_anchors(env)?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_conflicting_release_writes_nothing_at_all() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            conflicting_pair(env)?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;
            let target_before = rev(env, "refs/heads/main")?;
            let metadata_before = rev(env, "refs/heads/hitch-metadata")?;
            let before = all_refs(env)?;

            let error = release_plan(env, "dev", "main", ReleasePlanOptions::default(), false)
                .expect_err("a conflicting release must not produce a plan");
            assert!(
                error.to_string().contains("Merge conflict"),
                "expected the conflict report, got: {error:#}"
            );

            // Each of these asserted separately. "Nothing happened" is three
            // different claims — no ref moved, no tag exists, no metadata was
            // written — and the reason a release is all-or-nothing is that the
            // composition runs before anything is written.
            assert_eq!(rev(env, "refs/heads/main")?, target_before);
            assert_eq!(rev(env, "refs/heads/hitch-metadata")?, metadata_before);
            assert_eq!(
                all_refs(env)?,
                before,
                "a refused release must not have created a tag or an anchor"
            );
            let tags = git_plain(&env.temp_dir, &["tag", "--list"])?;
            assert!(
                tags.trim().is_empty(),
                "a refused release must not leave a tag: {tags:?}"
            );
            assert_no_release_anchors(env)?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_release_that_owes_a_dependent_rebuild_still_releases() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            make_feature(env, "feat-b")?;
            declare_branches(env, "dev", &["feat-a"])?;
            declare_branches(env, "staging", &["feat-b"])?;

            let plan = release_plan(env, "dev", "main", ReleasePlanOptions::default(), false)?;
            assert!(
                plan.detail
                    .dependents
                    .iter()
                    .any(|d| d.environment == "staging"),
                "staging bases on the released target, so it is a dependent: {:?}",
                plan.detail.dependents
            );

            // Break the dependent *after* planning. This is the documented
            // shape of the fingerprint: it names the release's own inputs and
            // the target, not every branch a dependent rebuild will read — that
            // rebuild fingerprints itself. So the plan is still current, and the
            // apply finds out the hard way.
            env.git.run(&["branch", "-D", "feat-b"])?;

            let receipt = apply_release(env, &plan, false)?;

            // The release itself is untouched by the dependent's failure: the
            // merge and the tag landed before the rebuild was even attempted,
            // and failing the whole operation would send the user to re-run a
            // release that succeeded — into a target that has already moved.
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert_eq!(
                rev(env, "refs/heads/main")?,
                Some(plan.detail.result_sha.clone())
            );
            let tag_ref = format!("refs/tags/{}", plan.detail.tag_name);
            assert!(
                rev(env, &tag_ref)?.is_some(),
                "the tag the plan named is the tag that exists: {tag_ref}"
            );
            match dependent_outcome(&receipt.effects, "staging") {
                Some(DependentRebuildOutcome::Failed(_)) => {}
                other => panic!("expected staging's rebuild to be Failed, got {other:?}"),
            }
            assert!(
                receipt.has_owed_effects(),
                "a failed dependent is owed work, not a silent omission: {:?}",
                receipt.warnings
            );
            assert!(
                receipt
                    .warnings
                    .iter()
                    .any(|w| w.owes_effect && w.message.contains("hitch rebuild staging")),
                "and the warning must name the command that settles it: {:?}",
                receipt.warnings
            );
            assert_no_release_anchors(env)?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_release_whose_push_is_denied_is_owed_a_plain_fast_forward() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            init_bare_origin(env, "origin-denied", true)?;

            let plan = release_plan(env, "dev", "main", ReleasePlanOptions::default(), true)?;
            assert!(
                refnames(&plan.effects)
                    .iter()
                    .any(|r| r == "refs/remotes/origin/main"),
                "a plan that will push predicts the remote effect: {:?}",
                refnames(&plan.effects)
            );

            let receipt = apply_release(env, &plan, true)?;
            assert_eq!(receipt.outcome, OperationOutcome::Applied);
            assert!(receipt.has_owed_effects());
            let owed: Vec<&str> = receipt
                .warnings
                .iter()
                .filter(|w| w.owes_effect)
                .map(|w| w.message.as_str())
                .collect();
            assert!(
                owed.iter().any(|m| m.contains("hitch push main")),
                "the remedy must be named: {owed:?}"
            );
            assert!(
                !owed.iter().any(|m| m.contains("hitch push main -f")),
                "release's push is a fast-forward, so `-f` is advice the remote \
                 will reject on a protected branch: {owed:?}"
            );
            // The local publish is real, and the receipt must not claim the
            // remote moved.
            assert_eq!(
                rev(env, "refs/heads/main")?,
                Some(plan.detail.result_sha.clone())
            );
            assert!(
                !refnames_applied(&receipt.effects)
                    .iter()
                    .any(|r| r == "refs/remotes/origin/main"),
                "a denied push must not appear in the receipt's effects"
            );
            assert_no_release_anchors(env)?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `Cow`-returning counterpart to [`refnames`], for the applied side.
    fn refnames_applied(effects: &[AppliedEffect]) -> Vec<String> {
        effects.iter().map(|e| e.refname().into_owned()).collect()
    }

    // ---------------------------------------------------------------------
    // P6. Everything above drives the library, because what it is about is a
    // structured claim. What follows drives the *binary*, because the channel
    // contract — `--json` owns stdout and nothing else does, prose owns
    // everything else — and `PlanPurpose`'s safety property are only
    // observable from outside the process.
    // ---------------------------------------------------------------------

    /// Two promoted branches that cannot compose: both rewrite `shared.txt`
    /// from the same ancestor, incompatibly.
    ///
    /// Written straight into `hitch.json` rather than promoted, because
    /// `promote` has its own conflict gate and would refuse the second branch —
    /// and a fixture that needs the command under test's permission to exist is
    /// not a fixture.
    fn declare_conflicting_pair(env: &TestEnvironment) -> anyhow::Result<()> {
        for (branch, body) in [("feat-left", "left"), ("feat-right", "right")] {
            env.git.run(&["checkout", "-b", branch])?;
            env.fs.write_file("shared.txt", body)?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git
                .run(&["commit", "-m", &format!("{branch}: rewrite shared.txt")])?;
            env.git.run(&["checkout", "main"])?;
        }
        declare_branches(env, "dev", &["feat-left", "feat-right"])
    }

    fn json_document(stdout: &str) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::from_str(stdout)?)
    }

    /// The top-level key set of a `--json` document, sorted.
    fn envelope_keys(value: &serde_json::Value) -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .expect("a --json document is an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn the_json_document_is_a_versioned_envelope_over_a_plan_and_a_receipt() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &[])?;
            env.hitch
                .run()
                .args(&["promote", "feat-a", "dev"])
                .execute()?
                .assert_success();

            let result = env
                .hitch
                .run()
                .args(&["--json", "rebuild", "dev"])
                .execute()?;
            let document = json_document(&result.stdout())?;
            result.assert_success();

            // The three keys, and only those three. A document's shape is its
            // contract with a consumer that has never read this source, so it
            // is pinned exactly: an added top-level key is a silent
            // compatibility change, and a removed one is a silent break, and
            // neither would fail a test that only looked for what it wanted.
            assert_eq!(
                envelope_keys(&document),
                vec!["plan", "receipt", "schema_version"]
            );
            assert_eq!(
                document["schema_version"].as_u64(),
                Some(1),
                "the version is what lets a consumer tell a missing key from a new one"
            );
            assert!(
                document["plan"].is_object(),
                "the plan half of the envelope: {document}"
            );
            assert!(
                document["receipt"].is_object(),
                "an applied operation has a receipt, and saying `null` here would \
                 be indistinguishable from a preview: {document}"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_preview_document_says_no_receipt_rather_than_omitting_the_key() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let result = env
                .hitch
                .run()
                .args(&["--json", "rebuild", "dev", "--dry-run"])
                .execute()?;
            let document = json_document(&result.stdout())?;
            result.assert_success();

            // `receipt: null`, not an absent `receipt`. The two mean different
            // things to a consumer: one is a command that decided and stopped,
            // the other is a schema that forgot to say. `null` is the answer
            // that cannot be confused with the second.
            assert_eq!(
                envelope_keys(&document),
                vec!["plan", "receipt", "schema_version"]
            );
            assert!(
                document["receipt"].is_null(),
                "a preview has nothing to report about what happened: {document}"
            );
            assert!(document["plan"].is_object());
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn json_stdout_carries_no_escape_bytes_and_the_prose_path_carries_them() -> anyhow::Result<()> {
        const ESC: char = '\u{1b}';
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let prose_run = env
                .hitch
                .run()
                .args(&["rebuild", "dev", "--dry-run"])
                .execute()?;
            let prose = prose_run.stdout();
            prose_run.assert_success();

            let json_run = env
                .hitch
                .run()
                .args(&["--json", "rebuild", "dev", "--dry-run"])
                .execute()?;
            let document = json_run.stdout();
            json_run.assert_success();

            // Asserted in the order that makes the second assertion mean
            // something. "The document has no escape bytes" passes trivially
            // on a build whose renderer emitted none either; the control
            // assertion rules that out by requiring the *other* path to have
            // them, which is what `colored`'s forced override guarantees even
            // through a pipe.
            assert!(
                prose.contains(ESC),
                "the control: the prose path must colour, or the next assertion \
                 passes for the wrong reason:\n{prose}"
            );
            assert!(
                !document.contains(ESC),
                "a JSON consumer does not want SGR sequences inside its document:\n{document}"
            );
            // And it still parses, which is the point of the flag.
            json_document(&document)?;
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    #[test]
    fn a_json_document_survives_the_exit_two_that_it_exists_to_explain() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            declare_conflicting_pair(env)?;

            // `process::exit` skips normal shutdown, so buffered stdout is lost
            // in exactly the case the flag is for: a CI job that ran out of
            // patience mid-rebuild, exited 2, and now has an error code and no
            // way to find out why. This test only has value because the exit
            // path really does call `exit` rather than returning.
            let result = env
                .hitch
                .run()
                .args(&["--json", "rebuild", "dev"])
                .execute()?;
            let document = json_document(&result.stdout())?;
            result.assert_exit_code(2);
            assert_eq!(
                envelope_keys(&document),
                vec!["plan", "receipt", "schema_version"]
            );
            // The exit code is a claim the *document* has to back up, so the
            // two are asserted together: this receipt has to be the one that
            // says a branch was held, or the 2 is a number with no explanation
            // behind it.
            assert_eq!(
                document["receipt"]["outcome"].as_str(),
                Some("AppliedWithHolds"),
                "the typed form of the exit-2 fact: {document}"
            );

            // The hold is named twice, in two roles, and the roles are what keep
            // this from being the duplication the CLI had. The *plan* half
            // predicts it, with the partner and the file count, because that is
            // what a prediction is for and it was decided before the apply
            // began. The *receipt* half confirms it, in the authority's own
            // verdict, naming which branch was held. A consumer holding the
            // document gets both; one reading only `receipt.warnings` gets
            // nothing, and should — see `ExecutionReceipt::warnings`.
            let plan_warnings = document["plan"]["warnings"]
                .as_array()
                .expect("plan warnings is an array")
                .iter()
                .map(|w| w["message"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>();
            assert!(
                plan_warnings.iter().any(|m| m.contains("feat-right")),
                "the plan half must name the hold it predicts: {plan_warnings:?}"
            );

            let receipt_warnings = document["receipt"]["warnings"]
                .as_array()
                .expect("warnings is an array")
                .iter()
                .map(|w| w["message"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>();
            assert!(
                receipt_warnings.is_empty(),
                "a hold is nothing the apply discovered, so the receipt half must \
                 not restate the plan's prediction: {receipt_warnings:?}"
            );

            let held = document["receipt"]["resulting_state"]["environments"]
                .as_array()
                .expect("resulting_state names the environments")
                .iter()
                .find(|e| e["name"].as_str() == Some("dev"))
                .expect("dev is in the resulting state");
            assert_eq!(
                held["health"]["partially_realised"]["held"],
                serde_json::json!(["feat-right"]),
                "the fact behind the exit 2, from the authority: {held}"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// The generalisation behind the three per-site fixes: **a receipt's
    /// warnings never restate a plan's warnings.**
    ///
    /// The copy this forbids was one mechanical `map` repeated in `rebuild`,
    /// `declaration` and `release` (and a second one in `declaration`'s
    /// approval arm), so it produced four double-printed advisories and one
    /// sentence that appeared twice wearing `⛔` in the plan and `⚠️` in the
    /// receipt. Asserted across all three operations rather than once per site,
    /// because the failure mode is a *new* planner reintroducing the shape, not
    /// a regression in a particular executor.
    ///
    /// The three plans chosen here are the three advisory families that existed:
    /// a hold (rebuild), a `--no-rebuild` stale environment (declaration), and
    /// `--no-prune` (release). Each is a consequence decided *before* the apply
    /// began, which is the whole reason none of them is something the apply
    /// learned.
    #[test]
    fn a_receipt_never_restates_a_plan_warning() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            // A hold, which the rebuild planner turns into an advisory.
            conflicting_pair(env)?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;
            let rebuild = plan(env, PlanPurpose::Preview, false)?;
            assert!(
                !rebuild.warnings.is_empty(),
                "this test is vacuous unless the plan predicts something"
            );
            let rebuild_receipt = apply(env, &rebuild, false)?;
            assert_no_restated_warning(&rebuild, &rebuild_receipt);

            // A `--no-rebuild` stale environment, from the declaration planner.
            let declaration = plan_demote(
                &context_for(env, false)?,
                "feat-b",
                "dev",
                DeclarationPlanOptions { no_rebuild: true },
                &mut |_| {},
            )?;
            assert!(
                declaration
                    .warnings
                    .iter()
                    .any(|w| w.message.contains("left stale")),
                "this test is vacuous unless the plan predicts a stale environment: {:?}",
                declaration.warnings
            );
            let declaration_receipt =
                apply_declaration_plan(&context_for(env, false)?, &declaration, &mut |_| {})?;
            assert_no_restated_warning(&declaration, &declaration_receipt);

            // A `--no-prune` consequence, from the release planner.
            declare_branches(env, "qa", &["feat-a"])?;
            let release = release_plan(
                env,
                "dev",
                "main",
                ReleasePlanOptions {
                    no_prune: true,
                    ..Default::default()
                },
                false,
            )?;
            assert!(
                release
                    .warnings
                    .iter()
                    .any(|w| w.message.contains("stay in their declarations")),
                "this test is vacuous unless the plan predicts a --no-prune \
                 consequence: {:?}",
                release.warnings
            );
            let release_receipt = apply_release(env, &release, false)?;
            assert_no_restated_warning(&release, &release_receipt);

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A plan's warnings are predictions; a receipt's warnings are facts the
    /// apply discovered. No message may appear in both.
    ///
    /// Compared by message rather than by count, because the thing being
    /// forbidden is the *same sentence twice*: a receipt that happened to carry
    /// its own distinct warning is allowed, and a plan that carried none would
    /// pass vacuously — hence the `assert!(!plan.warnings.is_empty())` at every
    /// call site rather than inside here.
    fn assert_no_restated_warning<I>(plan: &OperationPlan<I>, receipt: &Receipt) {
        for warning in &receipt.warnings {
            assert!(
                !plan.warnings.iter().any(|p| p.message == warning.message),
                "a receipt warning restates a plan's prediction verbatim, so one \
                 fact is printed in two documents — and a prediction, at that: \
                 {:?}",
                warning.message
            );
        }
    }

    #[test]
    fn an_empty_release_prints_a_diagnostic_and_no_document() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // The one `--json` path that emits no document, and deliberately so.
            // A release of nothing has no plan and no receipt, and the envelope
            // has no honest contents: the alternative — a document describing
            // an operation that did not happen — is the failure mode this whole
            // program exists to remove. The consumer's contract is therefore
            // "exit 0 with an empty stdout means nothing was released", which
            // is a contract, where a fabricated document would not be one.
            let result = env
                .hitch
                .run()
                .args(&["--json", "release", "dev", "main"])
                .execute()?
                .assert_success();

            result
                .assert_stderr_contains("nothing to release")
                .assert_stdout_is_empty();
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// The asymmetry `PlanPurpose` exists to encode, asserted where it is
    /// actually made — the two calls differ in one argument.
    ///
    /// A preview and the plan it previews must name the *same* refs, or "dry
    /// run" is a second opinion from a second decision point. The one thing
    /// they may disagree about is the anchor, which is the property itself: a
    /// plan that might be applied keeps its composed commit reachable, a plan
    /// that will not leaves nothing behind. `cleanup`'s prunable set is
    /// `["backup", "prev"]`, so a wrong answer here is a leaked ref, silently,
    /// on every `--dry-run` a user ever ran.
    #[test]
    fn a_preview_and_the_plan_it_previews_name_the_same_refs_except_the_anchor(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            // Preview first, and the anchor check immediately after it: the
            // `Confirm` plan below creates one on purpose, so checking afterwards
            // would find that one and prove nothing about the preview.
            let preview = plan(env, PlanPurpose::Preview, false)?;
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/build/")),
                "planning a preview wrote an anchor, which is a write a dry run \
                 promised it would not make"
            );
            let confirm = plan(env, PlanPurpose::Confirm, false)?;

            let preview_refs: std::collections::BTreeSet<String> =
                refnames(&preview.effects).into_iter().collect();
            let confirm_refs: std::collections::BTreeSet<String> =
                refnames(&confirm.effects).into_iter().collect();

            let extra: Vec<&String> = confirm_refs.difference(&preview_refs).collect();
            let missing: Vec<&String> = preview_refs.difference(&confirm_refs).collect();

            assert!(
                missing.is_empty(),
                "a preview that predicts a ref the real plan will not touch is \
                 describing a different operation: only the applied plan has \
                 {missing:?}, missing from the preview of {preview_refs:?}"
            );
            assert_eq!(
                extra.len(),
                1,
                "the applied plan should differ from its preview by exactly one \
                 ref — the anchor — not by {extra:?}"
            );
            assert!(
                extra[0].starts_with("refs/hitch/build/"),
                "the one ref a preview does not predict must be the anchor, and \
                 the anchor is what makes that safe: {extra:?}"
            );
            assert!(
                preview.detail.anchor_ref.is_none(),
                "a preview must not anchor at all, whatever it went on to predict"
            );
            assert_eq!(
                confirm.detail.anchor_ref.as_deref(),
                Some(extra[0].as_str())
            );

            discard_plan(&context_for(env, false)?, &confirm);
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A decline is a decision, and a decision that leaves a ref behind is not
    /// one.
    ///
    /// The anchor exists only between planning and applying, and nothing prunes
    /// it — `cleanup`'s prunable set is `["backup", "prev"]`. A refused plan
    /// that skipped its discard would leak one ref per refusal, on a code path
    /// that looks like it did nothing at all.
    #[test]
    fn a_declined_rebuild_writes_nothing_and_leaves_no_anchor() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let metadata_before = git_plain(&env.temp_dir, &["rev-parse", "hitch-metadata"])?;
            let run = hitch::utils::prelude::rebuild_environment_gated(
                &context_for(env, false)?,
                "dev",
                false,
                None,
                hitch::utils::prelude::StepNarration::Suppressed,
                |_plan| Ok(false),
            )?;
            assert!(
                run.is_none(),
                "a declining gate returns None, distinct from a run that applied"
            );

            assert_eq!(
                rev(env, "refs/heads/dev")?,
                None,
                "a refused rebuild must not have created the environment branch"
            );
            assert_eq!(
                rev(env, "refs/hitch/state/dev")?,
                None,
                "a refused rebuild must not have written a build record — a \
                 record is a claim that a build happened"
            );
            assert_eq!(
                git_plain(&env.temp_dir, &["rev-parse", "hitch-metadata"])?,
                metadata_before,
                "the 'rebuilt_at' stamp is a metadata write, and a refusal wrote \
                 nothing"
            );
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/build/")),
                "a refused rebuild leaked its anchor"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// The same claim for a release, whose anchor lives in its own ref family —
    /// and whose preview is the one a user reaches for most often, because
    /// releasing is the operation people most want to see before doing.
    #[test]
    fn a_release_preview_leaves_the_target_the_tag_space_and_the_anchor_untouched(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let main_before = rev(env, "refs/heads/main")?;
            let tags_before = git_plain(&env.temp_dir, &["tag", "--list"])?;

            let plan = release_plan(env, "dev", "main", ReleasePlanOptions::default(), false)?;
            // Release's own cleanup, because a preview anchors nothing and this
            // is the call that says so rather than merely being safe.
            discard_release_plan(&context_for(env, false)?, &plan);

            assert_eq!(
                rev(env, "refs/heads/main")?,
                main_before,
                "a preview composed a release; it did not perform one"
            );
            assert_eq!(
                git_plain(&env.temp_dir, &["tag", "--list"])?,
                tags_before,
                "a preview must not create the release tag"
            );
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/release/")),
                "a release preview left an anchor behind"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A decline is a decision the *command* has to honour, not just the gated
    /// sequence: exit 0, nothing on stdout, and a repository that is exactly as
    /// it was.
    ///
    /// Driven through `commands::rebuild::run` rather than
    /// `rebuild_environment_gated`, because the claim under test is the
    /// command's — declining is not a failure, and a command that reported a
    /// refused confirmation as an error would train users to pass `--yes` to
    /// every mutating command, which is the opposite of the point.
    ///
    /// Two things make a decline reachable here, and both are needed:
    /// `GlobalContext.confirm` is a public field precisely so a test can answer
    /// "no" (the harness injects `--yes`, which would otherwise make declines
    /// unreachable from any test), *and* the context must not assume yes. A
    /// rebuild only requires confirmation when it owes a push
    /// (`plan_rebuild` reads `context.should_push()`), so `no_push` is false
    /// here — the real case in which a human is asked about a rebuild at all.
    ///
    /// The return value cannot carry this claim: `rebuild::run` answers
    /// `Ok(false)` both for a decline and for a clean apply with no holds, since
    /// `Ok(true)` is the exit-2 signal. The repository is what tells them apart.
    #[test]
    fn a_declined_rebuild_exits_zero_and_writes_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let context = context_declining(env)?;
            // Only reachable if the gate was actually consulted, so asserted
            // rather than assumed.
            assert!(
                !context.assume_yes,
                "a context that assumes yes never reaches the Confirm it is given"
            );

            let accepted = hitch::commands::rebuild::run(
                hitch::commands::rebuild::RebuildCommand {
                    env_name: "dev".to_string(),
                    force: false,
                    dry_run: false,
                    on_conflict: None,
                    pr_comments: false,
                    replay_resolutions: false,
                },
                &context,
            )?;
            assert!(
                !accepted,
                "a declined rebuild must not report holds: {accepted}"
            );

            assert_eq!(
                rev(env, "refs/heads/dev")?,
                None,
                "a declined rebuild must not have created the environment branch"
            );
            assert_eq!(
                rev(env, "refs/hitch/state/dev")?,
                None,
                "a build record is a claim that a build happened; nothing was built"
            );
            // On the *content* of the metadata, not its tip: taking the
            // environment lock and releasing it are two real commits, so the ref
            // moves even when the declaration does not. `rebuilt_at` is the
            // stamp a build writes, and it is the only thing a rebuild adds.
            let config = env.read_hitch_config()?;
            let dev = config
                .environments
                .get("dev")
                .expect("the fixture declared 'dev'");
            assert!(
                dev.rebuilt_at.is_none(),
                "a decline stamped 'rebuilt_at' on {:?}",
                dev.rebuilt_at
            );
            assert!(
                !dev.locked,
                "the lock was taken and released, so the flag must be clear again"
            );
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/build/")),
                "the command's decline path leaked the anchor the plan had already \
                 written. Nothing prunes refs/hitch/build/*, so this is one leaked \
                 ref per refusal, on a path that looks like it did nothing."
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A `--json` run without `--yes` must **fail**, and this is a regression test
    /// for a specific shape: `rebuild_environment_gated` used to match
    /// `Ok(false) | Err(_)` together, so the gate's refusal was reported as a
    /// decline — anchor discarded, `Ok(None)`, and the command exiting **0** with
    /// an empty stdout and the "re-run with `--yes`" reason thrown away. A CI
    /// consumer reads that as success. A unit test on `decide_gate` cannot catch
    /// it, because the bug was in how the caller handled the `Err`, so this one
    /// has to go through the binary.
    ///
    /// `with_no_push(false)` is load-bearing rather than incidental: a rebuild
    /// only *requires* confirmation when it owes a push (`plan_rebuild` reads
    /// `context.should_push()`), so with the harness's default `--no-push` this
    /// command would apply silently and the test would pass for the wrong
    /// reason.
    #[test]
    fn a_json_rebuild_without_yes_fails_loudly_and_leaves_nothing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let result = env
                .hitch
                .run()
                .args(&["--json", "rebuild", "dev"])
                .with_no_push(false)
                .with_yes(false)
                .execute()?;

            // `assert_*` take `self`, so read stderr before consuming the result.
            let stderr = result.stderr();
            result.assert_exit_code(1).assert_stdout_is_empty();
            assert!(
                stderr.contains("--yes"),
                "a refusal that does not name the flag that would clear it is a dead \
                 end:\n{stderr}"
            );

            assert_eq!(
                rev(env, "refs/heads/dev")?,
                None,
                "refusing to prompt must not have composed anything"
            );
            assert_eq!(rev(env, "refs/hitch/state/dev")?, None);
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/build/")),
                "the refusal path leaked the anchor. Nothing prunes \
                 refs/hitch/build/*, so this is one leaked ref per refusal."
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// The release half of the same rule, and a regression test for the arm that
    /// was actually written wrong: `perform_release_core` was
    /// `if !confirm_plan(...)? { discard_release_plan(...) }`, which discards on
    /// a *decline* and lets an `Err` propagate — so a `--json` release without
    /// `--yes` leaked `refs/hitch/release/*`, a family nothing prunes, one
    /// permanently leaked ref per refusal. Promote and demote escaped the same
    /// bug only because `plan_declaration_change` anchors nothing, which is
    /// exactly why the rule is about *arms* and not about commands: a new
    /// planner that anchors inherits the bug without inheriting the history.
    #[test]
    fn a_refused_json_release_leaves_no_anchor_and_no_tag() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let main_before = git_plain(&env.temp_dir, &["rev-parse", "main"])?;
            let result = env
                .hitch
                .run()
                .args(&["--json", "release", "dev", "main"])
                .with_yes(false)
                .execute()?;

            let stderr = result.stderr();
            result.assert_exit_code(1).assert_stdout_is_empty();
            assert!(stderr.contains("--yes"), "{stderr}");

            assert_eq!(
                git_plain(&env.temp_dir, &["rev-parse", "main"])?,
                main_before,
                "refusing to prompt must not have merged anything into the target"
            );
            assert!(
                git_plain(&env.temp_dir, &["tag", "--list"])
                    .unwrap_or_default()
                    .trim()
                    .is_empty(),
                "a refused release must not have created its tag"
            );
            assert!(
                !all_refs(env)?
                    .iter()
                    .any(|r| r.starts_with("refs/hitch/release/")),
                "the refusal path leaked refs/hitch/release/*. Nothing prunes that \
                 family, so this is one leaked ref per refusal."
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A plan that lists its own temporary scaffolding as a change is describing
    /// a post-state that will not exist.
    ///
    /// The anchor is created and removed by the same operation, so putting it
    /// beside `main   a16a75c → 5bf671e` claims a ref a reader may go looking
    /// for. It is *not* deleted from the model — a plan that hid a write would
    /// be hiding a write — it is given its own labelled position, matched by ref
    /// family and never by its description text.
    #[test]
    fn a_release_plan_separates_its_anchor_from_the_changes_that_survive() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            make_feature(env, "feat-a")?;
            declare_branches(env, "dev", &["feat-a"])?;

            let plan = release_plan(env, "dev", "main", ReleasePlanOptions::default(), false)?;
            let rendered = hitch::core::render::render_plan(&plan);
            discard_release_plan(&context_for(env, false)?, &plan);

            let (before_anchor, after_anchor) = rendered
                .split_once("Held only until the publish lands")
                .unwrap_or_else(|| {
                    panic!(
                        "a plan that anchors must say so somewhere:
{rendered}"
                    )
                });
            assert!(
                !before_anchor.contains("release/main/"),
                "the anchor must not appear among the surviving changes:
{before_anchor}"
            );
            assert!(
                after_anchor.contains("release/main/"),
                "and the position it is given must still name it: {after_anchor}"
            );
            // The build record is a surviving, user-meaningful write, so the
            // same separation must not swallow it. A predicate on
            // `refs/hitch/` would have.
            assert!(
                !rendered.contains("state/dev"),
                "a release writes no build record, so its absence here is the \
                 control for the assertion above: {rendered}"
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}
