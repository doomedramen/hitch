//! Red-team tests: each one performs an actual attack against the built
//! binary and asserts hitch is inert. These are regression guards for the
//! trust-boundary work, not documentation.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;

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
}
