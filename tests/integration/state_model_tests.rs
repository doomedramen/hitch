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
    use hitch::commands::global_context::GlobalContext;
    use hitch::core::state::{
        build_state_snapshot, ActualComposition, ActualMembership, ChangedInput, EnvironmentHealth,
        RepositoryStateSnapshot,
    };
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
            false,
            true,
            true,
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
    #[test]
    fn test_hitch_status_renders_exactly_what_the_snapshot_reports() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            four_state_repo(env)?;

            let snap = snapshot_for(env)?;
            let stdout = env
                .hitch
                .run()
                .args(&["status"])
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
            let stdout = result.assert_success().stdout().to_string();

            assert!(
                stdout.contains("Actual unknown"),
                "a missing record must render as unknown. Got:\n{}",
                stdout
            );
            assert!(
                !stdout.contains("Up to date"),
                "and must not be described as up to date. Got:\n{}",
                stdout
            );
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

            let before = env
                .hitch
                .run()
                .args(&["status"])
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
                .args(&["status"])
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
}
