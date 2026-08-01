//! Integration tests for `hitch push`.
//!
//! Both tests push against a real local bare repository standing in for
//! `origin` — the same technique `crash_recovery_tests.rs` and
//! `trust_boundary_tests.rs` use — so these exercise the actual git
//! plumbing, not a mock. No `hitch setup` deploy key is configured in either
//! test, so `push`/`force_push_with_deploy_key_if_configured` take their
//! fallback branch: a plain `git push`/`git push --force-with-lease` against
//! `origin`.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

    /// A path next to the test repository rather than inside it, mirroring
    /// `crash_recovery_tests.rs::sibling_path` — a bare "remote" living
    /// inside `env.temp_dir` would show up as untracked content in the
    /// repo's own `git status` and trip hitch's working-tree-clean guard.
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

    /// Test-only: sets up a scratch bare remote to stand in for origin, and
    /// wires the test repo's `origin` remote at it. Deliberately spawns
    /// plain git rather than going through `GitOperations` (which has no
    /// repo at this path yet).
    fn init_bare_origin(env: &TestEnvironment, name: &str) -> anyhow::Result<std::path::PathBuf> {
        let bare_path = sibling_path(env, name);
        std::fs::create_dir_all(&bare_path)?;
        #[allow(clippy::disallowed_methods)]
        let init = std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&bare_path)
            .stdin(std::process::Stdio::null())
            .output()?;
        assert!(init.status.success(), "failed to init bare origin repo");

        env.git
            .run(&["remote", "add", "origin", &bare_path.to_string_lossy()])?
            .assert_success();

        Ok(bare_path)
    }

    /// Resolve `rev` against the bare repo at `bare_path`, returning an
    /// empty string if it doesn't resolve. Uses `--verify --quiet` — a plain
    /// `git rev-parse <ref>` on an unresolvable name doesn't error, it just
    /// echoes the argument back as literal text on stdout, which would make
    /// an emptiness check on the raw output silently wrong.
    fn rev_parse_in_bare(
        env: &TestEnvironment,
        bare_path: &std::path::Path,
        rev: &str,
    ) -> anyhow::Result<String> {
        Ok(env
            .git
            .run(&[
                format!("--git-dir={}", bare_path.to_string_lossy()).as_str(),
                "rev-parse",
                "--verify",
                "--quiet",
                rev,
            ])?
            .stdout()
            .trim()
            .to_string())
    }

    /// Regression test for the bug this test file was added to close:
    /// `hitch push <branch> -f` used to lease against `None`, which
    /// `--force-with-lease` reads as "the remote branch must not exist yet"
    /// — so the command failed with a "stale info" rejection whenever the
    /// remote branch already existed, which is the common case, not an edge
    /// case. `push.rs` now reads `refs/remotes/origin/<branch>` and leases
    /// against the real observed value.
    #[test]
    fn test_hitch_push_force_succeeds_against_existing_remote_branch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let bare_path = init_bare_origin(env, "bare-origin.git")?;

            // Get `dev` onto the remote once, so the remote branch already
            // exists — the exact precondition the bug required.
            env.hitch
                .run_raw()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run_raw()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            let remote_tip_before = rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;
            assert!(
                !remote_tip_before.is_empty(),
                "the initial rebuild's push must have landed on the bare remote"
            );

            // Advance `dev` locally (a second rebuild), so force-pushing it
            // means overwriting a remote branch that already exists.
            env.git
                .run(&["checkout", "-b", "feature-1"])?
                .assert_success();
            env.fs.write_file("1.txt", "one")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "feature 1"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();
            env.hitch
                .run_raw()
                .args(&["promote", "feature-1", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run_raw()
                .args(&["rebuild", "dev", "--no-push"])
                .execute()?
                .assert_success();

            let local_tip_after = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            assert_ne!(
                local_tip_after, remote_tip_before,
                "the second rebuild must have actually moved 'dev' locally"
            );

            let push_result = env.hitch.run_raw().args(&["push", "dev", "-f"]).execute()?;

            push_result
                .assert_success()
                .assert_stdout_contains("Pushed 'dev' to origin");

            let remote_tip_after = rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;
            assert_eq!(
                remote_tip_after, local_tip_after,
                "force-push must have moved the remote branch to the new local tip"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The safety property `--force-with-lease` exists for: `hitch push
    /// <branch> -f` must still refuse to overwrite a remote branch that has
    /// moved since hitch last observed it (a concurrent push from
    /// elsewhere), rather than blindly forcing through.
    ///
    /// This does NOT have differential power against the bug the test above
    /// regresses — leasing against `None` also rejects an existing remote
    /// branch unconditionally, so this test passes on that old code too, for
    /// the wrong reason. What it does catch: `push.rs` regressing to an
    /// actual unconditional `--force` (no lease at all), or re-fetching
    /// before leasing and thereby self-healing the staleness this test
    /// stages. Verified by hand: swapping `push.rs`'s force branch for a
    /// bare `force_push_branch` call (no lease) makes this test fail.
    #[test]
    fn test_hitch_push_force_rejects_when_remote_moved_since_last_observed() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let bare_path = init_bare_origin(env, "bare-origin.git")?;

            env.hitch
                .run_raw()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run_raw()
                .args(&["rebuild", "dev"])
                .execute()?
                .assert_success();

            // Simulate a concurrent push from somewhere else: advance the
            // bare remote's 'dev' directly, bypassing this checkout entirely
            // so its local 'refs/remotes/origin/dev' stays stale.
            #[allow(clippy::disallowed_methods)]
            let clone_dir = sibling_path(env, "concurrent-clone");
            #[allow(clippy::disallowed_methods)]
            let clone = std::process::Command::new("git")
                .args([
                    "clone",
                    &bare_path.to_string_lossy(),
                    &clone_dir.to_string_lossy(),
                ])
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(clone.status.success(), "failed to clone the bare origin");

            // The bare repo's default HEAD doesn't necessarily point at
            // 'dev' (nothing sets it there), so the clone's checked-out
            // branch after a plain clone isn't guaranteed to be 'dev' —
            // check it out explicitly so the concurrent commit lands on the
            // right history and a push of it is a genuine fast-forward.
            #[allow(clippy::disallowed_methods)]
            let checkout = std::process::Command::new("git")
                .args(["checkout", "dev"])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(
                checkout.status.success(),
                "failed to check out 'dev' in the concurrent clone: {}",
                String::from_utf8_lossy(&checkout.stderr)
            );

            #[allow(clippy::disallowed_methods)]
            std::process::Command::new("git")
                .args(["config", "user.email", "concurrent@example.com"])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new("git")
                .args(["config", "user.name", "Concurrent Pusher"])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            std::fs::write(clone_dir.join("concurrent.txt"), "someone else's change")?;
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            #[allow(clippy::disallowed_methods)]
            std::process::Command::new("git")
                .args(["commit", "-m", "concurrent change"])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            #[allow(clippy::disallowed_methods)]
            let push = std::process::Command::new("git")
                .args(["push", "origin", "dev"])
                .current_dir(&clone_dir)
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(
                push.status.success(),
                "the simulated concurrent push must land on the bare remote: {}",
                String::from_utf8_lossy(&push.stderr)
            );

            let remote_tip_after_concurrent_push =
                rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;

            // The local tracking ref must still be the pre-concurrent-push
            // value — that staleness IS the scenario. Note the ordering
            // constraint that makes this test work: every `hitch rebuild`
            // (including the one `hitch promote` triggers) starts with
            // `synchronize_branches`, which calls `fetch_all_remotes` and so
            // refreshes `refs/remotes/origin/*`. So the concurrent push has
            // to happen AFTER the last rebuild, not before it — otherwise
            // hitch re-observes the remote and the lease is legitimately
            // up to date, and there is no staleness left to test.
            let local_tracking = env
                .git
                .run(&[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "refs/remotes/origin/dev",
                ])?
                .stdout()
                .trim()
                .to_string();
            assert_ne!(
                local_tracking, remote_tip_after_concurrent_push,
                "the local remote-tracking ref must still be stale for this test to mean \
                 anything — if it already matches, something re-fetched and the lease would \
                 legitimately pass"
            );

            // Advance local `dev` directly, without any hitch command — a
            // rebuild here would re-fetch and destroy the staleness above.
            env.git.run(&["checkout", "dev"])?.assert_success();
            env.fs.write_file("local-only.txt", "local change")?;
            env.git.run(&["add", "."])?.assert_success();
            env.git
                .run(&["commit", "-m", "local advance on dev"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            let push_result = env.hitch.run_raw().args(&["push", "dev", "-f"]).execute()?;

            push_result
                .assert_failure()
                .assert_stderr_contains("stale info");

            let remote_tip_unchanged = rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;
            assert_eq!(
                remote_tip_unchanged, remote_tip_after_concurrent_push,
                "a rejected lease must not have moved the remote branch at all"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Baseline coverage for the non-force path: `hitch push <branch>`
    /// (without `-f`) pushes a brand-new branch to a remote that doesn't
    /// have it yet.
    #[test]
    fn test_hitch_push_plain_pushes_new_branch_to_remote() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let bare_path = init_bare_origin(env, "bare-origin.git")?;

            env.hitch
                .run_raw()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();
            env.hitch
                .run_raw()
                .args(&["rebuild", "dev", "--no-push"])
                .execute()?
                .assert_success();

            let remote_before = rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;
            assert!(
                remote_before.is_empty(),
                "the bare remote must not have 'dev' yet, found: {}",
                remote_before
            );

            let push_result = env.hitch.run_raw().args(&["push", "dev"]).execute()?;

            push_result
                .assert_success()
                .assert_stdout_contains("Pushed 'dev' to origin");

            let local_tip = env
                .git
                .run(&["rev-parse", "refs/heads/dev"])?
                .stdout()
                .trim()
                .to_string();
            let remote_tip = rev_parse_in_bare(env, &bare_path, "refs/heads/dev")?;
            assert_eq!(
                remote_tip, local_tip,
                "plain push must land the branch on the remote"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
