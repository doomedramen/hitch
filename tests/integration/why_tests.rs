//! Integration tests for `hitch why` (spec §14).
//!
//! # What these tests are for, given `tests/unit/why_tests.rs` exists
//!
//! The unit tests build hand-made snapshots and assert the words; these build
//! real repositories and assert the *command* — which is where the parts a model
//! cannot be tested on live. Three of them:
//!
//! 1. **Subject resolution.** Which of the three forms a name resolves to is
//!    decided in `commands/why.rs` against git and `hitch.json`, and it is the
//!    one place `why` can be *wrong* rather than merely different. It has its
//!    own `rev_parse_opt`, and the interesting cases are the ones a hand-built
//!    snapshot cannot express: a name that is both an environment and a branch,
//!    a name that resolves to a ref hitch has never promoted.
//! 2. **The `--json` document.** The wire contract, which nothing else pins. The
//!    `form` discriminator in particular is what a consumer branches on, so it
//!    is asserted per form rather than once.
//! 3. **The lock and the error exits.** Both are *refusals*, and a refusal that
//!    exits 0 is indistinguishable from an answer.
//!
//! # Why the prose assertions are about content, not layout
//!
//! `render_why` right-aligns nothing and left-aligns the membership column to
//! the widest environment or branch name, so `dev` and `feature/alpha` produce
//! different padding. `assert_stdout_has_line` takes the line whole, which is
//! the stronger assertion, and these tests use it only on lines whose padding is
//! a function of a name the test itself chose — a single environment, or a
//! single branch. Anything whose width depends on a *set* is asserted on the
//! line's trimmed content instead.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// A feature branch off `main` with one file on it.
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

    /// Write branches straight into `hitch.json` on `hitch-metadata`, bypassing
    /// `promote`'s conflict preflight.
    ///
    /// Needed wherever a *feature* is supposed to be in conflict, because
    /// `promote` refuses to promote a branch it already knows conflicts.
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

    fn json_document(stdout: &str) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::from_str(stdout)?)
    }

    /// Every capitalised token in the document, sorted and deduplicated.
    ///
    /// The P7 decision is that every enum in this envelope is `snake_case`,
    /// because a variant's Rust name is not a wire contract anyone should be
    /// depending on. A collector over the whole document — rather than an
    /// assertion on the four enums this file knows about — is what makes a *new*
    /// enum without the rename fail here too.
    fn pascal_case_tokens(value: &serde_json::Value, into: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => {
                for token in s.split(['_', '-', ' ']) {
                    if token.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                        into.push(token.to_string());
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    pascal_case_tokens(item, into);
                }
            }
            serde_json::Value::Object(fields) => {
                for (key, field) in fields {
                    pascal_case_tokens(field, into);
                    if key.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                        into.push(key.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------
    // Form 1: `hitch why <branch>` — the branch everywhere
    // -----------------------------------------------------------------

    /// A promoted, built branch gets the feature form: its name, one line per
    /// environment it stands in, and a closing sentence about the whole set.
    ///
    /// The membership line is asserted whole, which is a padding-sensitive
    /// assertion and is allowed to be: there is exactly one environment here, so
    /// the column width is a function of one name this test chose.
    #[test]
    fn test_why_a_promoted_branch_answers_for_every_environment() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            make_feature(env, "feature/beta")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature/beta", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["why", "feature/alpha"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_has_line("feature/alpha")
                .stdout()
                .to_string();

            // The branch, then a line per environment, then the summary. No
            // `Desired`/`Actual` here: those are the *environment* form's, and a
            // branch asked about across every environment has one of everything
            // per environment.
            assert!(
                stdout.contains("● Included"),
                "an included branch says so: {stdout}"
            );
            assert!(
                !stdout.contains("Desired"),
                "the feature form is a status, not an environment write-up:\n{stdout}"
            );
            // It names *environments*, not sibling branches: the question is what
            // this branch's standing is everywhere, and a list of its peers in
            // each is a different question with a different command.
            assert!(
                !stdout.contains("feature/beta"),
                "the feature form does not enumerate the environment's other branches:\n{stdout}"
            );
            assert!(
                stdout.contains("feature/alpha is already in every environment that declares it"),
                "and the summary is about the whole set: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A branch hitch has never heard of is still a branch if the ref resolves,
    /// and the honest answer is "not promoted to any environment" — plus how to
    /// change that. Erroring here would be the *more* helpful behaviour, and it
    /// is the one this deliberately does not have: a reader who just made a
    /// branch and has not promoted it yet is asking a reasonable question.
    #[test]
    fn test_why_an_unpromoted_branch_says_it_is_not_promoted() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;

            let result = env.hitch.run().args(&["why", "feature/alpha"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("not promoted to any environment")
                // And the remedy, because a status with no next step is a
                // dead end for the reader.
                .stdout()
                .to_string();
            assert!(
                stdout.contains("hitch promote feature/alpha"),
                "the answer names the command that changes it: {stdout}"
            );
            // A `NotDesired` row is a statement about the *declaration*, and this
            // environment's own branch being missing is not a reason for it — so
            // the row carries no reason line. An environment-level fact stapled
            // onto a `NotDesired` cell reads as an explanation for the branch's
            // absence and is about something else entirely.
            assert!(
                !stdout.contains("does not exist"),
                "an unpromoted branch is not explained by the environment's state:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A name that resolves to no ref, and is not a declared branch, is *not a
    /// branch* — and saying so beats printing a grid with one empty row in it.
    /// Exit 1, because a caller that passed a typo needs to be able to tell.
    #[test]
    fn test_why_a_name_that_resolves_to_nothing_is_an_error() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["why", "no/such/branch"])
                .execute()?
                .assert_failure()
                .assert_stderr_contains("not an environment and not a branch")
                // The environments that *do* exist, so the reader can see the
                // near-miss rather than guess.
                .assert_stderr_contains("dev");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Form 2: `hitch why <environment>` — desired against actual
    // -----------------------------------------------------------------

    /// The environment form is the one that carries the shared *equation* — the
    /// same `render_equation` a plan's "Will change" section uses, so a plan and
    /// an explanation cannot describe the same environment differently.
    #[test]
    fn test_why_an_environment_shows_desired_actual_and_its_branches() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["why", "dev"]).execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_has_line("dev")
                .stdout()
                .to_string();

            for heading in ["Desired", "Actual", "Branches"] {
                assert!(
                    stdout.contains(&format!("\n{heading}\n")),
                    "the {heading} section is present:\n{stdout}"
                );
            }
            // The equation itself, in both columns: desired has the branch,
            // actual has it too, so the two agree — which is what a current
            // environment looks like.
            assert!(
                stdout.contains("dev = main + feature/alpha"),
                "the desired equation names the base and the branch:\n{stdout}"
            );
            // The membership of each branch, in the same vocabulary §12 uses.
            assert!(
                stdout.contains("● Included"),
                "a built branch reads as included: {stdout}"
            );
            // The verdict, as a sentence about the environment.
            assert!(
                stdout.contains("dev is realised"),
                "a current environment says so plainly: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// An environment that was declared but never built has no *actual*
    /// composition, and the form must not invent one. `hitch add` creates exactly
    /// this state, so it needs no fixture beyond the command.
    #[test]
    fn test_why_an_unbuilt_environment_has_no_actual_section() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["why", "dev"]).execute()?;
            let stdout = result.assert_success().stdout().to_string();

            assert!(stdout.contains("\nDesired\n"), "{stdout}");
            assert!(
                !stdout.contains("\nActual\n"),
                "there is no build, so there is no actual composition to state: {stdout}"
            );
            // And the verdict says *that*, rather than reading as a stale
            // "needs rebuild" — which is a different problem with a different
            // remedy.
            assert!(
                stdout.contains("never built") || stdout.contains("does not exist"),
                "the verdict distinguishes never-built from stale:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // Form 3: `hitch why <branch> <environment>` — one pair, in full
    // -----------------------------------------------------------------

    /// The two-argument form is what a user runs *before* a `promote`, so it
    /// carries three things the other two do not: the branch's membership on its
    /// own, a `Why?` when there is a reason, and the environment's own state.
    #[test]
    fn test_why_a_branch_in_an_environment_renders_the_pair() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();

            let result = env
                .hitch
                .run()
                .args(&["why", "feature/alpha", "dev"])
                .execute()?;
            let stdout = result
                .assert_success()
                // The headline names both halves, because the form is about a
                // *pair* and a reader who has six branches open needs to know
                // which one this is.
                .assert_stdout_has_line("feature/alpha → dev")
                .assert_stdout_contains("Desired")
                .assert_stdout_contains("Actual")
                .assert_stdout_contains("Membership")
                .stdout()
                .to_string();

            assert!(
                stdout.contains("● Included"),
                "the membership is the answer to the question asked: {stdout}"
            );
            // No `Why?` — a branch that is simply in the build has nothing to
            // explain, and an empty section would be noise on the most common
            // query this command will ever answer.
            assert!(
                !stdout.contains("\nWhy?\n"),
                "nothing is wrong with this branch, so nothing is explained:\n{stdout}"
            );
            // The environment's verdict still shows, because "included" in a
            // locked, two-rebuilds-behind environment means something quite
            // different from "included" in a current one.
            assert!(
                stdout.contains("dev is realised"),
                "the environment's own state is in the answer too: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A hold is the case the `Why?` section exists for, and it must name both
    /// the partner and the files — a reason that cannot be acted on is not a
    /// reason.
    ///
    /// The fixture has to bypass `promote`'s preflight, which is why it declares
    /// directly: `promote` refuses a branch it knows will conflict, so promoting
    /// this one and *then* conflicting it is the other way to reach the state, and
    /// the direct declaration is both shorter and the more honest way to say
    /// "assume this conflict is real".
    #[test]
    fn test_why_a_held_branch_names_its_partner_and_its_files() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            // Two branches that both rewrite the same line, from the same base.
            for branch in ["feature/payments", "feature/dashboard"] {
                env.git.run(&["checkout", "-b", branch])?;
                env.fs.write_file("src/shared.txt", branch)?;
                env.git.run(&["add", "-f", "src/shared.txt"])?;
                env.git.run(&["commit", "-m", branch])?;
                env.git.run(&["checkout", "main"])?;
            }
            declare_branches(env, "dev", &["feature/payments", "feature/dashboard"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_exit_code(2);

            let result = env
                .hitch
                .run()
                .args(&["why", "feature/dashboard", "dev"])
                .execute()?;
            let stdout = result
                .assert_success()
                .assert_stdout_contains("⛔ Held")
                .stdout()
                .to_string();

            assert!(
                stdout.contains("feature/payments"),
                "the hold names its partner, or it is not actionable: {stdout}"
            );
            assert!(
                stdout.contains("src/shared.txt"),
                "and the file that conflicted: {stdout}"
            );
            // `hitch resolve` is the only command that operates on a held
            // branch, so it is the only next step worth offering.
            assert!(
                stdout.contains("hitch resolve"),
                "a hold is resolved, not rebuilt around: {stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The lock is a human-facing refusal, and the two-argument form is the one
    /// run immediately before a `promote` — the exact moment it would bite. So it
    /// has to be in that form's answer, and it has to name the command that
    /// clears it.
    #[test]
    fn test_why_names_the_lock_that_would_refuse_the_promote() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            // Both environment-bearing forms name it, through one shared
            // function, so they cannot word it differently.
            for args in [vec!["why", "feature/alpha", "dev"], vec!["why", "dev"]] {
                let result = env.hitch.run().args(&args).execute()?;
                let stdout = result.assert_success().stdout().to_string();
                assert!(
                    stdout.contains("dev is locked"),
                    "{args:?} names the lock:\n{stdout}"
                );
                assert!(
                    stdout.contains("hitch unlock dev"),
                    "{args:?} names the command that clears it:\n{stdout}"
                );
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A locked environment *actually* refuses a promote, which is the
    /// consequence the sentence claims. Asserted because a sentence about a
    /// consequence that is not the consequence is the worst kind of wrong.
    #[test]
    fn test_a_lock_refuses_the_promote_the_why_mentions() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            make_feature(env, "feature/beta")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["promote", "feature/beta", "dev"])
                .execute()?
                .assert_failure();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// An environment named as the second argument that does not exist is an
    /// error naming the ones that do — and *not* an answer about the branch
    /// everywhere, which is what silently dropping the argument would produce.
    /// That would be the worst possible outcome: a question about one place
    /// answered as though it were a question about all of them.
    #[test]
    fn test_why_an_unknown_second_argument_is_an_error() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;

            let result = env
                .hitch
                .run()
                .args(&["why", "feature/alpha", "nosuchenv"])
                .execute()?;
            let stderr = result
                .assert_failure()
                .assert_stderr_contains("No environment named 'nosuchenv'")
                .stderr()
                .to_string();
            assert!(
                stderr.contains("dev"),
                "the error names the environments that do exist: {stderr}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A name that is both an environment and a promoted branch is genuinely
    /// ambiguous, and the two readings show different things — so both are named,
    /// each with the command that asks it.
    ///
    /// Note what the error cannot do: it cannot offer `hitch why <name>` for the
    /// environment reading, because that is the command that just failed. The
    /// second positional is the only disambiguator the forms allow and it works
    /// in one direction only, so the environment reading points at the view that
    /// does show it.
    #[test]
    fn test_why_a_name_that_is_both_an_environment_and_a_branch_names_both_readings(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "qa"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            // A *branch* called `qa`, in an environment called `dev`. `qa` is now
            // a name with two readings.
            //
            // Declared directly rather than promoted, and not as a convenience:
            // `hitch promote qa dev` refuses with "Environment 'qa' has no
            // branches promoted", because the preflight resolves `qa` against
            // the *environment* of that name and never reaches the branch. That
            // is `promote`'s own ambiguity resolution, in the opposite direction
            // from this command's, and it is why the fixture has to bypass it.
            make_feature(env, "qa")?;
            declare_branches(env, "dev", &["qa"])?;
            env.hitch
                .run()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let result = env.hitch.run().args(&["why", "qa"]).execute()?;
            let stderr = result
                .assert_failure()
                .assert_stderr_contains("ambiguous")
                .stderr()
                .to_string();

            // Both readings, each with the way to ask it.
            assert!(
                stderr.contains("hitch why qa <environment>"),
                "the branch reading is disambiguable with the second positional: {stderr}"
            );
            assert!(
                stderr.contains("hitch status --environments qa"),
                "the environment reading points at a view that shows it: {stderr}"
            );
            // And it does *not* suggest re-running the command that just failed.
            assert!(
                !stderr.contains("hitch why qa\n"),
                "no suggestion to run the ambiguous command again: {stderr}"
            );

            // The disambiguated forms both work, which is the point of the error.
            env.hitch
                .run()
                .args(&["why", "qa", "dev"])
                .execute()?
                .assert_success()
                .assert_stdout_has_line("qa → dev");
            // And the environment reading, through the command the error named.
            env.hitch
                .run()
                .args(&["status", "--environments", "qa"])
                .execute()?
                .assert_success()
                .assert_stdout_contains("qa");

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // -----------------------------------------------------------------
    // The `--json` document
    // -----------------------------------------------------------------

    /// The three forms are distinguished by a `form` field, and it is a field
    /// rather than a wrapper key precisely so a consumer can branch on it.
    /// Asserted once per form, over a real repository, because "the discriminator
    /// exists" and "the discriminator says the right thing for *this* form" are
    /// different claims and only the second one is useful.
    #[test]
    fn test_why_json_carries_a_form_discriminator_per_form() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();

            let cases = [
                (vec!["--json", "why", "feature/alpha"], "feature"),
                (vec!["--json", "why", "dev"], "environment"),
                (
                    vec!["--json", "why", "feature/alpha", "dev"],
                    "feature_in_environment",
                ),
            ];

            for (args, form) in &cases {
                let result = env.hitch.run().args(args).execute()?;
                let document = json_document(&result.assert_success().stdout())?;

                assert_eq!(
                    document["why"]["form"].as_str(),
                    Some(*form),
                    "{args:?} is the {form} form:\n{document:#?}"
                );
                // The envelope: a version, and one key holding the view. A
                // `receipt` here would say "nothing happened" — true, and
                // useless — so its absence is part of the claim.
                assert_eq!(
                    document["schema_version"].as_u64(),
                    Some(1),
                    "{args:?} is versioned:\n{document:#?}"
                );
                let mut keys: Vec<&str> = document
                    .as_object()
                    .expect("an object")
                    .keys()
                    .map(|k| k.as_str())
                    .collect();
                keys.sort_unstable();
                assert_eq!(
                    keys,
                    vec!["schema_version", "why"],
                    "{args:?} has exactly the envelope keys and no receipt:\n{document:#?}"
                );
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Every enum in the document is `snake_case`, because a variant's Rust name
    /// is not a wire contract. A collector over the *whole* document rather than
    /// an assertion on the enums this test knows about, so a new enum without
    /// the rename fails here too.
    #[test]
    fn test_why_json_contains_no_rust_type_names() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();

            // Every form, and two states, because a document is only as
            // snake_case as its least cooperative field.
            for args in [
                vec!["--json", "why", "feature/alpha"],
                vec!["--json", "why", "dev"],
                vec!["--json", "why", "feature/alpha", "dev"],
            ] {
                let result = env.hitch.run().args(&args).execute()?;
                let stdout = result.assert_success().stdout().to_string();
                let document = json_document(&stdout)?;
                let mut found = Vec::new();
                pascal_case_tokens(&document, &mut found);
                found.sort();
                found.dedup();
                assert!(
                    found.is_empty(),
                    "{args:?} leaks Rust type names: {found:?}\n{stdout}"
                );
            }

            // And the membership is spelled the way a consumer would have to
            // write it down.
            let result = env
                .hitch
                .run()
                .args(&["--json", "why", "feature/alpha", "dev"])
                .execute()?;
            let document = json_document(&result.assert_success().stdout())?;
            assert_eq!(
                document["why"]["membership"].as_str(),
                Some("included"),
                "the membership is snake_case:\n{document:#?}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `--json` owns stdout and nothing else does — a read-only command has
    /// nothing to warn about, so a stray line on stdout would corrupt a
    /// consumer's parse rather than merely be untidy.
    #[test]
    fn test_why_json_stdout_is_exactly_one_document() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();

            let result = env
                .hitch
                .run()
                .args(&["--json", "why", "feature/alpha", "dev"])
                .execute()?;
            let stdout = result.assert_success().stdout().to_string();

            // `from_str` rejects trailing content, so a second document or a
            // stray print is a failure here rather than a surprise in a
            // consumer.
            serde_json::from_str::<serde_json::Value>(&stdout)
                .unwrap_or_else(|e| panic!("stdout is not exactly one document: {e}\n{stdout}"));
            assert!(
                !stdout.contains('\u{1b}'),
                "no escape bytes in a machine-readable document:\n{stdout}"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `why` is read-only, and the cheapest way to show that is to ask about a
    /// repository while something else holds the lock. A read-only command that
    /// took the repo lock would block here, and a reader waiting on a mutation
    /// in progress is exactly the wrong time to make them.
    #[test]
    fn test_why_does_not_take_the_repository_lock() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            make_feature(env, "feature/alpha")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();
            // The *environment* lock, which is the one a reader would be blocked
            // by. It does not take the repo-wide flock, so this test is about the
            // human-facing one and about the command not refusing.
            env.hitch
                .run()
                .args(&["lock", "dev"])
                .execute()?
                .assert_success();

            env.hitch
                .run()
                .args(&["why", "dev"])
                .execute()?
                .assert_success();

            // A branch question works under a lock too, for the same reason.
            env.hitch
                .run()
                .args(&["why", "feature/alpha"])
                .execute()?
                .assert_success();

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Every row of the matrix, asked about as a `why`, gets an answer that
    /// agrees with the cell — the property that makes the two commands one
    /// vocabulary rather than two.
    #[test]
    fn test_why_agrees_with_the_status_matrix_about_every_branch() -> anyhow::Result<()> {
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
            make_feature(env, "feature/alpha")?;
            make_feature(env, "feature/beta")?;
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run()
                .args(&["promote", "feature/alpha", "qa"])
                .execute()?
                .assert_success();
            // Promoted but *not* built, so its cell is "needs rebuild" and its
            // why must say why.
            env.hitch
                .run()
                .args(&["promote", "feature/beta", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            let status = env
                .hitch
                .run()
                .args(&["status"])
                .execute()?
                .assert_success()
                .stdout()
                .to_string();

            for (branch, cell) in [
                ("feature/alpha", "● included"),
                ("feature/beta", "↻ needs rebuild"),
            ] {
                // The matrix says it…
                assert!(
                    status.contains(cell),
                    "the matrix calls {branch} {cell:?}:\n{status}"
                );
                // …and the why, asked for that same pair, says the same thing.
                let environment = if branch == "feature/beta" {
                    "dev"
                } else {
                    "qa"
                };
                let why = env
                    .hitch
                    .run()
                    .args(&["why", branch, environment])
                    .execute()?
                    .assert_success()
                    .stdout()
                    .to_string();
                let membership = match cell {
                    "● included" => "● Included",
                    "↻ needs rebuild" => "↻ Needs rebuild",
                    other => panic!("no membership for {other:?}"),
                };
                assert!(
                    why.contains(membership),
                    "{branch} in {environment} is {membership:?} to `why` and \
                     {cell:?} to the matrix:\n{why}\n--- matrix ---\n{status}"
                );
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
