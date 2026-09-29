//! P9 Constraint 11: default output explains meaning, `--verbose` explains
//! mechanism.
//!
//! One scenario drives the common commands' normal paths against one repo and
//! checks every byte they print against a list of mechanism words. The list is
//! the contract; the allow-list below is the only way past it, and every entry
//! says why. Future phases that add output extend the scenario, not the rules.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// A mechanism word. Matching is case-insensitive and anchored at the
    /// start of a word; `whole` also anchors the end (`ref` must not match
    /// `reference`, but `anchor` should match `anchored`).
    struct Forbidden {
        needle: &'static str,
        whole: bool,
    }

    const FORBIDDEN: &[Forbidden] = &[
        Forbidden {
            needle: "sha",
            whole: true,
        },
        Forbidden {
            needle: "oid",
            whole: true,
        },
        Forbidden {
            needle: "ref",
            whole: true,
        },
        Forbidden {
            needle: "refs/",
            whole: false,
        },
        Forbidden {
            needle: "cas",
            whole: true,
        },
        Forbidden {
            needle: "eject",
            whole: false,
        },
        Forbidden {
            needle: "materialis",
            whole: false,
        },
        Forbidden {
            needle: "materializ",
            whole: false,
        },
        Forbidden {
            needle: "merge-tree",
            whole: false,
        },
        Forbidden {
            needle: "update-ref",
            whole: false,
        },
        Forbidden {
            needle: "commit-tree",
            whole: false,
        },
        Forbidden {
            needle: "force-with-lease",
            whole: false,
        },
        Forbidden {
            needle: "journal",
            whole: true,
        },
        Forbidden {
            needle: "fingerprint",
            whole: false,
        },
        Forbidden {
            needle: "anchor",
            whole: false,
        },
        Forbidden {
            needle: "hitch-metadata",
            whole: false,
        },
    ];

    /// A match survives only if the offending *line* contains `line_contains`.
    struct Allowed {
        needle: &'static str,
        line_contains: &'static str,
        reason: &'static str,
    }

    const ALLOWED: &[Allowed] = &[
        Allowed {
            needle: "hitch-metadata",
            line_contains: "git push origin hitch-metadata",
            reason: "a pasteable remedy: the user must run this exact git command \
                     (Constraint 11, exception 2)",
        },
        Allowed {
            needle: "hitch-metadata",
            line_contains: "Push manually with: git push origin hitch-metadata",
            reason: "the next step for a failed settings push is a pasteable git \
                     command (Constraint 11, exception 2)",
        },
        Allowed {
            needle: "hitch-metadata",
            line_contains: "git checkout hitch-metadata && git pull",
            reason: "a pasteable reconcile remedy for a diverged remote \
                     (Constraint 11, exception 2)",
        },
    ];
    // `detached HEAD` (the user's own checkout, in git's own term) contains no
    // forbidden token, so it needs no entry.

    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Every forbidden needle found in `line`, respecting word boundaries.
    fn hits(line: &str) -> Vec<&'static str> {
        let lower = line.to_lowercase();
        let mut found = Vec::new();
        for f in FORBIDDEN {
            let mut from = 0;
            while let Some(pos) = lower[from..].find(f.needle) {
                let start = from + pos;
                let end = start + f.needle.len();
                let before_ok = lower[..start]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !is_word_char(c));
                let after_ok =
                    !f.whole || lower[end..].chars().next().is_none_or(|c| !is_word_char(c));
                if before_ok && after_ok {
                    found.push(f.needle);
                    break;
                }
                from = end;
            }
        }
        found
    }

    fn violations(step: &str, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.lines() {
            for needle in hits(line) {
                let allowed = ALLOWED.iter().any(|a| {
                    assert!(!a.reason.is_empty());
                    a.needle == needle && line.contains(a.line_contains)
                });
                if !allowed {
                    out.push(format!("[{step}] `{needle}` in: {line}"));
                }
            }
        }
        out
    }

    #[derive(Clone, Copy)]
    enum Expect {
        Ok,
        Fail,
        Code(i32),
        /// A prompt step: which way the refusal exits varies by command, and the
        /// text is what is under test.
        Any,
    }

    struct Scenario<'a> {
        env: &'a TestEnvironment,
        verbose: bool,
        /// Run without the harness's `--no-push`, so push-owing strings print.
        push: bool,
        transcript: Vec<(String, String)>,
    }

    impl Scenario<'_> {
        fn step(&mut self, expect: Expect, args: &[&str]) -> anyhow::Result<()> {
            self.step_with(true, expect, args)
        }

        /// `yes: false` makes the gate print its plan and question, then refuse
        /// for want of a terminal: the only way to see a prompt's text.
        fn step_with(&mut self, yes: bool, expect: Expect, args: &[&str]) -> anyhow::Result<()> {
            let mut run = self
                .env
                .hitch
                .run()
                .with_yes(yes)
                .with_no_push(!self.push)
                .args(args);
            if self.verbose {
                run = run.verbose();
            }
            let result = run.execute()?;
            let text = format!("{}\n{}", result.stdout(), result.stderr());
            let code = result.exit_code();
            let ok = match expect {
                Expect::Ok => code == Some(0),
                Expect::Fail => code == Some(1),
                Expect::Code(c) => code == Some(c),
                Expect::Any => code.is_some(),
            };
            assert!(
                ok,
                "step `hitch {}` exited {code:?}:\n{text}",
                args.join(" ")
            );
            self.transcript.push((args.join(" "), text));
            Ok(())
        }
    }

    fn make_feature(
        env: &TestEnvironment,
        name: &str,
        file: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", name])?;
        env.fs.write_file(file, body)?;
        env.git.run(&["add", "-f", file])?;
        env.git.run(&["commit", "-m", &format!("{name}: change")])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    /// Declare branches directly, because `promote` refuses a branch it already
    /// knows conflicts and the scenario needs a hold.
    fn declare_branches(
        env: &TestEnvironment,
        environment: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "hitch-metadata"])?;
        let mut config: serde_json::Value = serde_json::from_str(&env.fs.read_file("hitch.json")?)?;
        config["environments"][environment]["branches"] = serde_json::to_value(branches)?;
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: declare branches directly"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    fn run_scenario(verbose: bool) -> anyhow::Result<Vec<(String, String)>> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::None, |env| {
            let mut s = Scenario {
                env,
                verbose,
                push: false,
                transcript: Vec::new(),
            };

            s.step(Expect::Ok, &["init"])?;
            env.setup_git_for_hitch()?;
            s.step(Expect::Ok, &["add", "dev"])?;
            s.step(Expect::Ok, &["add", "qa"])?;

            make_feature(env, "feat-a", "shared.txt", "alpha\n")?;
            make_feature(env, "feat-b", "shared.txt", "beta\n")?;
            make_feature(env, "feat-c", "c.txt", "c\n")?;

            s.step(Expect::Ok, &["promote", "feat-a", "dev"])?;
            declare_branches(env, "dev", &["feat-a", "feat-b"])?;
            s.step(Expect::Code(2), &["rebuild", "dev"])?;
            s.step(Expect::Ok, &["status"])?;
            s.step(Expect::Ok, &["why", "feat-b"])?;
            s.step(Expect::Ok, &["why", "dev"])?;
            s.step(Expect::Ok, &["log"])?;

            s.step(Expect::Ok, &["lock", "qa"])?;
            s.step(Expect::Fail, &["promote", "feat-c", "qa"])?;
            s.step(Expect::Ok, &["unlock", "qa"])?;

            s.step(Expect::Ok, &["add", "prod"])?;
            s.step(
                Expect::Ok,
                &[
                    "set",
                    "prod",
                    "--requires-approval",
                    "true",
                    "--add-approver",
                    "alice@example.com",
                ],
            )?;
            s.step(Expect::Ok, &["promote", "feat-c", "prod"])?;
            s.step(Expect::Ok, &["approvals", "list"])?;
            let request_id = env.read_hitch_config()?.approval_requests[0].id.clone();
            s.step(Expect::Ok, &["approvals", "status", &request_id])?;

            s.step(Expect::Ok, &["cleanup"])?;
            s.step(Expect::Ok, &["cleanup", "--apply"])?;

            s.step(Expect::Ok, &["demote", "feat-b", "dev"])?;
            s.step(Expect::Ok, &["release", "dev", "main"])?;
            s.step(Expect::Ok, &["log"])?;
            Ok(s.transcript)
        })
    }

    /// The same check over commands that owe a push, which the main scenario
    /// never reaches because the harness injects `--no-push`.
    fn run_push_scenario() -> anyhow::Result<Vec<(String, String)>> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::None, |env| {
            let mut s = Scenario {
                env,
                verbose: false,
                push: true,
                transcript: Vec::new(),
            };
            s.step_with(true, Expect::Ok, &["init"])?;
            env.setup_git_for_hitch()?;

            let bare = env.temp_dir.parent().unwrap().join(format!(
                "{}-origin",
                env.temp_dir.file_name().unwrap().to_string_lossy()
            ));
            std::fs::create_dir_all(&bare)?;
            #[allow(clippy::disallowed_methods)]
            let init = std::process::Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&bare)
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(init.status.success());
            env.git
                .run(&["remote", "add", "origin", &bare.to_string_lossy()])?
                .assert_success();

            make_feature(env, "feat-a", "a.txt", "a\n")?;
            make_feature(env, "feat-c", "c.txt", "c\n")?;
            env.git
                .run(&["push", "origin", "main", "feat-a", "feat-c"])?;
            env.git.run(&["push", "origin", "hitch-metadata"])?;

            s.step(Expect::Ok, &["add", "dev"])?;
            s.step(Expect::Ok, &["promote", "feat-a", "dev"])?;
            s.step(Expect::Ok, &["rebuild", "dev"])?;
            // Prompts: each owes a push, so each prints its confirmation reason.
            s.step_with(false, Expect::Any, &["set", "dev", "--on-conflict", "halt"])?;
            s.step_with(false, Expect::Any, &["promote", "feat-c", "dev"])?;
            s.step_with(false, Expect::Any, &["rebuild", "dev"])?;
            s.step_with(false, Expect::Any, &["remove", "dev"])?;
            s.step_with(false, Expect::Any, &["release", "dev", "main"])?;
            s.step(Expect::Ok, &["set", "dev", "--on-conflict", "halt"])?;
            s.step(Expect::Ok, &["status"])?;
            s.step(Expect::Ok, &["log"])?;
            Ok(s.transcript)
        })
    }

    #[test]
    fn push_owing_output_names_no_mechanism() -> anyhow::Result<()> {
        let transcript = run_push_scenario()?;
        assert!(
            transcript
                .iter()
                .any(|(_, t)| t.contains("publish it to origin")),
            "the push scenario must reach a push-owing prompt or it proves nothing"
        );
        let bad: Vec<String> = transcript
            .iter()
            .flat_map(|(step, text)| violations(step, text))
            .collect();
        assert!(
            bad.is_empty(),
            "push-time output leaked mechanism words:\n{}",
            bad.join("\n")
        );
        Ok(())
    }

    #[test]
    fn default_output_names_no_mechanism() -> anyhow::Result<()> {
        let transcript = run_scenario(false)?;
        let bad: Vec<String> = transcript
            .iter()
            .flat_map(|(step, text)| violations(step, text))
            .collect();
        assert!(
            bad.is_empty(),
            "default output leaked mechanism words:\n{}",
            bad.join("\n")
        );
        Ok(())
    }

    #[test]
    fn verbose_output_still_carries_mechanism() -> anyhow::Result<()> {
        let transcript = run_scenario(true)?;
        let promote = transcript
            .iter()
            .find(|(step, _)| step.starts_with("promote feat-a dev"))
            .expect("scenario promotes feat-a");
        // Moved to verbose by this task, not deleted.
        assert!(
            promote.1.contains("locked by") && promote.1.contains("unlocked"),
            "--verbose must still narrate the operation's own lock:\n{}",
            promote.1
        );
        Ok(())
    }
}

#[cfg(test)]
mod default_config_tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    #[test]
    fn a_fresh_init_says_nothing_about_a_missing_settings_file() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::None, |env| {
            let result = env.hitch.run().args(&["init"]).execute()?;
            let text = format!("{}\n{}", result.stdout(), result.stderr());
            result.assert_success();
            assert!(!text.contains("could not be read"), "{text}");
            assert!(!text.contains('⚠'), "{text}");
            Ok::<(), anyhow::Error>(())
        })
    }
}
