//! Integration tests for `build_activity`, the reader behind `hitch log`.
//!
//! These drive real commands and then call the library on the same repository,
//! because the thing under test is the walk over `hitch-metadata` history, not
//! any rendering of it.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::commands::global_context::{GlobalContext, GlobalFlags};
    use hitch::core::activity::{
        build_activity, ActivityLog, ActivityQuery, HitchEvent, RebuildOutcome,
    };
    use hitch::utils::logging::Logger;
    use std::sync::Arc;

    fn context_for(env: &TestEnvironment) -> anyhow::Result<GlobalContext> {
        GlobalContext::new_at_path(
            env.temp_dir.to_str().expect("utf-8 temp dir"),
            GlobalFlags {
                verbose: false,
                no_push: true,
                assume_yes: true,
                json: false,
            },
            Arc::new(Logger::new()),
        )
        .map_err(|e| anyhow::anyhow!("building a test GlobalContext failed: {e}"))
    }

    fn activity(env: &TestEnvironment, query: &ActivityQuery) -> anyhow::Result<ActivityLog> {
        build_activity(&context_for(env)?, query)
    }

    fn make_feature(env: &TestEnvironment, name: &str) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", name])?;
        env.fs
            .write_file(&format!("{}.txt", name.replace('/', "_")), "v1")?;
        env.git.run(&["add", "-A"])?;
        env.git
            .run(&["commit", "-m", &format!("{name}: initial")])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    fn add_env(env: &TestEnvironment, name: &str) {
        env.hitch
            .run()
            .args(&["add", name])
            .execute()
            .unwrap()
            .assert_success();
    }

    fn promote(env: &TestEnvironment, branch: &str, environment: &str) {
        env.hitch
            .run()
            .args(&["promote", branch, environment, "--no-rebuild"])
            .execute()
            .unwrap()
            .assert_success();
    }

    fn query(limit: usize) -> ActivityQuery {
        ActivityQuery {
            limit,
            ..Default::default()
        }
    }

    #[test]
    fn a_promote_is_one_entry_whose_actor_is_the_committer() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feature/a")?;
            promote(env, "feature/a", "dev");

            let user = env
                .git
                .run(&["config", "user.name"])?
                .stdout()
                .trim()
                .to_string();
            let log = activity(env, &query(50))?;
            let newest = &log.entries[0];
            assert_eq!(newest.actor, user);
            assert_eq!(
                newest.events,
                vec![HitchEvent::Promoted {
                    environment: "dev".into(),
                    branch: "feature/a".into()
                }]
            );
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn entries_are_newest_first_and_limit_counts_entries_not_commits() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            for i in 0..5 {
                let name = format!("feature/{i}");
                make_feature(env, &name)?;
                promote(env, &name, "dev");
            }
            let log = activity(env, &query(2))?;
            assert_eq!(log.entries.len(), 2);
            assert!(log.truncated);
            let promoted_branches: Vec<&str> = log
                .entries
                .iter()
                .flat_map(|e| &e.events)
                .filter_map(|ev| match ev {
                    HitchEvent::Promoted { branch, .. } => Some(branch.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(promoted_branches, vec!["feature/4", "feature/3"]);
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn an_unreadable_historical_config_is_skipped_not_fatal() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feature/a")?;

            env.git.run(&["checkout", "hitch-metadata"])?;
            let good = env.fs.read_file("hitch.json")?;
            env.fs.write_file("hitch.json", "{not json")?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: corrupt"])?;
            let bad = env
                .git
                .run(&["rev-parse", "HEAD"])?
                .stdout()
                .trim()
                .to_string();
            env.fs.write_file("hitch.json", &good)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: restore"])?;
            env.git.run(&["checkout", "main"])?;

            promote(env, "feature/a", "dev");

            let log = activity(env, &query(50))?;
            assert_eq!(log.skipped.len(), 1);
            assert_eq!(log.skipped[0].commit, bad);
            assert!(!log.skipped[0].reason.is_empty());
            assert!(log.entries.iter().any(|e| e.events.iter().any(
                |ev| matches!(ev, HitchEvent::Promoted { branch, .. } if branch == "feature/a")
            )));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_merge_on_hitch_metadata_is_walked_by_first_parent() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");

            env.git.run(&["checkout", "hitch-metadata"])?;
            env.git.run(&["checkout", "-b", "side"])?;
            let mut config: serde_json::Value =
                serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
            config["environments"]["qa"] = config["environments"]["dev"].clone();
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "side: add qa"])?;
            let side = env.git.run(&["rev-parse", "HEAD"])?.stdout().trim().to_string();
            env.git.run(&["checkout", "hitch-metadata"])?;
            env.git
                .run(&["merge", "--no-ff", "-m", "merge side", "side"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?;

            let log = activity(env, &query(50))?;
            let created_qa = log
                .entries
                .iter()
                .flat_map(|e| &e.events)
                .filter(|ev| matches!(ev, HitchEvent::EnvironmentCreated { environment, .. } if environment == "qa"))
                .count();
            assert_eq!(created_qa, 1);
            assert!(log.entries.iter().all(|e| e.commit != side));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn the_environment_filter_excludes_other_environments_events() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            add_env(env, "qa");
            make_feature(env, "feature/a")?;
            make_feature(env, "feature/devtools")?;
            promote(env, "feature/a", "dev");
            promote(env, "feature/devtools", "qa");

            let log = activity(
                env,
                &ActivityQuery {
                    environment: Some("dev".into()),
                    limit: 50,
                    ..Default::default()
                },
            )?;
            let events: Vec<&HitchEvent> = log.entries.iter().flat_map(|e| &e.events).collect();
            assert!(!events.is_empty());
            assert!(events.iter().all(|ev| ev.environment() == "dev"));
            assert!(!events
                .iter()
                .any(|ev| ev.branches().contains(&"feature/devtools")));

            let by_branch = activity(
                env,
                &ActivityQuery {
                    branch: Some("feature/devtools".into()),
                    limit: 50,
                    ..Default::default()
                },
            )?;
            assert!(!by_branch.entries.is_empty());
            assert!(by_branch
                .entries
                .iter()
                .flat_map(|e| &e.events)
                .all(|ev| ev.branches().contains(&"feature/devtools")));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    fn events_of(log: &ActivityLog) -> Vec<&HitchEvent> {
        log.entries.iter().flat_map(|e| &e.events).collect()
    }

    fn lock_events(log: &ActivityLog) -> Vec<&HitchEvent> {
        events_of(log)
            .into_iter()
            .filter(|e| matches!(e, HitchEvent::Locked { .. } | HitchEvent::Unlocked { .. }))
            .collect()
    }

    /// Commits `mutate(config)` onto `hitch-metadata` with a chosen committer
    /// date, bypassing hitch so the config carries no `lock_purpose`.
    fn commit_config(
        env: &TestEnvironment,
        when: &str,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> anyhow::Result<String> {
        env.git.run(&["checkout", "hitch-metadata"])?;
        let mut config: serde_json::Value = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        mutate(&mut config);
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "-f", "hitch.json"])?;
        #[allow(clippy::disallowed_methods)] // git.run cannot set GIT_COMMITTER_DATE
        let out = std::process::Command::new("git")
            .args(["commit", "-m", "test: hand-written"])
            .current_dir(&env.temp_dir)
            .env("GIT_COMMITTER_DATE", when)
            .stdin(std::process::Stdio::null())
            .output()?;
        anyhow::ensure!(out.status.success(), "git commit failed");
        let sha = env
            .git
            .run(&["rev-parse", "HEAD"])?
            .stdout()
            .trim()
            .to_string();
        env.git.run(&["checkout", "main"])?;
        Ok(sha)
    }

    #[test]
    fn a_promote_is_not_bracketed_by_lock_events() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feature/a")?;
            env.hitch
                .run()
                .args(&["promote", "feature/a", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();
            let log = activity(env, &query(50))?;
            assert!(lock_events(&log).is_empty(), "{:?}", log.entries);
            assert!(events_of(&log).iter().any(|ev| matches!(ev,
                HitchEvent::Promoted { branch, .. } if branch == "feature/a")));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_refused_operation_leaves_no_trace_in_the_log() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feature/a")?;
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature/a", "dev", "--no-rebuild"])
                .execute()?
                .assert_failure();
            let log = activity(env, &query(50))?;
            let locks = lock_events(&log);
            assert_eq!(locks.len(), 1, "{locks:?}");
            assert!(
                matches!(locks[0], HitchEvent::Locked { environment, .. } if environment == "dev")
            );
            assert!(!events_of(&log)
                .iter()
                .any(|ev| matches!(ev, HitchEvent::Promoted { .. })));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_manual_lock_and_unlock_are_events() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            for cmd in ["lock", "unlock"] {
                env.hitch
                    .run()
                    .args(&[cmd, "dev"])
                    .execute()?
                    .assert_success();
            }
            let log = activity(env, &query(50))?;
            let locks = lock_events(&log);
            assert_eq!(locks.len(), 2, "{locks:?}");
            assert!(
                matches!(locks[0], HitchEvent::Unlocked { environment } if environment == "dev")
            );
            assert!(
                matches!(locks[1], HitchEvent::Locked { environment, .. } if environment == "dev")
            );
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn legacy_brackets_are_collapsed_by_heuristic() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            let lock = |c: &mut serde_json::Value| {
                c["environments"]["dev"]["locked"] = true.into();
                c["environments"]["dev"]["locked_by"] = "someone@example.com".into();
            };
            let unlock = |c: &mut serde_json::Value| {
                c["environments"]["dev"]["locked"] = false.into();
                c["environments"]["dev"]["locked_by"] = serde_json::Value::Null;
            };
            // A bracket around a promote, seconds apart.
            commit_config(env, "2030-01-01T00:00:00Z", lock)?;
            commit_config(env, "2030-01-01T00:00:01Z", |c| {
                c["environments"]["dev"]["branches"] = serde_json::json!(["feature/a"]);
            })?;
            commit_config(env, "2030-01-01T00:00:02Z", unlock)?;
            let log = activity(env, &query(50))?;
            assert!(lock_events(&log).is_empty(), "{:?}", log.entries);
            assert!(events_of(&log).iter().any(|ev| matches!(ev,
                HitchEvent::Promoted { branch, .. } if branch == "feature/a")));

            // A lock held two hours with nothing in between is a human's.
            commit_config(env, "2030-01-01T05:00:00Z", lock)?;
            commit_config(env, "2030-01-01T07:00:00Z", unlock)?;
            let log = activity(env, &query(50))?;
            let locks = lock_events(&log);
            assert_eq!(locks.len(), 2, "{locks:?}");
            assert!(matches!(locks[0], HitchEvent::Unlocked { .. }));
            assert!(matches!(locks[1], HitchEvent::Locked { .. }));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn an_unreadable_root_does_not_make_the_next_commit_a_phantom_creation() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            // Rewrite history so the root's hitch.json is unreadable while the
            // next commit is fine: an orphan corrupt root, then the good config.
            let good = {
                env.git.run(&["checkout", "hitch-metadata"])?;
                env.fs.read_file("hitch.json")?
            };
            env.git.run(&["checkout", "--orphan", "rewritten"])?;
            env.git.run(&["rm", "-rf", "--cached", "."])?;
            env.fs.write_file("hitch.json", "{not json")?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: corrupt root"])?;
            env.fs.write_file("hitch.json", &good)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: good config"])?;
            env.git
                .run(&["branch", "-f", "hitch-metadata", "rewritten"])?;
            env.git.run(&["checkout", "main"])?;

            let log = activity(env, &query(50))?;
            assert_eq!(log.skipped.len(), 1);
            assert!(
                !events_of(&log)
                    .iter()
                    .any(|ev| matches!(ev, HitchEvent::EnvironmentCreated { .. })),
                "{:?}",
                log.entries
            );
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    fn set_lock(c: &mut serde_json::Value, locked: bool, purpose: Option<&str>) {
        let e = &mut c["environments"]["dev"];
        e["locked"] = locked.into();
        e["locked_by"] = if locked {
            "someone@example.com".into()
        } else {
            serde_json::Value::Null
        };
        e["lock_purpose"] = match (locked, purpose) {
            (true, Some(p)) => p.into(),
            _ => serde_json::Value::Null,
        };
    }

    #[test]
    fn the_legacy_window_is_inclusive_at_sixty_seconds() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            commit_config(env, "2030-01-01T00:00:00Z", |c| set_lock(c, true, None))?;
            commit_config(env, "2030-01-01T00:01:00Z", |c| set_lock(c, false, None))?;
            assert!(lock_events(&activity(env, &query(50))?).is_empty());

            commit_config(env, "2030-01-02T00:00:00Z", |c| set_lock(c, true, None))?;
            commit_config(env, "2030-01-02T00:01:01Z", |c| set_lock(c, false, None))?;
            assert_eq!(lock_events(&activity(env, &query(50))?).len(), 2);
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_legacy_bracket_open_at_the_limit_edge_is_kept() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            commit_config(env, "2030-01-01T00:00:00Z", |c| set_lock(c, true, None))?;
            commit_config(env, "2030-01-01T00:00:01Z", |c| set_lock(c, false, None))?;
            let log = activity(env, &query(1))?;
            assert_eq!(log.entries.len(), 1);
            assert!(matches!(
                log.entries[0].events[..],
                [HitchEvent::Unlocked { .. }]
            ));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_manual_unlock_is_read_from_the_old_configs_purpose() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            commit_config(env, "2030-01-01T00:00:00Z", |c| {
                set_lock(c, true, Some("manual"))
            })?;
            commit_config(env, "2030-01-01T00:00:01Z", |c| set_lock(c, false, None))?;
            let log = activity(env, &query(50))?;
            let locks = lock_events(&log);
            assert_eq!(locks.len(), 2, "{locks:?}");
            assert!(matches!(locks[0], HitchEvent::Unlocked { .. }));
            assert!(matches!(locks[1], HitchEvent::Locked { .. }));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_readable_root_commit_still_creates_its_environments() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            env.git.run(&["checkout", "hitch-metadata"])?;
            let good = env.fs.read_file("hitch.json")?;
            env.git.run(&["checkout", "--orphan", "rewritten"])?;
            env.git.run(&["rm", "-rf", "--cached", "."])?;
            env.fs.write_file("hitch.json", &good)?;
            env.git.run(&["add", "-f", "hitch.json"])?;
            env.git.run(&["commit", "-m", "test: readable root"])?;
            env.git
                .run(&["branch", "-f", "hitch-metadata", "rewritten"])?;
            env.git.run(&["checkout", "main"])?;

            let log = activity(env, &query(50))?;
            assert!(log.skipped.is_empty());
            assert!(events_of(&log).iter().any(|ev| matches!(ev,
                HitchEvent::EnvironmentCreated { environment, .. } if environment == "dev")));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    fn rebuild(env: &TestEnvironment, environment: &str) {
        env.hitch
            .run()
            .args(&["--no-push", "rebuild", environment])
            .execute()
            .unwrap();
    }

    fn rebuilt_outcomes(log: &ActivityLog) -> Vec<&RebuildOutcome> {
        events_of(log)
            .into_iter()
            .filter_map(|ev| match ev {
                HitchEvent::Rebuilt { outcome, .. } => Some(outcome),
                _ => None,
            })
            .collect()
    }

    fn setup_hold(env: &TestEnvironment) -> anyhow::Result<()> {
        add_env(env, "dev");
        env.fs.write_file("shared.txt", "base\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "Add shared.txt"])?;
        for b in ["branch-a", "branch-b"] {
            env.git.run(&["checkout", "-b", b])?;
            env.fs.write_file("shared.txt", &format!("from {b}\n"))?;
            env.git.run(&["add", "-f", "shared.txt"])?;
            env.git.run(&["commit", "-m", b])?;
            env.git.run(&["checkout", "main"])?;
        }
        env.git.run(&["checkout", "hitch-metadata"])?;
        let mut config: serde_json::Value = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git.run(&["commit", "-m", "test: inject branches"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    #[test]
    fn the_latest_rebuild_carries_its_holds() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_hold(env)?;
            rebuild(env, "dev");
            let log = activity(env, &query(50))?;
            match rebuilt_outcomes(&log).as_slice() {
                [RebuildOutcome::WithHolds { included, held }] => {
                    assert_eq!(included, &vec!["branch-a".to_string()]);
                    assert_eq!(held.len(), 1);
                    assert_eq!(held[0].branch, "branch-b");
                    assert_eq!(held[0].conflicts_with, "branch-a");
                }
                other => panic!("expected one WithHolds rebuild, got {other:?}"),
            }
            // Filtering by the held branch must still find the rebuild.
            let filtered = activity(
                env,
                &ActivityQuery {
                    branch: Some("branch-b".into()),
                    ..query(50)
                },
            )?;
            assert!(matches!(
                rebuilt_outcomes(&filtered).as_slice(),
                [RebuildOutcome::WithHolds { .. }]
            ));
            // A branch the rebuild never touched drops the event.
            let other = activity(
                env,
                &ActivityQuery {
                    branch: Some("nope".into()),
                    ..query(50)
                },
            )?;
            assert!(rebuilt_outcomes(&other).is_empty());
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn an_older_rebuild_is_unrecorded_not_clean() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feat-1")?;
            make_feature(env, "feat-2")?;
            promote(env, "feat-1", "dev");
            rebuild(env, "dev");
            promote(env, "feat-2", "dev");
            rebuild(env, "dev");
            let log = activity(env, &query(50))?;
            match rebuilt_outcomes(&log).as_slice() {
                [RebuildOutcome::Clean { included }, RebuildOutcome::Unrecorded] => {
                    assert_eq!(included.len(), 2);
                }
                other => panic!("expected [Clean, Unrecorded], got {other:?}"),
            }
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_record_from_a_rebuild_outside_the_walk_is_not_misattributed() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feat-1")?;
            make_feature(env, "feat-2")?;
            promote(env, "feat-1", "dev");
            rebuild(env, "dev");
            promote(env, "feat-2", "dev");
            let log = activity(env, &query(1))?;
            assert!(rebuilt_outcomes(&log).is_empty());
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_truncated_walk_showing_one_rebuild_leaves_it_unrecorded() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feat-1")?;
            promote(env, "feat-1", "dev");
            rebuild(env, "dev");
            let full = activity(env, &query(50))?;
            assert!(matches!(
                rebuilt_outcomes(&full).as_slice(),
                [RebuildOutcome::Clean { .. }]
            ));
            let log = activity(env, &query(1))?;
            assert!(log.truncated);
            assert!(matches!(
                rebuilt_outcomes(&log).as_slice(),
                [RebuildOutcome::Unrecorded]
            ));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    #[test]
    fn a_legacy_repo_without_a_record_says_unrecorded() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            add_env(env, "dev");
            make_feature(env, "feat-1")?;
            promote(env, "feat-1", "dev");
            rebuild(env, "dev");
            assert!(
                env.git
                    .run(&["rev-parse", "--verify", "refs/hitch/state/dev"])?
                    .success(),
                "a rebuild should have written a record"
            );
            env.git
                .run(&["update-ref", "-d", "refs/hitch/state/dev"])?
                .assert_success();
            let log = activity(env, &query(50))?;
            assert!(matches!(
                rebuilt_outcomes(&log).as_slice(),
                [RebuildOutcome::Unrecorded]
            ));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }
}
