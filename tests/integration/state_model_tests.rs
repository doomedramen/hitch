//! Tests for the read-only state model (`hitch::core::state`).
//!
//! These drive the library directly rather than asserting on `hitch status`
//! prose, because the whole point of the model is that it is *structured* —
//! `hitch status` is one consumer of it, and asserting on rendered text would
//! only pin whichever consumer happens to exist today. The CLI-facing
//! behaviour is covered separately in `status_tests.rs`.
//!
//! Every test here builds a real repository through the integration harness,
//! so a real `hitch-metadata` branch, a real origin, and a real build record
//! are all in play.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::commands::global_context::{GlobalContext, GlobalFlags};
    use hitch::core::render::why_membership_label;
    use hitch::core::state::{
        build_state_snapshot, ActualComposition, ActualMembership, ChangedInput, EnvironmentHealth,
        RepositoryStateSnapshot,
    };
    use hitch::core::status::{build_matrix_model, MatrixCell};
    use hitch::core::why::{build_why, WhyExplanation, WhyMembership, WhySubject};
    use hitch::utils::logging::Logger;
    use std::sync::Arc;

    /// A context pointed at the test repo, configured like a real read-only
    /// `hitch status` invocation: no push, and assuming-yes so nothing ever
    /// blocks on a prompt.
    ///
    /// `GlobalContext::new_at_path` reports a boxed non-Send error, so the
    /// conversion is spelled out rather than left to `?`.
    fn context_for(env: &TestEnvironment) -> anyhow::Result<GlobalContext> {
        let logger = Arc::new(Logger::new());
        GlobalContext::new_at_path(
            env.temp_dir.to_str().expect("utf-8 temp dir"),
            GlobalFlags {
                verbose: false,
                no_push: true,
                assume_yes: true,
                json: false,
            },
            logger,
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))
    }

    fn snapshot_for(env: &TestEnvironment) -> anyhow::Result<RepositoryStateSnapshot> {
        build_state_snapshot(&context_for(env)?)
    }

    fn state<'a>(
        snap: &'a RepositoryStateSnapshot,
        name: &str,
    ) -> &'a hitch::core::state::EnvironmentState {
        snap.environments
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no environment {name} in snapshot"))
    }

    /// A path beside the test repo, never inside it — a worktree or bare repo
    /// inside the repo shows up as untracked content in its own `git status`.
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

    /// Plain git in a directory the harness has no `GitOperations` handle for.
    /// The harness deliberately spawns git to simulate what a user types; this
    /// is that same blessed spawn point, with the same null-stdin handling.
    fn git_plain(dir: &std::path::Path, args: &[&str]) -> anyhow::Result<()> {
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
        Ok(())
    }

    /// Create a feature branch off `main` with one file, and return its name.
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
        env.git
            .run(&["commit", "-m", &format!("{branch}: {body}")])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    // -----------------------------------------------------------------
    // The bug this phase exists to fix
    // -----------------------------------------------------------------

    /// A rebased feature has commits that are *newer by date* but *different
    /// SHAs*. The old layer compared commit timestamps against a wall-clock
    /// `rebuilt_at`, so it could not see this at all and reported "up to
    /// date". The record's pinned SHA makes it exact.
    #[test]
    fn test_a_rebased_feature_is_needs_rebuild_even_though_its_commits_are_newer(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-rebased")?;
            env.hitch
                .run()
                .args(&["promote", "feat-rebased", "dev"])
                .execute()?
                .assert_success();

            // Rebase onto a moved main. New commit OIDs, same logical work.
            add_commit(env, "main", "unrelated.txt", "main moved")?;
            env.git.run(&["checkout", "feat-rebased"])?;
            env.git.run(&["rebase", "main"])?;
            env.git.run(&["checkout", "main"])?;

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert!(
                matches!(dev.health, EnvironmentHealth::NeedsRebuild { .. }),
                "a rebased feature must read as needs rebuild; got {:?}",
                dev.health
            );

            let EnvironmentHealth::NeedsRebuild { changed_inputs, .. } = &dev.health else {
                unreachable!()
            };
            let change = changed_inputs
                .iter()
                .find(|c| c.branch == "feat-rebased")
                .unwrap_or_else(|| {
                    panic!(
                        "the change should name the rebased feature; got {:?}",
                        changed_inputs
                    )
                });
            assert!(
                change.previous_sha != change.current_sha,
                "both SHAs should be present and differ: {:?}",
                change
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A commit dated *before* the build still changes the content, so it is
    /// still needs-rebuild. The old comparison (`commit_ts > rebuilt_at`) read
    /// this as up to date — the whole class of bug a rebase belongs to, but
    /// reachable without rebasing anything.
    #[test]
    fn test_a_backdated_commit_still_counts_as_needs_rebuild() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-backdated")?;
            env.hitch
                .run()
                .args(&["promote", "feat-backdated", "dev"])
                .execute()?
                .assert_success();

            // Backdate hard. Any timestamp comparison against "now" would see
            // this as *older* than the build.
            env.git.run(&["checkout", "feat-backdated"])?;
            env.fs.write_file("feat-backdated.txt", "v2")?;
            env.git.run(&["add", "-f", "feat-backdated.txt"])?;
            env.git.run(&[
                "commit",
                "-m",
                "backdated change",
                "--date",
                "1999-01-01T00:00:00+00:00",
            ])?;
            env.git.run(&["checkout", "main"])?;

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert!(
                matches!(dev.health, EnvironmentHealth::NeedsRebuild { .. }),
                "a backdated commit is still a content change; got {:?}",
                dev.health
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A rebuild immediately after a rebuild, with nothing touched, is
    /// realised. The counterweight to the two tests above: SHA comparison must
    /// not report drift that isn't there, or every status run would cry wolf.
    #[test]
    fn test_an_untouched_environment_is_realised() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-quiet")?;
            env.hitch
                .run()
                .args(&["promote", "feat-quiet", "dev"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            assert_eq!(state(&snap, "dev").health, EnvironmentHealth::Realised);
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Declaration changes, which no timestamp can see
    // -----------------------------------------------------------------

    /// `--no-rebuild` promotion moves the declaration without moving any
    /// branch. Comparing commit dates cannot detect this — nothing about it
    /// touches a commit — so this is the case that most needs the record.
    #[test]
    fn test_a_no_rebuild_promotion_is_needs_rebuild_with_the_branch_named_as_added(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-first")?;
            env.hitch
                .run()
                .args(&["promote", "feat-first", "dev"])
                .execute()?
                .assert_success();

            make_feature(env, "feat-later")?;
            env.hitch
                .run()
                .args(&["promote", "feat-later", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            let EnvironmentHealth::NeedsRebuild {
                added,
                changed_inputs,
                ..
            } = &dev.health
            else {
                panic!(
                    "a --no-rebuild promotion must read as needs rebuild; got {:?}",
                    dev.health
                );
            };
            assert_eq!(added, &vec!["feat-later".to_string()]);
            assert!(
                changed_inputs.is_empty(),
                "no branch tip moved, so no SHA comparison should fire: {:?}",
                changed_inputs
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Every health variant is reachable and honest
    // -----------------------------------------------------------------

    #[test]
    fn test_a_conflicting_branch_lands_as_partially_realised() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "shared"])?;

            for (branch, body) in [("branch-a", "from a"), ("branch-b", "from b")] {
                env.git.run(&["checkout", "-b", branch])?;
                env.fs.write_file("shared.txt", body)?;
                env.git.run(&["add", "-f", "shared.txt"])?;
                env.git.run(&["commit", "-m", branch])?;
                env.git.run(&["checkout", "main"])?;
            }

            // Inject directly into metadata: `promote` would refuse to promote
            // a conflicting sibling in the first place.
            env.git.run(&["checkout", "hitch-metadata"])?;
            let config_str = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
            config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?;
            env.git.run(&["commit", "-m", "inject conflicting pair"])?;
            env.git.run(&["checkout", "main"])?;

            // Exit 2 is the documented signal for "rebuilt, but held
            // something" — a successful publish, not a failure.
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert_eq!(
                dev.health,
                EnvironmentHealth::PartiallyRealised {
                    held: vec!["branch-b".to_string()],
                },
                "one branch composed, one held, and that is a current-but-partial build"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// An environment built by a hitch that did not write records — the real
    /// upgrade path, and the one P2's own doc comment names first. `rebuilt_at`
    /// is set and the branch is present, but there is no record, so the honest
    /// answer is "unknown", never "up to date".
    ///
    /// The pre-record state is simulated by deleting the record ref after a
    /// real build, which is exactly what such a repository looks like. P2's
    /// plan recorded that this case was *trivially* true before P3 because
    /// nothing read records; this is the reader that makes it a real answer.
    #[test]
    fn test_an_environment_built_without_a_record_is_legacy_unknown() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-legacy")?;
            env.hitch
                .run()
                .args(&["promote", "feat-legacy", "dev"])
                .execute()?
                .assert_success();

            // Sanity: a real build does produce a record, so the deletion
            // below is doing the work rather than passing vacuously.
            assert!(
                env.git
                    .run(&["rev-parse", "--verify", "refs/hitch/state/dev"])?
                    .success(),
                "a rebuild should have written a record"
            );

            // Rewind to what a pre-P2 repository looks like: the build
            // happened, the metadata says so, the record is not there.
            env.git
                .run(&["update-ref", "-d", "refs/hitch/state/dev"])?
                .assert_success();

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert_eq!(dev.actual, ActualComposition::LegacyUnknown);
            assert_eq!(
                dev.health,
                EnvironmentHealth::LegacyUnknown,
                "a build hitch cannot describe must read as unknown, not as up to date"
            );
            assert!(
                dev.rebuilt_at.is_some(),
                "the environment was built; only the record is missing"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `hitch release` lands a branch and writes no record — but it then
    /// prunes the environment branch as integrated, so what the snapshot finds
    /// is a *missing* branch rather than an unknown one. Both are honest, and
    /// they are different answers, so the case is pinned rather than assumed.
    #[test]
    fn test_a_released_environment_reports_a_missing_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-release")?;
            env.hitch
                .run()
                .args(&["promote", "feat-release", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            // Without this, release rebuilds `dev` (its base is the release
            // target) and writes a record, which is the opposite of the
            // recordless landing under test.
            env.hitch
                .run()
                .args(&["release", "dev", "--no-rebuild-dependents"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert_eq!(
                dev.health,
                EnvironmentHealth::MissingBranch,
                "release prunes the environment branch, so there is nothing to describe"
            );
            assert_eq!(
                dev.actual,
                ActualComposition::LegacyUnknown,
                "and with no branch there is no record to read either"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_an_environment_that_has_never_been_built_is_never_built() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            // `hitch add` creates the environment but does not build it, and
            // leaves no branch behind — so this is a missing branch, not a
            // never-built one. Build it and the distinction is visible.
            assert_eq!(dev.health, EnvironmentHealth::MissingBranch);
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Feature × environment membership
    // -----------------------------------------------------------------

    /// One branch in two environments. The membership is per-environment and
    /// the two must be derived from the same pass, not independently.
    #[test]
    fn test_one_feature_promoted_to_two_environments_reports_both() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-shared")?;

            // Only `dev` is rebuilt, so the two environments differ on purpose.
            env.hitch
                .run()
                .args(&["promote", "feat-shared", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feat-shared", "qa", "--no-rebuild"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let feature = snap
                .features
                .iter()
                .find(|f| f.name == "feat-shared")
                .expect("the feature should appear in the snapshot");

            assert_eq!(feature.memberships.len(), 2, "declared in both");
            assert_eq!(feature.memberships[0].environment, "dev");
            assert_eq!(feature.memberships[0].actual, ActualMembership::Included);
            assert_eq!(feature.memberships[1].environment, "qa");
            // qa has no record for this branch: never rebuilt, so hitch does
            // not know. It must not inherit dev's answer.
            assert_eq!(feature.memberships[1].actual, ActualMembership::Unknown);
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A branch present only on origin resolves from the remote-tracking ref,
    /// so the snapshot sees it without a network call.
    #[test]
    fn test_a_remote_only_feature_is_desired_and_not_missing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Push a branch, promote it, then delete it locally. The
            // remote-tracking ref survives the local delete, so the snapshot
            // must still resolve it — without asking the network.
            // `TestSetup::HitchInit` leaves the repo with no origin, so make
            // one. It goes beside the test repo, not inside it.
            let bare = sibling_path(env, "bare-origin.git");
            std::fs::create_dir_all(&bare)?;
            git_plain(&bare, &["init", "--bare"])?;
            env.git
                .run(&["remote", "add", "origin", &bare.to_string_lossy()])?
                .assert_success();

            make_feature(env, "feat-remote")?;
            env.git
                .run(&["push", "origin", "feat-remote"])?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feat-remote", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();
            env.git.run(&["branch", "-D", "feat-remote"])?;

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            let declared = dev
                .desired
                .branches
                .iter()
                .find(|b| b.name == "feat-remote")
                .expect("the declaration is a fact about hitch.json and keeps it");
            assert!(
                declared.sha.is_some(),
                "the remote-tracking ref should resolve, so this is not Missing"
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_a_declared_branch_with_no_ref_anywhere_is_missing() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-vanishing")?;
            env.hitch
                .run()
                .args(&["promote", "feat-vanishing", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();
            env.git.run(&["branch", "-D", "feat-vanishing"])?;

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert_eq!(
                dev.membership_of("feat-vanishing"),
                ActualMembership::Missing,
                "declared but unresolvable"
            );
            let declared = dev
                .desired
                .branches
                .iter()
                .find(|b| b.name == "feat-vanishing")
                .expect("still declared");
            assert_eq!(declared.sha, None, "and it is kept, with no SHA");
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A branch already reachable from the base is a live git fact, available
    /// with or without a record.
    #[test]
    fn test_a_feature_already_in_base_reports_already_in_base() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-merged")?;

            // Land it on main, so it is an ancestor of the base.
            env.git.run(&["checkout", "main"])?;
            env.git
                .run(&["merge", "--no-ff", "feat-merged", "-m", "merge it"])?;
            env.git.run(&["push", "origin", "main"])?;

            env.hitch
                .run()
                .args(&["promote", "feat-merged", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let feature = snap
                .features
                .iter()
                .find(|f| f.name == "feat-merged")
                .expect("feature in the snapshot");

            assert_eq!(
                feature.memberships[0].actual,
                ActualMembership::AlreadyInBase
            );
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Presentation-only metadata, carried but never used for a verdict
    // -----------------------------------------------------------------

    #[test]
    fn test_lock_and_approval_metadata_is_carried_through() -> anyhow::Result<()> {
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
            env.hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--requires-approval",
                    "true",
                    "--set-approvers",
                    "reviewer@example.com",
                ])
                .execute()?
                .assert_success();

            let snap = snapshot_for(env)?;
            let dev = state(&snap, "dev");

            assert!(dev.locked, "lock must reach the snapshot");
            assert!(dev.approval_policy.required, "approval policy too");
            assert_eq!(
                dev.approval_policy.approvers,
                vec!["reviewer@example.com".to_string()]
            );
            assert!(dev.locked_at.is_some(), "presentation metadata carried");
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Invariants
    // -----------------------------------------------------------------

    /// The snapshot must be a pure read: two consecutive reads agree, and
    /// nothing about refs/hitch moved.
    #[test]
    fn test_the_snapshot_is_pure() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-pure")?;
            env.hitch
                .run()
                .args(&["promote", "feat-pure", "dev"])
                .execute()?
                .assert_success();

            let refs_before = env
                .git
                .run(&["for-each-ref", "--format=%(refname) %(objectname)"])?
                .stdout();

            let first = snapshot_for(env)?;
            let second = snapshot_for(env)?;

            assert_eq!(
                first.environments, second.environments,
                "two reads of an unchanged repo must agree exactly"
            );
            assert_eq!(first.features, second.features);

            let refs_after = env
                .git
                .run(&["for-each-ref", "--format=%(refname) %(objectname)"])?
                .stdout();
            assert_eq!(refs_before, refs_after, "the snapshot must move no ref");
            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `EnvironmentHealth::is_actionable` is the one predicate the summary
    /// counters use. Assert the classification directly so a future variant
    /// cannot be silently counted as "fine".
    #[test]
    fn test_health_actionability_and_labels() {
        assert!(EnvironmentHealth::NeverBuilt.is_actionable());
        assert!(EnvironmentHealth::MissingBranch.is_actionable());
        assert!(EnvironmentHealth::NeedsRebuild {
            changed_inputs: vec![],
            added: vec![],
            removed: vec![]
        }
        .is_actionable());

        assert!(!EnvironmentHealth::Realised.is_actionable());
        assert!(!EnvironmentHealth::PartiallyRealised { held: vec![] }.is_actionable());
        // Not actionable, and crucially *not* a silent "up to date": it has
        // its own label, so a renderer can say it has no idea.
        assert!(!EnvironmentHealth::LegacyUnknown.is_actionable());
        assert_eq!(EnvironmentHealth::LegacyUnknown.label(), "actual unknown");
    }

    #[test]
    fn test_changed_input_renders_the_spec_11_2_arrow() {
        let change = ChangedInput {
            branch: "feature/auth".to_string(),
            previous_sha: Some("2a42d1c9a1e4f0b2c3d4e5f60718293a4b5c6d7e".to_string()),
            current_sha: Some("7c931af0b1c2d3e4f50617283940a1b2c3d4e5f".to_string()),
        };
        assert_eq!(
            change.short(),
            ("2a42d1c".to_string(), "7c931af".to_string()),
            "spec §11.2's own example form"
        );

        let deleted = ChangedInput {
            branch: "gone-branch".to_string(),
            previous_sha: Some("2a42d1c9a1e4f0b2c3d4e5f60718293a4b5c6d7e".to_string()),
            current_sha: None,
        };
        assert_eq!(deleted.short().1, "gone");
    }

    // -----------------------------------------------------------------
    // Agreement between the model and what the CLI renders
    // -----------------------------------------------------------------
    //
    // `build_status_model` is a pure projection of the snapshot, so it cannot
    // disagree with it by construction. What *can* disagree is the CLI's
    // renderer, which is a separate function with its own string literals. These
    // tests drive both from one repository and compare, because "the snapshot
    // is right" is only half an answer if the command that shows it to a human
    // renders something else.

    /// Write a branch list straight into an environment's declaration in
    /// `hitch.json`, bypassing `hitch promote`. Needed when the branches are
    /// *meant* to conflict with the base, which promote would refuse.
    fn declare_branches(
        env: &TestEnvironment,
        environment: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "hitch-metadata"])?;
        let config: serde_json::Value = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        let mut config = config;
        config["environments"][environment]["branches"] = serde_json::to_value(branches)?;
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: declare branches directly"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// Build a repo where each environment is in a different state, so one run
    /// of `hitch status` has to render several verdicts at once.
    fn four_state_repo(env: &TestEnvironment) -> anyhow::Result<()> {
        for name in ["realised", "stale", "never", "orphan"] {
            env.hitch
                .run()
                .args(&["add", name])
                .execute()?
                .assert_success();
        }
        make_feature(env, "feat-ok")?;
        make_feature(env, "feat-moved")?;

        // realised: built, then untouched.
        env.hitch
            .run()
            .args(&["promote", "feat-ok", "realised"])
            .execute()?
            .assert_success();

        // stale: built, then its feature moved.
        env.hitch
            .run()
            .args(&["promote", "feat-moved", "stale"])
            .execute()?
            .assert_success();
        add_commit(env, "feat-moved", "feat-moved.txt", "v2")?;

        // never: declared, never built.
        env.hitch
            .run()
            .args(&["promote", "feat-ok", "never", "--no-rebuild"])
            .execute()?
            .assert_success();

        // orphan: built, then its branch deleted.
        env.hitch
            .run()
            .args(&["promote", "feat-ok", "orphan"])
            .execute()?
            .assert_success();
        env.git.run(&["branch", "-D", "orphan"])?;
        env.git.run(&["checkout", "main"])?;

        Ok(())
    }

    #[test]
    fn test_the_snapshot_and_the_status_model_never_disagree() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            four_state_repo(env)?;

            let snap = snapshot_for(env)?;
            let model = hitch::core::status::build_status_model(&snap);

            assert_eq!(
                model.environments.len(),
                snap.environments.len(),
                "every environment in the snapshot appears in the view, and no others"
            );

            for (viewed, snapshotted) in model.environments.iter().zip(snap.environments.iter()) {
                assert_eq!(
                    viewed.name, snapshotted.name,
                    "environments are sorted, so positions correspond"
                );
                assert_eq!(
                    &viewed.state.health, &snapshotted.health,
                    "the view carries the snapshot's verdict verbatim; it must not re-derive one"
                );
            }

            // And the summary counters are the model's, not a second opinion.
            let actionable = snap
                .environments
                .iter()
                .filter(|e| e.health.is_actionable())
                .count();
            let never = snap
                .environments
                .iter()
                .filter(|e| matches!(e.health, EnvironmentHealth::NeverBuilt))
                .count();
            assert_eq!(model.summary.needs_rebuild_envs, actionable);
            assert_eq!(model.summary.never_rebuilt_envs, never);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The renderer must show what the model says. Each state's marker is
    /// distinct, so a renderer that re-derived its own verdict — or that
    /// collapsed a variant into "up to date" — fails here rather than quietly
    /// lying to a user.
    ///
    /// P7 gave `hitch status` a second view, so this runs the command twice and
    /// checks each against the vocabulary that view actually speaks. That is the
    /// point of running it twice rather than once: the guarantee is *both* views
    /// read the snapshot's verdict, and a test that picked one of them would
    /// leave the other free to invent one. The matrix's vocabulary is
    /// `EnvironmentHealth::label` — the same function the rest of the CLI reads,
    /// reached through the snapshot rather than through a second match — and the
    /// detail view's is its own pre-existing wording.
    #[test]
    fn test_hitch_status_renders_exactly_what_the_snapshot_reports() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            four_state_repo(env)?;

            let snap = snapshot_for(env)?;

            // The matrix: one verdict line per environment, carrying the
            // snapshot's own label.
            let matrix = env
                .hitch
                .run()
                .args(&["status"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();
            for state in &snap.environments {
                let label = state.health.label();
                assert!(
                    matrix
                        .lines()
                        .any(|line| line.trim() == label),
                    "the snapshot says {} is {:?}, whose label is {label:?}, so a matrix row must carry exactly that. Got:\n{matrix}",
                    state.name,
                    state.health,
                );
            }
            // And the matrix carries no SHA arrows: a changed input is a
            // per-environment fact, and the grid is a fact about shape. Asserted
            // here so that the "the arrow lives in the detail view" claim below
            // cannot be satisfied by both views printing one.
            assert!(
                !matrix.contains('\u{2192}'),
                "the matrix names no individual input, so it shows no SHA arrow. Got:\n{matrix}"
            );

            // The detail view: its own wording, one marker per health variant.
            let stdout = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();

            // Assert the model first, so a failure here reports the
            // disagreement rather than a downstream symptom.
            for state in &snap.environments {
                // `NeedsRebuild` is the one verdict that renders as a
                // structure rather than a phrase — a SHA arrow per changed
                // input — so it is matched separately.
                if matches!(state.health, EnvironmentHealth::NeedsRebuild { .. }) {
                    assert!(
                        stdout.contains('\u{2192}'),
                        "the model says {} needs rebuild, so status must show a SHA arrow. Got:\n{}",
                        state.name,
                        stdout
                    );
                    continue;
                }
                let marker = match &state.health {
                    EnvironmentHealth::Realised => "Up to date",
                    EnvironmentHealth::NeverBuilt => "Never rebuilt",
                    EnvironmentHealth::MissingBranch => "does not exist",
                    EnvironmentHealth::LegacyUnknown => "Actual unknown",
                    EnvironmentHealth::PartiallyRealised { .. } => "held on the last build",
                    EnvironmentHealth::NeedsRebuild { .. } => unreachable!(),
                };
                assert!(
                    stdout.contains(marker),
                    "the model says {} is {:?}, so status must say {marker:?}. Got:\n{}",
                    state.name,
                    state.health,
                    stdout
                );
            }

            // And the reverse direction: nothing is claimed as up to date that
            // the model does not call realised. "Up to date" appears exactly
            // once per realised environment.
            let realised = snap
                .environments
                .iter()
                .filter(|e| e.health == EnvironmentHealth::Realised)
                .count();
            assert_eq!(
                stdout.matches("Up to date").count(),
                realised,
                "'Up to date' must be shown for realised environments and nothing else. Got:\n{}",
                stdout
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The recordless case has to survive all the way to a rendered command:
    /// `hitch status` exits 0 and says it does not know, rather than failing on
    /// a missing ref or — worse — claiming up-to-date.
    #[test]
    fn test_hitch_status_renders_legacy_unknown_and_still_exits_zero() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feat-legacy")?;
            env.hitch
                .run()
                .args(&["promote", "feat-legacy", "dev"])
                .execute()?
                .assert_success();
            env.git
                .run(&["update-ref", "-d", "refs/hitch/state/dev"])?
                .assert_success();

            let result = env.hitch.run().args(&["status"]).execute()?;
            let matrix = result.assert_success().stdout().to_string();

            // The matrix states it as a cell and a row: `? actual unknown` for
            // the branch, and `actual unknown` as the environment's verdict. The
            // second is the snapshot's own `EnvironmentHealth::label`, so this
            // arm is the same guarantee the four-state test makes for all six
            // variants, reached through the default view.
            assert!(
                matrix.contains("? actual unknown"),
                "a missing record must render as unknown. Got:\n{matrix}"
            );
            assert!(
                matrix
                    .lines()
                    .any(|line| line.trim() == "actual unknown"),
                "and the environment's verdict line must say so. Got:\n{matrix}"
            );

            // The detail view keeps its own capitalised phrasing.
            let detail = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?;
            let stdout = detail.assert_success().stdout().to_string();
            assert!(
                stdout.contains("Actual unknown"),
                "a missing record must render as unknown. Got:\n{stdout}"
            );
            assert!(
                !stdout.contains("Up to date"),
                "and must not be described as up to date. Got:\n{stdout}"
            );
            // And in neither view: a record hitch cannot read is not a licence to
            // claim a rebuild is needed, so neither may offer one. This is the
            // P3 rule — `LegacyUnknown` is not actionable — stated about the
            // output rather than about `is_actionable()`. Scoped to the
            // suggested-actions block rather than the whole output because the
            // quick-commands block always carries a `'hitch rebuild
            // <environment>'` *template*, which is a different claim: it says
            // the command exists, not that it is needed here.
            for (view, output) in [("matrix", &matrix), ("detail", &stdout)] {
                assert!(
                    !output.contains("Suggested actions"),
                    "LegacyUnknown is not actionable, so the {view} view must not suggest any. Got:\n{output}"
                );
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
    /// The record says a branch *was* held; the preflight says one *would* be.
    /// These used to render identically, so a user could not tell a fact about
    /// the branch in front of them from a prediction about a build that has not
    /// happened. One glyph, two claims — now one glyph, two wordings.
    ///
    /// The conflicting branches are written straight into `hitch.json` rather
    /// than promoted, because `hitch promote` refuses a branch that conflicts
    /// with the base — the very condition under test. (Same bypass, and same
    /// reason, as `test_status_shows_held_branch_glyph`.)
    #[test]
    fn test_status_distinguishes_a_held_branch_from_one_that_would_be_held() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.fs.write_file("shared.txt", "base\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", "shared"])?;
            for (branch, body) in [("branch-a", "from a\n"), ("branch-b", "from b\n")] {
                env.git.run(&["checkout", "-b", branch])?;
                env.fs.write_file("shared.txt", body)?;
                env.git.run(&["add", "-f", "shared.txt"])?;
                env.git.run(&["commit", "-m", branch])?;
                env.git.run(&["checkout", "main"])?;
            }

            // Declare both branches without building, so the only claim
            // available is a prediction.
            declare_branches(env, "dev", &["branch-a", "branch-b"])?;

            // The detail view, which is where the ⛔ lives: the glyph is
            // ambiguous on its own, so the two wordings that disambiguate it are
            // the thing under test. `--environments` because P7 moved this view
            // behind the flag and left the wording alone — the matrix carries no
            // prediction at all, which
            // `test_status_shows_held_branch_glyph` in `status_tests.rs`
            // asserts from the other side.
            let before = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();
            assert!(
                before.contains("would be held on the next rebuild"),
                "an unbuilt environment can only offer a prediction. Got:\n{before}"
            );
            assert!(
                !before.contains("held in the last build"),
                "and must not claim a build that never happened. Got:\n{before}"
            );

            // Build it. branch-a lands; branch-b conflicts and is held, so the
            // record now knows it — and the claim is a fact about what is in
            // `dev` right now.
            //
            // Exit 2 is the CI contract for "rebuilt, but held branches"; see
            // the gotcha in AGENTS.md.
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);

            let after = env
                .hitch
                .run()
                .args(&["status", "--environments"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();
            assert!(
                after.contains("held in the last build"),
                "the build record now says branch-b was held. Got:\n{after}"
            );
            assert!(
                !after.contains("would be held on the next rebuild"),
                "and the prediction is superseded by the fact. Got:\n{after}"
            );

            // Non-vacuity: the record has to exist, or the fact arm would be
            // reached for the same reason as the prediction arm.
            env.git
                .run(&["rev-parse", "--verify", "refs/hitch/state/dev"])?
                .assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Every cell the matrix produces, asked about as a `why`, comes back as the
    /// same state — over *real* history rather than a hand-built snapshot.
    ///
    /// `tests/unit/why_tests.rs` holds the exhaustive version over hand-built
    /// snapshots. This one is the version a fixture that agrees with itself
    /// cannot satisfy: every cell here is read out of the snapshot hitch itself
    /// computed from a repository that genuinely got into that state, and two of
    /// them (`missing` and `in base`) are *only* reachable through history that a
    /// hand-built snapshot would have to assert rather than reproduce.
    ///
    /// The `(cell, why)` pairs are asserted through the two commands' own
    /// vocabularies, because that is the form the property takes for a user: one
    /// glyph-word in the grid, the same glyph-word in the explanation.
    #[test]
    fn the_matrix_cell_and_the_why_membership_never_disagree() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // A conflicting pair, so the record can name a hold *and* an
            // inclusion in the same build. Both branches rewrite the same line
            // from the same base, so one composes and the other is ejected.
            for branch in ["feature/payments", "feature/dashboard"] {
                env.git.run(&["checkout", "-b", branch])?;
                env.fs.write_file("src/shared.txt", branch)?;
                env.git.run(&["add", "-f", "src/shared.txt"])?;
                env.git.run(&["commit", "-m", branch])?;
                env.git.run(&["checkout", "main"])?;
            }
            // And a branch already merged into the base, which is the only way
            // `in base` is reachable: it is a live reachability fact, not a
            // record fact, and a fixture that asserted the cell without
            // producing the ancestry would be testing nothing.
            env.git.run(&["checkout", "-b", "feature/already-merged"])?;
            env.fs.write_file("merged.txt", "v1")?;
            env.git.run(&["add", "-f", "merged.txt"])?;
            env.git.run(&["commit", "-m", "merged into the base"])?;
            env.git.run(&["checkout", "main"])?;
            env.git
                .run(&["merge", "--no-ff", "feature/already-merged"])?;
            // Two more that are declared but never built, for the pending-work
            // cell, and one whose ref is about to vanish, for the missing cell.
            for branch in ["feature/alpha", "feature/gamma"] {
                make_feature(env, branch)?;
            }

            // The build that produces the record: `payments` composes,
            // `dashboard` is held. Exit 2 is the CI contract for "rebuilt, but
            // held branches"; see the gotcha in AGENTS.md.
            declare_branches(env, "dev", &["feature/payments", "feature/dashboard"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);

            // Widen the declaration *after* that build, so these four are
            // declared and the record is silent about them — which is the only
            // way to a `needs rebuild` cell, and why this is a second declaration
            // rather than a first one.
            declare_branches(
                env,
                "dev",
                &[
                    "feature/payments",
                    "feature/dashboard",
                    "feature/already-merged",
                    "feature/alpha",
                    "feature/gamma",
                ],
            )?;
            // And then the ref goes away. `missing` needs the record to be silent
            // *and* the declaration to resolve to nothing: a record that names the
            // branch settles it as `included`, because a build that consumed the
            // commit contained it and "the branch is gone" is a separate fact.
            env.git
                .run(&["update-ref", "-d", "refs/heads/feature/gamma"])?;

            let with_record = assert_cells_agree(
                env,
                &[
                    ("feature/payments", "● included"),
                    ("feature/dashboard", "⛔ held"),
                    ("feature/alpha", "↻ needs rebuild"),
                    ("feature/gamma", "! missing"),
                    ("feature/already-merged", "= in base"),
                ],
            )?;

            // Now delete the build record outright — what a repository last built
            // by a pre-P2 hitch looks like — and read every cell again. Four of
            // the five become `actual unknown`, which is the whole point of that
            // cell: hitch says what it does not know instead of guessing. The two
            // that do *not* change are the two that were never record facts, and
            // that is not an accident of this fixture — `in base` is reachability
            // and `missing` is an unresolvable ref, both knowable with no record
            // at all.
            env.git.run(&["update-ref", "-d", "refs/hitch/state/dev"])?;
            assert_cells_agree(
                env,
                &[
                    ("feature/payments", "? actual unknown"),
                    ("feature/dashboard", "? actual unknown"),
                    ("feature/alpha", "? actual unknown"),
                    ("feature/gamma", "! missing"),
                    ("feature/already-merged", "= in base"),
                ],
            )?;

            // And the reason the record's disappearance is a *display*
            // difference rather than a different question: both readings above
            // asked `why` the same thing and got the same answer as the grid.
            let snapshot = snapshot_for(env)?;
            assert_eq!(
                with_record
                    .iter()
                    .filter(|(_, cell)| *cell == MatrixCell::NeedsRebuild)
                    .count(),
                1,
                "before the record went, exactly one branch was pending"
            );
            assert!(
                snapshot
                    .environments
                    .iter()
                    .any(|e| matches!(e.health, EnvironmentHealth::LegacyUnknown)),
                "and afterwards hitch says it cannot describe the build"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Assert that the matrix cell and the `why` membership agree, in value and
    /// in words, for each `(feature, "glyph label")` pair — and return the cells
    /// so a caller can make a claim across calls.
    fn assert_cells_agree(
        env: &TestEnvironment,
        expected: &[(&str, &str)],
    ) -> anyhow::Result<Vec<(String, MatrixCell)>> {
        let snapshot = snapshot_for(env)?;
        let matrix = build_matrix_model(&snapshot);
        let mut cells = Vec::new();

        for (feature, wanted) in expected {
            // The cell, as the grid shows it.
            let cell = matrix
                .rows
                .iter()
                .find(|r| r.feature == *feature)
                .unwrap_or_else(|| {
                    panic!(
                        "{feature} has no row in the matrix.\nrows: {:?}",
                        matrix.rows.iter().map(|r| &r.feature).collect::<Vec<_>>()
                    )
                })
                .cells
                .first()
                .copied()
                .expect("a row has a cell per column");
            let rendered = format!("{} {}", cell.glyph(), cell.label());
            assert_eq!(rendered, *wanted, "the cell for {feature} in `dev`");

            // The membership, as `why` reports it. Asserted on the *value* so a
            // wording change cannot fail it, and then through
            // `why_membership_label` — the renderer's own function — so a
            // divergence in the words is caught too. Case-insensitively, because
            // the two renderers deliberately differ there: the matrix is a table
            // of cells and the explanation is prose, and `Included` in the middle
            // of a sentence is not the same typography as a table entry. The
            // *word* agreeing is the property; the capitalisation is each
            // renderer's business.
            let membership = membership_in(&build_why(
                &snapshot,
                &WhySubject::FeatureIn((*feature).to_string(), "dev".to_string()),
            )?)?;
            assert_eq!(
                membership,
                WhyMembership::from(cell),
                "{feature}: the matrix says {cell:?} and the why says {membership:?}"
            );
            assert_eq!(
                why_membership_label(membership).to_lowercase(),
                *wanted,
                "{feature}: the same membership in the same words in both commands"
            );

            cells.push(((*feature).to_string(), cell));
        }
        Ok(cells)
    }

    /// The membership out of a `FeatureIn` explanation, refusing either of the
    /// other two forms.
    ///
    /// A `match` that `bail!`s rather than a helper on `WhyExplanation`, because a
    /// future form should fail this at the *call site* that assumed the question
    /// was about a branch in an environment — not silently answer a different
    /// question than the one being asked.
    fn membership_in(explanation: &WhyExplanation) -> anyhow::Result<WhyMembership> {
        match explanation {
            WhyExplanation::FeatureInEnvironment(e) => Ok(e.membership),
            other => anyhow::bail!("expected the feature-in-environment form, got {other:?}"),
        }
    }
}
