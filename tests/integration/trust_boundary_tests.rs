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
            let init = std::process::Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&bare_path)
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
            let init = std::process::Command::new("git")
                .args(["init", "--bare"])
                .current_dir(&bare_path)
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
}
