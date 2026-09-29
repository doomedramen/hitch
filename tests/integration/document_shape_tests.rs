//! What each of the new operations' documents *say about themselves*.
//!
//! `core::render` being the only place allowed to choose words is what keeps a
//! plan from being worded twice, and its own unit tests pin the prose — that
//! renderer is pure, so that is the right place for it. What no test held until
//! now is the wiring: that `hitch lock` really produces a `Lock` plan with a
//! `Lock` edit, and not a `Lock` headline on top of an `Unlock` edit, or a
//! `SetEnvironment` plan whose detail happens to be right. Task 8 found a defect
//! of exactly that shape — `Approve  into 'production'`, a headline whose
//! "is this a demote?" test keyed on an empty list — and found it by *reading*
//! the output, which is not a repeatable way to find the next one.
//!
//! So the golden is typed rather than prose: under `--json` a document is
//! `{schema_version, plan, receipt}`, and what a consumer keys on is the plan's
//! `kind`, the intent's variant, the detail's discriminator, the outcome, and
//! the effect kinds. Those are the five facts that decide what the document
//! means, they are stable when the wording improves, and a mismatch is a real
//! defect rather than a copy edit.
//!
//! Prose is not ignored here — it is pinned where it belongs, in
//! `core::render`'s tests. This file is about the two halves agreeing.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::command_runners::HitchCommandResult;
    use crate::test_framework::*;
    use serde_json::Value;

    /// A document, reduced to the five facts that give it meaning.
    ///
    /// Rendered rather than compared field by field so a failure prints one
    /// readable line per fact. Compared field by field, a mismatch reports a
    /// `Value` for both sides and the reader has to diff them to find which of
    /// the five moved.
    fn describe(document: &Value) -> String {
        let plan = &document["plan"];
        let receipt = &document["receipt"];
        let intent = plan["intent"]
            .as_object()
            .map(|o| {
                let (name, body) = o.iter().next().expect("an intent has a variant");
                match body.as_object() {
                    Some(fields) if name == "SetEnvironment" => {
                        let n = fields["changes"].as_array().map(Vec::len).unwrap_or(0);
                        format!("{name}({n} change)")
                    }
                    _ => name.clone(),
                }
            })
            .unwrap_or_else(|| "none".into());
        let edit = match plan["detail"].get("edit") {
            Some(Value::String(e)) => e.clone(),
            _ => "-".into(),
        };
        let outcome = match receipt {
            Value::Null => "null receipt (a preview)".into(),
            r => r["outcome"].as_str().unwrap_or("?").to_string(),
        };
        let effects: Vec<String> = receipt["effects"]
            .as_array()
            .map(|effects| {
                effects
                    .iter()
                    .map(|e| {
                        e.as_object()
                            .and_then(|o| o.keys().next())
                            .cloned()
                            .unwrap_or_else(|| "?".into())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let owed: Vec<String> = receipt["warnings"]
            .as_array()
            .map(|warnings| {
                warnings
                    .iter()
                    .filter(|w| w["owes_effect"] == Value::Bool(true))
                    .map(|w| owed_subject(w["message"].as_str().unwrap_or("?")))
                    .collect()
            })
            .unwrap_or_default();

        format!(
            "kind:    {}\nintent:  {intent}\nedit:    {edit}\noutcome: {outcome}\n\
             effects: [{}]\nstill owed: [{}]",
            plan["kind"].as_str().unwrap_or("?"),
            effects.join(", "),
            owed.join(", "),
        )
    }

    /// The thing a warning says is still owed, reduced to a name.
    ///
    /// A warning's full text embeds whatever `git` said, and `git` rewrites its
    /// error messages between versions — pinning the whole sentence would make
    /// this test a tripwire for a git upgrade rather than for hitch. What the
    /// document owes is a *branch name*, so that is what is compared, and
    /// anything this cannot reduce is passed through verbatim rather than
    /// guessed at, so an unrecognised shape is visible in the diff instead of
    /// silently matching.
    fn owed_subject(message: &str) -> String {
        match message
            .split_once("delete branch '")
            .and_then(|(_, rest)| rest.split_once('\''))
        {
            Some((branch, _)) => branch.to_string(),
            None => message.to_string(),
        }
    }

    fn expect_document(case: &str, result: &HitchCommandResult, expected: &str) {
        let stdout = result.stdout();
        let document: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
            panic!(
                "[{case}] stdout is not one JSON document ({e})\n  argv: {:?}\n{stdout}",
                result.args()
            )
        });
        assert_eq!(
            document["schema_version"],
            Value::from(1),
            "[{case}] wrong schema_version\n{stdout}"
        );
        assert_eq!(
            describe(&document),
            expected,
            "[{case}] the document describes a different operation\n  argv: {:?}",
            result.args(),
        );
    }

    /// Promote a branch into `dev` without building it, so a later command has
    /// something in the declaration to act on.
    fn promote_a_feature(env: &TestEnvironment, name: &str) -> anyhow::Result<()> {
        env.git.run(&["checkout", "-b", name])?;
        env.fs.write_file("feature.txt", "work")?;
        env.git.run(&["add", "-f", "feature.txt"])?;
        env.git.run(&["commit", "-m", "Add feature work"])?;
        env.git.run(&["checkout", "main"])?;
        env.hitch
            .run()
            .args(&["promote", name, "dev", "--no-rebuild"])
            .execute()?
            .assert_success();
        Ok(())
    }

    /// The five metadata commands are five operations, not one with five flags.
    ///
    /// All four of `lock`, `unlock`, `set` and `add` produce a plan whose detail
    /// has the same *keys* — they share `MetadataPlanDetail` — so the shape test
    /// cannot tell them apart and `edit` is the only discriminator there is. That
    /// makes this the test that would catch a copy-paste in the dispatch: an
    /// `add` that planned a `Lock` edit, a `set` that planned a `Create`, or a
    /// `remove` that planned nothing. Each also gets a distinct `OperationKind`,
    /// because a `--json` consumer's switch is on `kind` and a `--json` consumer
    /// that receives `Lock` for a `hitch unlock` has been told a falsehood.
    ///
    /// `set` appears twice on purpose. Setting the base to the base it already
    /// has is a `NoChange` outcome and *zero* effects — a successful operation
    /// that did not do anything, which is the one outcome most easily flattened
    /// into "applied" by accident.
    #[test]
    fn each_metadata_command_is_its_own_operation() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            expect_document(
                "add dev",
                &env.hitch.run().args(&["add", "dev", "--json"]).execute()?,
                "kind:    AddEnvironment\n\
                 intent:  AddEnvironment\n\
                 edit:    Create\n\
                 outcome: Applied\n\
                 effects: [MetadataChange]\n\
                 still owed: []",
            );
            expect_document(
                "lock dev",
                &env.hitch.run().args(&["lock", "dev", "--json"]).execute()?,
                "kind:    Lock\n\
                 intent:  LockEnvironment\n\
                 edit:    Lock\n\
                 outcome: Applied\n\
                 effects: [MetadataChange]\n\
                 still owed: []",
            );
            expect_document(
                "unlock dev",
                &env.hitch
                    .run()
                    .args(&["unlock", "dev", "--json"])
                    .execute()?,
                "kind:    Unlock\n\
                 intent:  UnlockEnvironment\n\
                 edit:    Unlock\n\
                 outcome: Applied\n\
                 effects: [MetadataChange]\n\
                 still owed: []",
            );
            // No-op first: a `set` that names a field already at that value.
            expect_document(
                "set dev --base main (already on main)",
                &env.hitch
                    .run()
                    .args(&["set", "dev", "--base", "main", "--json"])
                    .execute()?,
                "kind:    SetEnvironment\n\
                 intent:  SetEnvironment(0 change)\n\
                 edit:    Settings\n\
                 outcome: NoChange\n\
                 effects: []\n\
                 still owed: []",
            );
            expect_document(
                "set dev --min-approvals 2",
                &env.hitch
                    .run()
                    .args(&[
                        "set",
                        "dev",
                        "--min-approvals",
                        "2",
                        "--add-approver",
                        "a@e.com",
                        "--add-approver",
                        "b@e.com",
                        "--json",
                    ])
                    .execute()?,
                "kind:    SetEnvironment\n\
                 intent:  SetEnvironment(2 change)\n\
                 edit:    Settings\n\
                 outcome: Applied\n\
                 effects: [MetadataChange]\n\
                 still owed: []",
            );

            promote_a_feature(env, "feat-live")?;
            expect_document(
                "remove dev --force",
                &env.hitch
                    .run()
                    .args(&["remove", "dev", "--force", "--json"])
                    .execute()?,
                "kind:    RemoveEnvironment\n\
                 intent:  RemoveEnvironment\n\
                 edit:    Destroy\n\
                 outcome: Applied\n\
                 effects: [MetadataChange]\n\
                 still owed: []",
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// A preview is a plan and nothing else; the apply is a plan and a receipt.
    ///
    /// The `null` receipt is the whole claim, and it is a deliberate one. A
    /// preview is dry-run-by-default for `hitch cleanup` — a reader who runs it
    /// has asked *what would happen*, and the answer has no "after". A receipt
    /// with `"effects": []` and `"outcome": "Applied"` would say it happened,
    /// and `null` says it did not, which is the difference between a preview and
    /// a lie. This is the same reasoning as the one-half envelope for
    /// `hitch status` and `hitch why`.
    ///
    /// The second half is the one effect in the CLI that is a *deletion*, and
    /// the warning a refused `git branch -d` earns. That warning is the only
    /// current producer of a receipt warning that owes an effect on this path,
    /// and it is here rather than in the plan because no fingerprint could have
    /// protected it: the plan is built before anything is deleted, and the
    /// failure is `git`'s, discovered while applying.
    #[test]
    fn a_cleanup_preview_is_a_plan_and_nothing_else() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch.run().args(&["add", "dev", "--json"]).execute()?;
            // Promoted, then the environment removed without the branch ever
            // being merged — the ordinary shape of a feature that shipped and
            // was then demoted, and the one `git branch -d` refuses.
            promote_a_feature(env, "feat-stranded")?;
            env.hitch
                .run()
                .args(&["remove", "dev", "--force", "--json"])
                .execute()?
                .assert_success();

            let preview = env.hitch.run().args(&["cleanup", "--json"]).execute()?;
            let document: Value = serde_json::from_str(&preview.stdout()).expect("a document");
            assert_eq!(
                document["receipt"],
                Value::Null,
                "a preview has no 'after', so its receipt is null rather than a \
                 receipt claiming nothing happened: {}",
                preview.stdout()
            );
            assert_eq!(document["plan"]["kind"], Value::from("Cleanup"));

            expect_document(
                "cleanup --apply",
                &env.hitch
                    .run()
                    .args(&["cleanup", "--apply", "--json"])
                    .execute()?,
                "kind:    Cleanup\n\
                 intent:  Cleanup\n\
                 edit:    -\n\
                 outcome: Applied\n\
                 effects: []\n\
                 still owed: [feat-stranded]",
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `approvals approve` plans with the *declaration* planner and records its
    /// own kind, so a consumer can tell an approved promote from a hand-run one.
    ///
    /// It shares `plan_declaration_change` deliberately — one plan per operation,
    /// and an approved promotion is the same edit to a declaration as any other
    /// promotion — so the detail here is the declaration detail and `edit` is
    /// absent. `kind: ApprovalApply` is the part that is its own: `hitch approve`
    /// and `hitch promote` produce byte-identical plans, and the only thing in
    /// either document that says a human authorised this one is the kind. A
    /// consumer that keys on `kind` is therefore the only thing that can answer
    /// "was this promoted or approved?", and this is the test that holds the
    /// answer.
    ///
    /// The second effect is the one worth checking for: a dependent rebuild that
    /// *ran* rides on the parent's effect list with its own `held` list, because
    /// the nested receipt is discarded and `✓` alone would be true and useless.
    #[test]
    fn approve_plans_as_a_declaration_change_and_records_its_own_kind() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch.run().args(&["add", "dev", "--json"]).execute()?;
            env.hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "1",
                    "--add-approver",
                    "alice@example.com",
                    "--json",
                ])
                .execute()?
                .assert_success();
            promote_a_feature(env, "feat-gated")?;

            let listing = env
                .hitch
                .run()
                .args(&["approvals", "list"])
                .execute()?
                .stdout();
            let request = listing
                .lines()
                .find(|line| line.contains("feat-gated"))
                .and_then(|line| line.split_whitespace().next())
                .expect("an approval request for feat-gated")
                .to_string();
            env.git.config_user("Alice", "alice@example.com")?;

            let approved = env
                .hitch
                .run()
                .args(&[
                    "approvals",
                    "approve",
                    &request,
                    "--comment",
                    "ok",
                    "--json",
                ])
                .execute()?;
            expect_document(
                "approvals approve",
                &approved,
                "kind:    ApprovalApply\n\
                 intent:  ApplyApproval\n\
                 edit:    -\n\
                 outcome: Applied\n\
                 effects: [MetadataChange, DependentEnvironmentRebuild]\n\
                 still owed: []",
            );
            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }
}
