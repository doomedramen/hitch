//! Red-team tests: each one performs an actual attack and asserts hitch is
//! inert. These are regression guards for the trust-boundary work, not
//! documentation.
//!
//! Most tests here drive the built `hitch` binary directly. The two
//! deploy-key push tests instead call `GitOperations::push_with_ssh_identity`
//! / `force_push_with_ssh_identity` directly against a local bare "remote":
//! exercising them through the CLI would need a real `hitch setup` (a GitHub
//! repo, a ruleset, an actual deploy key) which isn't available to a local
//! test run, but the function under test is exactly the same code hitch's
//! push path calls, and a local-path push still triggers the client-side
//! `pre-push` hook this suite is guarding against — GIT_SSH_COMMAND is simply
//! unused when the destination isn't an SSH URL.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use hitch::utils::git_operations::GitOperations;

    /// A path next to the test repository rather than inside it. The hook
    /// directory and its sentinel must not live inside `env.temp_dir` (the
    /// repo's own working tree) or they show up as untracked content in the
    /// repo's own `git status`, which trips hitch's "working tree is not
    /// clean" guard before the attack under test is even exercised — see the
    /// same gotcha documented in AGENTS.md for worktrees.
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

    /// Write an executable hook script at `hooks_dir/name` that touches
    /// `sentinel` when it runs.
    fn write_hook(hooks_dir: &std::path::Path, name: &str, sentinel: &std::path::Path) {
        std::fs::create_dir_all(hooks_dir).expect("create evil hooks dir");
        let hook = hooks_dir.join(name);
        std::fs::write(
            &hook,
            format!("#!/bin/sh\ntouch {}\n", sentinel.to_string_lossy()),
        )
        .expect("write hook script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
                .expect("make hook executable");
        }
    }

    /// A repository-local `core.hooksPath` must not get hitch to execute a
    /// script. hitch runs under a deploy key that bypasses branch protection,
    /// so a hook firing inside its process is arbitrary code with push rights.
    ///
    /// Uses the `reference-transaction` hook rather than `post-commit`:
    /// hitch's metadata writes (`begin_branch_write`/`commit_branch_write`)
    /// go through `commit-tree` + `update-ref` plumbing, never a porcelain
    /// `git commit`, so `post-commit` never fires regardless of hardening —
    /// confirmed by manually instrumenting every hook name and running
    /// `hitch add` against it. `reference-transaction` fires on the
    /// `update-ref` every mutating command performs against
    /// `refs/heads/hitch-metadata`, so it actually exercises the attack this
    /// test is guarding against.
    #[test]
    fn test_repo_local_hooks_path_does_not_execute_under_hitch() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let sentinel = sibling_path(env, "PWNED");
            let hooks_dir = sibling_path(env, "evil-hooks");
            std::fs::create_dir_all(&hooks_dir)?;

            // reference-transaction fires on every ref update hitch makes
            // (it updates refs/heads/hitch-metadata on every mutating
            // command via `update-ref`).
            let hook = hooks_dir.join("reference-transaction");
            std::fs::write(
                &hook,
                format!("#!/bin/sh\ntouch {}\n", sentinel.to_string_lossy()),
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
            }

            env.git
                .run(&["config", "core.hooksPath", &hooks_dir.to_string_lossy()])?
                .assert_success();

            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            assert!(
                !sentinel.exists(),
                "a repo-local hook executed inside hitch's process"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// `post-index-change` fires after `read-tree`/`update-index` touch the
    /// index — exactly what `begin_branch_write`/`stage_file_in_pending_write`
    /// use under the hood for every `modify_metadata` call, i.e. hitch's most
    /// common mutating operation. Regression guard for a gap where
    /// `run_git_command_with_index` built its own `Command` instead of
    /// going through the shared hardened builder.
    #[test]
    fn test_repo_local_hooks_path_does_not_execute_via_scratch_index_writes() -> anyhow::Result<()>
    {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let sentinel = sibling_path(env, "PWNED-index");
            let hooks_dir = sibling_path(env, "evil-hooks-index");
            write_hook(&hooks_dir, "post-index-change", &sentinel);

            env.git
                .run(&["config", "core.hooksPath", &hooks_dir.to_string_lossy()])?
                .assert_success();

            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            assert!(
                !sentinel.exists(),
                "a repo-local post-index-change hook executed during a scratch-index write"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// See the module doc comment for why this calls `GitOperations` directly
    /// rather than driving the CLI: `push_with_ssh_identity` is the deploy-key
    /// push path (AGENTS.md documents it as the only correct way to push a
    /// protected branch), and it built its own unhardened `Command` — a
    /// repo-local `pre-push` hook fired on an equivalent unguarded push.
    #[test]
    fn test_repo_local_hooks_path_does_not_execute_via_ssh_identity_push() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let sentinel = sibling_path(env, "PWNED-push");
            let hooks_dir = sibling_path(env, "evil-hooks-push");
            write_hook(&hooks_dir, "pre-push", &sentinel);

            env.git
                .run(&["config", "core.hooksPath", &hooks_dir.to_string_lossy()])?
                .assert_success();

            let bare_path = sibling_path(env, "bare-remote.git");
            std::fs::create_dir_all(&bare_path)?;
            // Test-only: sets up a scratch bare remote for the red-team
            // scenario, so it deliberately spawns plain git rather than
            // going through GitOperations (which has no repo here yet).
            #[allow(clippy::disallowed_methods)]
            let init = std::process::Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&bare_path)
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(init.status.success(), "failed to init bare remote repo");

            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            let branch = git_ops.get_current_branch()?;

            // remote_url is a plain local path rather than an ssh:// URL, so
            // GIT_SSH_COMMAND is simply unused — this still exercises the
            // client-side pre-push hook, which fires regardless of transport.
            git_ops.push_with_ssh_identity(
                &branch,
                "/nonexistent/dummy-identity",
                &bare_path.to_string_lossy(),
            )?;

            assert!(
                !sentinel.exists(),
                "a repo-local pre-push hook executed during push_with_ssh_identity"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// See `test_repo_local_hooks_path_does_not_execute_via_ssh_identity_push`
    /// — same gap and same rationale, for the force-push (lease) variant used
    /// when hitch has to overwrite a protected branch's tip.
    #[test]
    fn test_repo_local_hooks_path_does_not_execute_via_ssh_identity_force_push(
    ) -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::GitOnly, |env| {
            let sentinel = sibling_path(env, "PWNED-force-push");
            let hooks_dir = sibling_path(env, "evil-hooks-force-push");
            write_hook(&hooks_dir, "pre-push", &sentinel);

            env.git
                .run(&["config", "core.hooksPath", &hooks_dir.to_string_lossy()])?
                .assert_success();

            let bare_path = sibling_path(env, "bare-remote-force.git");
            std::fs::create_dir_all(&bare_path)?;
            // Test-only: sets up a scratch bare remote for the red-team
            // scenario, so it deliberately spawns plain git rather than
            // going through GitOperations (which has no repo here yet).
            #[allow(clippy::disallowed_methods)]
            let init = std::process::Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&bare_path)
                .stdin(std::process::Stdio::null())
                .output()?;
            assert!(init.status.success(), "failed to init bare remote repo");

            let git_ops = GitOperations::new_at_path(&env.temp_dir.to_string_lossy())?;
            let branch = git_ops.get_current_branch()?;

            git_ops.force_push_with_ssh_identity(
                &branch,
                None,
                "/nonexistent/dummy-identity",
                &bare_path.to_string_lossy(),
            )?;

            assert!(
                !sentinel.exists(),
                "a repo-local pre-push hook executed during force_push_with_ssh_identity"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// With `require_signed_resolutions` on, a planted resolution ref — the
    /// shape an attacker with push access creates — must not be replayed into
    /// a build, even under `--yes --replay-resolutions`.
    #[test]
    fn test_unsigned_resolution_is_not_replayed_when_signing_required() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            env.hitch
                .run()
                .args(&["add", "dev"])
                .execute()?
                .assert_success();

            // Build two branches that conflict on the same file. `hitch
            // promote` runs a pre-promote compatibility preflight that would
            // reject the second branch outright, so — exactly as
            // `resolve_tests.rs`'s `setup_and_record` does — the branches are
            // injected directly into `hitch.json` to get them both into the
            // environment's promoted list without going through that gate.
            for (branch, content) in [("feat-a", "A\n"), ("feat-b", "B\n")] {
                env.git.run(&["checkout", "main"])?.assert_success();
                env.git.run(&["checkout", "-b", branch])?.assert_success();
                env.fs.write_file("clash.txt", content)?;
                env.git.run(&["add", "."])?.assert_success();
                env.git
                    .run(&["commit", "-m", &format!("{} edits clash.txt", branch)])?
                    .assert_success();
                env.git.run(&["checkout", "main"])?.assert_success();
            }

            env.git
                .run(&["checkout", "hitch-metadata"])?
                .assert_success();
            let raw = env.fs.read_file("hitch.json")?;
            let mut config: serde_json::Value = serde_json::from_str(&raw)?;
            config["require_signed_resolutions"] = serde_json::Value::Bool(true);
            config["environments"]["dev"]["branches"] = serde_json::json!(["feat-a", "feat-b"]);
            env.fs
                .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
            env.git.run(&["add", "hitch.json"])?.assert_success();
            env.git
                .run(&["commit", "-m", "test: require signed resolutions"])?
                .assert_success();
            env.git.run(&["checkout", "main"])?.assert_success();

            // Rebuild with replay enabled. There is no signed resolution, so
            // the conflicting branch must be held, not silently composed.
            let result = env
                .hitch
                .run()
                .args(&[
                    "--no-push",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                    "--yes",
                ])
                .execute()?;

            let combined = format!("{}{}", result.stdout(), result.stderr());
            assert!(
                !combined.contains("Applying recorded resolution"),
                "an unsigned resolution was replayed while signing was required:\n{}",
                combined
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// Set up `dev` with `branch-a`/`branch-b` conflicting on `shared.txt` and
    /// start a Mode B resolve session on `branch-b`, staging `resolved-both`
    /// as the fix. Returns the resolve worktree path so the caller can decide
    /// whether to configure SSH signing before `--continue --record`.
    fn setup_conflict_and_start_resolve(
        env: &TestEnvironment,
    ) -> anyhow::Result<std::path::PathBuf> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.fs.write_file("shared.txt", "base\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?.assert_success();
        env.git.run(&["commit", "-m", "base"])?.assert_success();

        env.git
            .run(&["checkout", "-b", "branch-a"])?
            .assert_success();
        env.fs.write_file("shared.txt", "from-a\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?.assert_success();
        env.git.run(&["commit", "-m", "a"])?.assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();

        env.git
            .run(&["checkout", "-b", "branch-b"])?
            .assert_success();
        env.fs.write_file("shared.txt", "from-b\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?.assert_success();
        env.git.run(&["commit", "-m", "b"])?.assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();

        env.git
            .run(&["checkout", "hitch-metadata"])?
            .assert_success();
        let config_str = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
        config["environments"]["dev"]["branches"] = serde_json::json!(["branch-a", "branch-b"]);
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?.assert_success();
        env.git
            .run(&["commit", "-m", "test: inject branches"])?
            .assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();

        env.hitch
            .run()
            .args(&["--no-push", "resolve", "dev", "--branch", "branch-b"])
            .execute()?
            .assert_success();

        let worktree_path = env.temp_dir.parent().unwrap().join(format!(
            ".hitch-resolve-{}-dev-branch-b",
            env.temp_dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(worktree_path.join("shared.txt"), "resolved-both\n")?;
        // Test-only: simulates a user staging their edited file by hand in
        // the resolve worktree, so it deliberately spawns plain git rather
        // than going through GitOperations.
        #[allow(clippy::disallowed_methods)]
        std::process::Command::new("git")
            .args(["add", "shared.txt"])
            .current_dir(&worktree_path)
            .stdin(std::process::Stdio::null())
            .status()?;

        Ok(worktree_path)
    }

    /// Flip `require_signed_resolutions` on in `hitch.json`, from `main`,
    /// leaving the checkout back on `main` when done.
    fn require_signed_resolutions(env: &TestEnvironment) -> anyhow::Result<()> {
        env.git
            .run(&["checkout", "hitch-metadata"])?
            .assert_success();
        let raw = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&raw)?;
        config["require_signed_resolutions"] = serde_json::Value::Bool(true);
        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?.assert_success();
        env.git
            .run(&["commit", "-m", "test: require signed resolutions"])?
            .assert_success();
        env.git.run(&["checkout", "main"])?.assert_success();
        Ok(())
    }

    /// Generate a throwaway SSH keypair (never the host's real identity) in
    /// `dir` and configure this repo to sign with it and trust it, under the
    /// given principal name. Returns the allowed-signers file path.
    fn configure_ssh_signing(
        env: &TestEnvironment,
        dir: &std::path::Path,
        principal: &str,
    ) -> anyhow::Result<std::path::PathBuf> {
        std::fs::create_dir_all(dir)?;
        let key_path = dir.join("id_hitch_test");

        // ssh-keygen is not git; test-only throwaway key generation, not part
        // of hitch's own subprocess surface.
        #[allow(clippy::disallowed_methods)]
        let keygen = std::process::Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-f",
                &key_path.to_string_lossy(),
                "-N",
                "",
                "-C",
                principal,
                "-q",
            ])
            .stdin(std::process::Stdio::null())
            .output()?;
        assert!(
            keygen.status.success(),
            "failed to generate test SSH keypair: {}",
            String::from_utf8_lossy(&keygen.stderr)
        );

        let pub_key_path = dir.join("id_hitch_test.pub");
        let pub_key = std::fs::read_to_string(&pub_key_path)?;
        let allowed_signers_path = dir.join("allowed_signers");
        std::fs::write(&allowed_signers_path, format!("{} {}", principal, pub_key))?;

        env.git
            .run(&["config", "gpg.format", "ssh"])?
            .assert_success();
        env.git
            .run(&["config", "user.signingkey", &pub_key_path.to_string_lossy()])?
            .assert_success();
        env.git
            .run(&[
                "config",
                "gpg.ssh.allowedSignersFile",
                &allowed_signers_path.to_string_lossy(),
            ])?
            .assert_success();

        Ok(allowed_signers_path)
    }

    /// A resolution recorded with no SSH signing key configured has no
    /// signature (`sign_bytes_ssh` returns `None`). When the repository
    /// later requires signed resolutions, that unsigned resolution must be
    /// held rather than replayed — this is the exact attack the feature
    /// exists to close: `refs/hitch/resolutions/*` is writable by anyone
    /// with push access, and `recorded_by` alone is self-reported.
    #[test]
    fn test_unsigned_recorded_resolution_is_held_when_signing_required() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflict_and_start_resolve(env)?;

            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "resolve",
                    "dev",
                    "--branch",
                    "branch-b",
                    "--continue",
                    "--record",
                ])
                .execute()?
                .assert_success();

            require_signed_resolutions(env)?;

            let result = env
                .hitch
                .run()
                .args(&[
                    "--no-push",
                    "--yes",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                ])
                .execute()?;

            let combined = format!("{}{}", result.stdout(), result.stderr());
            assert!(
                !combined.contains("Reused recorded resolution"),
                "an unsigned resolution was replayed while signing was required:\n{}",
                combined
            );
            assert!(
                combined.contains("held") || combined.contains("not signed"),
                "expected 'branch-b' to be held, not replayed:\n{}",
                combined
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// The positive case: a resolution signed with a key present in the
    /// repository's allowed-signers file replays cleanly under
    /// `require_signed_resolutions` — proving the gate is a real signature
    /// check, not a blanket "never replay recorded resolutions".
    #[test]
    fn test_signed_recorded_resolution_replays_when_signer_is_trusted() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let keys_dir = sibling_path(env, "ssh-keys-trusted");
            configure_ssh_signing(env, &keys_dir, "hitch-test@example.com")?;
            env.git
                .run(&["config", "user.email", "hitch-test@example.com"])?
                .assert_success();

            setup_conflict_and_start_resolve(env)?;

            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "resolve",
                    "dev",
                    "--branch",
                    "branch-b",
                    "--continue",
                    "--record",
                ])
                .execute()?
                .assert_success();

            require_signed_resolutions(env)?;

            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "--yes",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                ])
                .execute()?
                .assert_success()
                .assert_stdout_contains("Reused recorded resolution");

            assert_eq!(
                env.git.run(&["show", "dev:shared.txt"])?.stdout().trim(),
                "resolved-both"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    /// A resolution signed by a real, verifiable key that is *not* the
    /// principal the repository's allowed-signers file trusts must still be
    /// held — an unknown signer is exactly as untrusted as no signature at
    /// all, never a partial pass.
    #[test]
    fn test_signed_recorded_resolution_is_held_when_signer_is_untrusted() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let keys_dir = sibling_path(env, "ssh-keys-untrusted");
            // The key signs as "hitch-test@example.com", but the
            // allowed-signers file only trusts a *different* principal — so
            // verification must fail even though the signature is otherwise
            // perfectly valid.
            configure_ssh_signing(env, &keys_dir, "hitch-test@example.com")?;
            let allowed_signers_path = keys_dir.join("allowed_signers");
            std::fs::write(
                &allowed_signers_path,
                format!(
                    "someone-else@example.com {}",
                    std::fs::read_to_string(keys_dir.join("id_hitch_test.pub"))?
                ),
            )?;
            env.git
                .run(&["config", "user.email", "hitch-test@example.com"])?
                .assert_success();

            setup_conflict_and_start_resolve(env)?;

            env.hitch
                .run()
                .args(&[
                    "--no-push",
                    "resolve",
                    "dev",
                    "--branch",
                    "branch-b",
                    "--continue",
                    "--record",
                ])
                .execute()?
                .assert_success();

            require_signed_resolutions(env)?;

            let result = env
                .hitch
                .run()
                .args(&[
                    "--no-push",
                    "--yes",
                    "rebuild",
                    "dev",
                    "--replay-resolutions",
                ])
                .execute()?;

            let combined = format!("{}{}", result.stdout(), result.stderr());
            assert!(
                !combined.contains("Reused recorded resolution"),
                "a resolution signed by an untrusted signer was replayed:\n{}",
                combined
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
