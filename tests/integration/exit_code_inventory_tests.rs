//! The exit codes, in one place.
//!
//! Every other test in the suite asserts a command's *behaviour*; this one
//! asserts only the code, and only because the code is a contract other things
//! read. A shell script in CI branches on it, a deploy pipeline decides whether
//! a hold is a warning or a failure, and a `--json` consumer treats a non-zero
//! exit as "the document on stdout is not a result". None of those read the
//! message text, so none of them would notice a code moving.
//!
//! Scattering these across the per-command files is what made them drift: each
//! `assert_failure()` says *something* went wrong, never *which* wrong, so a
//! command that started exiting 2 where it used to exit 1 passed every test
//! that had been written before. One inventory, asserted in one test, means a
//! change to one code is reported next to the nineteen that did not change.
//!
//! It duplicates assertions that exist in `lock_unlock_tests.rs`,
//! `add_remove_tests.rs`, `cleanup_tests.rs` and `approval_workflow_tests.rs`,
//! and that is deliberate rather than wasteful: those tests are about *what the
//! command did to the repository* and would be the natural place to delete a
//! loose `assert_failure` as tidying. This file's claim is narrower and
//! survives that.
//!
//! Two cases cannot be reached from here and are asserted in
//! `plan_apply_tests.rs` instead, because reaching them needs an answer to a
//! prompt rather than the absence of a TTY: a *declined* confirmation is `Ok`,
//! and a non-TTY prompt is a *refusal* which is `Err` — same "no" from the
//! user, opposite exit codes, and the difference between them is the whole
//! reason `--yes` exists.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::command_runners::HitchCommandResult;
    use crate::test_framework::*;

    /// Assert a command's exit code, naming the case when it is wrong.
    ///
    /// `HitchCommandResult::assert_exit_code` panics with the command line and
    /// both streams, which is enough to debug with and not enough to recognise:
    /// a failure here is "one of twenty codes moved", and the reader has to
    /// diff it against the list to find out which. The case label goes in the
    /// message so the failure is self-describing.
    fn expect_code(case: &str, result: &HitchCommandResult, expected: i32) {
        assert_eq!(
            result.exit_code(),
            Some(expected),
            "[{case}] expected exit {expected}, got {:?}\n  argv: {:?}\n  stdout: {}\n  \
             stderr: {}",
            result.exit_code(),
            result.args(),
            result.stdout(),
            result.stderr(),
        );
    }

    /// `lock` / `unlock`: a successful change is 0, and every refusal is 1.
    ///
    /// Both refusals are the same refusal seen from two sides — `lock` on a
    /// locked environment and `unlock` on an unlocked one are both "there is
    /// nothing to do here, and I will not pretend otherwise" — which is why
    /// they share a code rather than getting 0 as an idempotent success. A
    /// script that cannot tell "already in the state I wanted" from "changed"
    /// is better served by a code that says so.
    #[test]
    fn lock_and_unlock_exit_zero_for_a_change_and_one_for_a_no_op() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            expect_code(
                "add dev",
                &env.hitch.run().args(&["add", "dev"]).execute()?,
                0,
            );

            expect_code(
                "lock dev",
                &env.hitch.run().args(&["lock", "dev"]).execute()?,
                0,
            );
            expect_code(
                "lock dev (already locked)",
                &env.hitch.run().args(&["lock", "dev"]).execute()?,
                1,
            );

            expect_code(
                "unlock dev",
                &env.hitch.run().args(&["unlock", "dev"]).execute()?,
                0,
            );
            expect_code(
                "unlock dev (already unlocked)",
                &env.hitch.run().args(&["unlock", "dev"]).execute()?,
                1,
            );

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `set`: 0 when it changed something (or was asked to change nothing), 1
    /// for a policy refusal, and 1 for `--json` without `--yes`.
    ///
    /// The two 1s are different failures and the code merges them on purpose —
    /// a caller needs to know "this did not happen", and the difference between
    /// "your request was refused" and "you asked a program to decide for itself
    /// and did not" is in the document or the message, not the exit code. What
    /// must not happen is a `--json` refusal exiting 0, because a program that
    /// exits 0 with an empty stdout is indistinguishable from a run that
    /// succeeded and printed nothing.
    #[test]
    fn set_exits_one_for_a_refusal_and_for_json_without_yes() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // A threshold with nobody who can meet it: a policy refusal, decided
            // by the plan rather than by a prompt, so the harness's injected
            // `--yes` does not reach it. The approver-less form is the one that
            // fires — `validate_approval_config` only inspects a threshold on an
            // environment that actually requires approval, so raising
            // `min_approvals` on one that does not is a legal no-op and is 0.
            expect_code(
                "set dev --requires-approval true (no approver)",
                &env.hitch
                    .run()
                    .args(&["set", "dev", "--requires-approval", "true"])
                    .execute()?,
                1,
            );

            expect_code(
                "set dev --json (no --yes)",
                &env.hitch
                    .run()
                    .with_yes(false)
                    .args(&["set", "dev", "--base", "main", "--json"])
                    .execute()?,
                1,
            );

            // A real change, and the same change asked for twice. The second is
            // a no-op rather than a refusal — the plan has nothing to propose
            // and says so — which is why this is 0 and the two above are not.
            expect_code(
                "set dev --base main",
                &env.hitch
                    .run()
                    .args(&["set", "dev", "--base", "main"])
                    .execute()?,
                0,
            );
            expect_code(
                "set dev --base main (already on main)",
                &env.hitch
                    .run()
                    .args(&["set", "dev", "--base", "main"])
                    .execute()?,
                0,
            );
            // No flags at all: the plan describes the environment and proposes
            // nothing. 0, because a reader who asked a question got an answer.
            expect_code(
                "set dev (no flags)",
                &env.hitch.run().args(&["set", "dev"]).execute()?,
                0,
            );

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `add` / `remove`: 0 for a change, 1 for a refusal, and 0 for a
    /// `--force` overrule.
    ///
    /// `remove --force` is the one code P8 moved. It used to be 1 — a refused
    /// removal, because promoted branches meant the environment was still
    /// referenced and stopping was the safe answer. It is now 0 because the
    /// refusal moved into the *plan* as a warning the reader is asked about,
    /// and `--force` is the answer to that question. Keeping the old 1 would
    /// have said "this failed" about a run that did exactly what it was asked,
    /// and a script that special-cased it would have been teaching users not to
    /// pass `--force`.
    #[test]
    fn add_and_remove_exit_zero_for_a_change_and_one_for_a_refusal() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            expect_code(
                "add staging",
                &env.hitch.run().args(&["add", "staging"]).execute()?,
                0,
            );
            expect_code(
                "add staging (already exists)",
                &env.hitch.run().args(&["add", "staging"]).execute()?,
                1,
            );

            // A promoted branch is what makes `remove` worth confirming, and
            // what `--force` overrules.
            expect_code(
                "add dev",
                &env.hitch.run().args(&["add", "dev"]).execute()?,
                0,
            );
            env.git.run(&["checkout", "-b", "feat-live"])?;
            env.fs.write_file("feat-live.txt", "work")?;
            env.git.run(&["add", "-f", "feat-live.txt"])?;
            env.git.run(&["commit", "-m", "feat-live: work"])?;
            env.git.run(&["checkout", "main"])?;
            env.hitch
                .run()
                .args(&["promote", "feat-live", "dev", "--no-rebuild"])
                .execute()?
                .assert_success();

            expect_code(
                "remove dev --force (branch still promoted)",
                &env.hitch
                    .run()
                    .args(&["remove", "dev", "--force"])
                    .execute()?,
                0,
            );

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `cleanup`: a sweep that keeps an unmerged branch is 0, a delete that fails
    /// at apply time is 1, and `--json` without `--yes` is 1.
    ///
    /// The first is not a partial failure: the planner declines what
    /// `git branch -d` would refuse and says so, so nothing was attempted and
    /// nothing is owed. The second is a real failure and exits like one, after
    /// the receipt of what did apply.
    #[test]
    fn cleanup_exits_zero_when_it_keeps_a_branch_and_one_when_a_delete_fails() -> anyhow::Result<()>
    {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            expect_code(
                "add dev",
                &env.hitch.run().args(&["add", "dev"]).execute()?,
                0,
            );

            // Never merged into `main`, so `-d` would refuse it and the plan
            // leaves it out.
            env.git.run(&["checkout", "-b", "feat-stranded"])?;
            env.fs.write_file("stranded.txt", "content")?;
            env.git.run(&["add", "-f", "stranded.txt"])?;
            env.git.run(&["commit", "-m", "Add feat-stranded"])?;
            env.git.run(&["checkout", "main"])?;

            let kept = env.hitch.run().args(&["cleanup", "--apply"]).execute()?;
            expect_code("cleanup --apply (one branch kept)", &kept, 0);
            assert!(
                !kept.stdout().contains("Still owed"),
                "nothing is owed for a kept branch:\n{}",
                kept.stdout(),
            );

            // A refusal the plan cannot foresee: a stale lock on the ref.
            env.git.run(&["branch", "feat-locked"])?;
            env.fs.write_file(".git/refs/heads/feat-locked.lock", "")?;
            expect_code(
                "cleanup --apply (a delete fails)",
                &env.hitch.run().args(&["cleanup", "--apply"]).execute()?,
                1,
            );

            expect_code(
                "cleanup --json (no --yes)",
                &env.hitch
                    .run()
                    .with_yes(false)
                    .args(&["cleanup", "--apply", "--json"])
                    .execute()?,
                1,
            );

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// `approvals approve`: 0 in both of its outcomes, because recording a
    /// request and applying one are both things the command did.
    ///
    /// The second is the one worth stating. When the last approval lands the
    /// command edits the declaration, rebuilds the environment, and prints a
    /// receipt — and the rebuild is an *owed* effect when it does not run, not a
    /// failure of the approval. So "the request is now `Applied` but `dev` is
    /// stale" is 0 with a receipt warning naming `hitch rebuild dev`, the same
    /// shape as `promote --no-rebuild` and as a release whose dependent rebuild
    /// could not run. A non-zero code would tell the operator their approval was
    /// lost, when what happened is that it was recorded and applied, with one
    /// step deferred. That third shape has no flag that produces it — reaching
    /// it means contriving a merge conflict — so it is held from the other side
    /// by `a_release_that_owes_a_dependent_rebuild_still_releases`, and this
    /// inventory records the two codes it can actually drive.
    #[test]
    fn approve_exits_zero_whether_it_records_or_applies() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;
        framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            // The requester cannot count toward their own threshold, so the
            // approvers have to be other people or the *request* fails rather
            // than the approval — a different code, and one that would have made
            // this test pass for the wrong reason.
            env.hitch
                .run()
                .args(&[
                    "set",
                    "dev",
                    "--requires-approval",
                    "true",
                    "--min-approvals",
                    "2",
                    "--add-approver",
                    "alice@example.com",
                    "--add-approver",
                    "bob@example.com",
                ])
                .execute()?
                .assert_success();

            // A branch that is not approved: the request is recorded, nothing
            // is applied, and the command exits 0 because it did what it was
            // asked — file a request.
            env.git.run(&["checkout", "-b", "feat-gated"])?;
            env.fs.write_file("gated.txt", "work")?;
            env.git.run(&["add", "-f", "gated.txt"])?;
            env.git.run(&["commit", "-m", "feat-gated: work"])?;
            env.git.run(&["checkout", "main"])?;
            expect_code(
                "promote feat-gated dev (below threshold)",
                &env.hitch
                    .run()
                    .args(&["promote", "feat-gated", "dev", "--no-rebuild"])
                    .execute()?,
                0,
            );

            let request = request_id(env, "feat-gated")?;
            env.git.config_user("Alice", "alice@example.com")?;
            expect_code(
                "approve (recorded, 1 of 2)",
                &env.hitch
                    .run()
                    .args(&[
                        "approvals",
                        "approve",
                        &request,
                        "--comment",
                        "lgtm, one to go",
                    ])
                    .execute()?,
                0,
            );

            env.git.config_user("Bob", "bob@example.com")?;
            expect_code(
                "approve (applied, 2 of 2)",
                &env.hitch
                    .run()
                    .args(&["approvals", "approve", &request, "--comment", "shipping it"])
                    .execute()?,
                0,
            );

            Ok::<(), anyhow::Error>(())
        })?;
        Ok(())
    }

    /// The request id for a branch, which `approvals list` prints in a column
    /// rather than offering a machine-readable lookup for.
    fn request_id(env: &TestEnvironment, branch: &str) -> anyhow::Result<String> {
        let listing = env
            .hitch
            .run()
            .args(&["approvals", "list"])
            .execute()?
            .stdout();
        let id = listing
            .lines()
            .find(|line| line.contains(branch))
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| anyhow::anyhow!("no approval request for '{branch}' in:\n{listing}"))?
            .to_string();
        Ok(id)
    }
}
