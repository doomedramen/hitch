//! Integration tests for shared rerere cache import/export.

#[cfg(test)]
mod tests {
    use crate::framework::TestSetup;
    use crate::test_framework::*;
    use std::path::PathBuf;

    /// Helper: inject two conflicting branches into hitch.json on hitch-metadata
    /// without going through `hitch promote` (which would block them via the
    /// pre-promote conflict check).
    fn inject_branches_into_metadata(
        env: &TestEnvironment,
        env_name: &str,
        branches: &[&str],
    ) -> anyhow::Result<()> {
        env.git.run(&["checkout", "hitch-metadata"])?;

        let config_str = env.fs.read_file("hitch.json")?;
        let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
        let branch_array = serde_json::Value::Array(
            branches
                .iter()
                .map(|b| serde_json::Value::String(b.to_string()))
                .collect(),
        );
        config["environments"][env_name]["branches"] = branch_array;

        env.fs
            .write_file("hitch.json", &serde_json::to_string_pretty(&config)?)?;
        env.git.run(&["add", "hitch.json"])?;
        env.git
            .run(&["commit", "-m", "test: inject conflicting branches"])?;
        env.git.run(&["checkout", "main"])?;
        Ok(())
    }

    fn setup_conflicting_rebuild(env: &TestEnvironment) -> anyhow::Result<()> {
        env.hitch
            .run()
            .args(&["add", "dev"])
            .execute()?
            .assert_success();

        env.fs.write_file("shared.txt", "base content\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git.run(&["commit", "-m", "Add shared.txt"])?;

        env.git.run(&["checkout", "-b", "branch-a"])?;
        env.fs.write_file("shared.txt", "from branch-a\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git
            .run(&["commit", "-m", "branch-a: update shared.txt"])?;
        env.git.run(&["checkout", "main"])?;

        env.git.run(&["checkout", "-b", "branch-b"])?;
        env.fs.write_file("shared.txt", "from branch-b\n")?;
        env.git.run(&["add", "-f", "shared.txt"])?;
        env.git
            .run(&["commit", "-m", "branch-b: update shared.txt"])?;
        env.git.run(&["checkout", "main"])?;

        inject_branches_into_metadata(env, "dev", &["branch-a", "branch-b"])?;

        Ok(())
    }

    fn setup_local_origin_remote(
        env: &TestEnvironment,
    ) -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
        // Keep the remote outside the repo working tree because Hitch uses `git clean -fd`
        // during some operations, which would delete untracked directories inside the repo.
        let remote_root = tempfile::tempdir()?;
        let remote_dir = remote_root.path().join("remote.git");

        env.git
            .run(&["init", "--bare", remote_dir.to_str().unwrap()])?
            .assert_success();
        env.git
            .run(&["remote", "add", "origin", remote_dir.to_str().unwrap()])?
            .assert_success();

        // Push all branches (main, hitch-metadata, and any test branches).
        env.git.run(&["push", "-u", "origin", "--all"])?;

        // Ensure clones check out `main` by default.
        env.git.run(&[
            "-C",
            remote_dir.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])?;

        Ok((remote_root, remote_dir))
    }

    #[test]
    fn test_reuse_resolutions_exports_and_imports_shared_rr_cache() -> anyhow::Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            setup_conflicting_rebuild(env)?;
            let (_remote_root, remote_dir) = setup_local_origin_remote(env)?;

            // Ensure rerere.enabled starts unset (best-effort).
            let _ = env.git.run(&["config", "--unset-all", "rerere.enabled"]);

            // Trigger rebuild — should fail due to conflict, leaving resolve state.
            env.hitch
                .run()
                .args(&["rebuild", "dev", "--reuse-resolutions"])
                .execute()?
                .assert_failure();

            // rerere.enabled should remain true during the pause.
            let rerere = env.git.run(&["config", "--get", "rerere.enabled"])?;
            assert!(rerere.success());
            assert_eq!(rerere.stdout().trim(), "true");

            // Resolve-state should indicate reuse_resolutions.
            let state_json = env
                .fs
                .read_file(".git/hitch-resolve-state.json")
                .expect("resolve state should exist");
            let state: hitch::utils::resolve_state::ResolveState =
                serde_json::from_str(&state_json)?;
            assert!(state.reuse_resolutions);

            // Manually resolve the conflict and stage it.
            env.fs.write_file("shared.txt", "resolved\n")?;
            env.git.run(&["add", "-f", "shared.txt"])?;

            // Ensure rr-cache has something to export (don't depend on git rerere internals).
            env.fs.write_file(".git/rr-cache/abcd/preimage", "pre\n")?;
            env.fs
                .write_file(".git/rr-cache/abcd/postimage", "post\n")?;

            // Continue rebuild (this triggers export + restores rerere.enabled).
            env.hitch
                .run()
                .args(&["resolve", "--continue"])
                .execute()?
                .assert_success();

            // rerere.enabled should be restored (unset).
            let rerere_after = env.git.run(&["config", "--get", "rerere.enabled"])?;
            assert!(
                !rerere_after.success() || rerere_after.stdout().trim().is_empty(),
                "rerere.enabled should be restored/unset after continue"
            );

            // Export should have committed into hitch-metadata.
            env.git.run(&["checkout", "hitch-metadata"])?;
            assert!(
                env.temp_dir.join("hitch/rr-cache/index.json").exists(),
                "index.json should be present on hitch-metadata"
            );
            assert!(
                env.temp_dir
                    .join("hitch/rr-cache/entries/abcd/preimage")
                    .exists(),
                "exported rr-cache preimage should be present on hitch-metadata"
            );
            env.git.run(&["checkout", "main"])?;

            // Push updated hitch-metadata to origin so a second clone can import.
            env.git.run(&["push", "origin", "hitch-metadata"])?;

            // Clone into a fresh directory and verify import copies files into .git/rr-cache.
            let clone_root = tempfile::tempdir()?;
            let clone_path = clone_root.path().join("repo");
            let clone_git_parent = GitCommandRunner::new(clone_root.path())?;
            clone_git_parent
                .run(&[
                    "clone",
                    remote_dir.to_str().unwrap(),
                    clone_path.to_str().unwrap(),
                ])?
                .assert_success();
            let clone_git = GitCommandRunner::new(&clone_path)?;

            // Need a local hitch-metadata branch for hitch health checks.
            clone_git.run(&["checkout", "-b", "hitch-metadata", "origin/hitch-metadata"])?;
            clone_git.run(&["checkout", "main"])?;

            // Run rebuild to trigger import. It will conflict, but import should have run first.
            env.hitch
                .run()
                .current_dir(&clone_path)
                .args(&["rebuild", "dev", "--reuse-resolutions"])
                .execute()?;

            assert!(
                clone_path.join(".git/rr-cache/abcd/preimage").exists(),
                "import should copy shared rr-cache into clone's .git/rr-cache"
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
