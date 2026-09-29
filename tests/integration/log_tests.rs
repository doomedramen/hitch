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
    use hitch::core::activity::{build_activity, ActivityLog, ActivityQuery, HitchEvent};
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

    /// Until every mutation stops leaving lock/unlock commits behind, a
    /// promote's entry also carries those events.
    fn without_lock_events(events: &[HitchEvent]) -> Vec<HitchEvent> {
        events
            .iter()
            .filter(|e| !matches!(e, HitchEvent::Locked { .. } | HitchEvent::Unlocked { .. }))
            .cloned()
            .collect()
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
            let promoted = log
                .entries
                .iter()
                .find(|e| {
                    without_lock_events(&e.events).iter().any(|ev| {
                        matches!(ev, HitchEvent::Promoted { environment, branch }
                            if environment == "dev" && branch == "feature/a")
                    })
                })
                .expect("an entry holds the promote");
            assert_eq!(promoted.actor, user);
            assert_eq!(without_lock_events(&promoted.events).len(), 1);
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
            assert!(log.entries.len() == 2 && log.truncated);
            assert!(log.entries[0].when >= log.entries[1].when);
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
            assert!(by_branch
                .entries
                .iter()
                .flat_map(|e| &e.events)
                .all(|ev| ev.branches().contains(&"feature/devtools")));
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }
}
