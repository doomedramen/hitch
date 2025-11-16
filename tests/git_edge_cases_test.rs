use anyhow::Result;
use std::process::Command;

// Import the proper test framework
mod common;
use common::{with_test_env, SetupLevel, TestEnv};

#[cfg(test)]
#[allow(unused_variables)]
#[allow(dead_code)]
mod git_edge_cases_tests {
    use super::*;

    /// Helper to ensure working tree is clean before hitch operations
    fn ensure_clean_working_tree(test_env: &TestEnv) -> Result<()> {
        // Clean up any existing changes first
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(test_env.path())
            .output()?;

        let status_str = String::from_utf8_lossy(&status_output.stdout);

        if !status_str.trim().is_empty() {
            // There are uncommitted changes, add and commit them
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(test_env.path())
                .output()?;

            let commit_output = Command::new("git")
                .args(["commit", "-m", "Clean up test environment"])
                .current_dir(test_env.path())
                .output()?;

            // Don't treat "nothing to commit" as an error
            if !commit_output.status.success() {
                let stderr = String::from_utf8_lossy(&commit_output.stderr);
                let stdout = String::from_utf8_lossy(&commit_output.stdout);
                if !(stderr.contains("nothing to commit") || stdout.contains("nothing to commit")) {
                    return Err(anyhow::anyhow!(
                        "Failed to commit: stderr={}, stdout={}",
                        stderr,
                        stdout
                    ));
                }
            }
        }

        Ok(())
    }

    /// Helper to clean up after hitch init (it leaves the working tree dirty)
    fn cleanup_after_hitch_init(test_env: &TestEnv) -> Result<()> {
        // Check git status after hitch init
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(test_env.path())
            .output()?;

        let status_str = String::from_utf8_lossy(&status_output.stdout);

        if !status_str.trim().is_empty() {
            // Hitch init leaves changes (hitch.json), commit them
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(test_env.path())
                .output()?;

            Command::new("git")
                .args(["commit", "-m", "Add hitch configuration"])
                .current_dir(test_env.path())
                .output()?;
        }

        Ok(())
    }

    /// Helper to run hitch command in test environment
    fn run_hitch_command(test_env: &TestEnv, args: &[&str]) -> Result<std::process::Output> {
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .output()?;

        Ok(output)
    }

    /// Helper to run hitch command and expect failure
    fn run_hitch_command_expect_failure(
        test_env: &TestEnv,
        args: &[&str],
    ) -> Result<std::process::Output> {
        let binary_path = test_env.hitch_binary();
        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(test_env.path())
            .output()?;

        if output.status.success() {
            return Err(anyhow::anyhow!(
                "Expected hitch command to fail, but it succeeded: hitch {}",
                args.join(" ")
            ));
        }

        Ok(output)
    }

    /// Helper to create and commit a file
    fn create_and_commit_file(test_env: &TestEnv, filename: &str, content: &str) -> Result<()> {
        let file_path = test_env.path().join(filename);
        std::fs::write(file_path, content)?;

        Command::new("git")
            .args(["add", filename])
            .current_dir(test_env.path())
            .output()?;

        Command::new("git")
            .args(["commit", "-m", &format!("Add {}", filename)])
            .current_dir(test_env.path())
            .output()?;

        Ok(())
    }

    /// Test Hitch behavior in detached HEAD state
    #[test]
    fn test_hitch_in_detached_head_state() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a commit and checkout detached HEAD
            create_and_commit_file(test_env, "test.txt", "test content")?;
            let commit_output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(test_env.path())
                .output()?;
            let commit_hash_owned = String::from_utf8_lossy(&commit_output.stdout);
            let commit_hash = commit_hash_owned.trim();

            Command::new("git")
                .args(["checkout", commit_hash])
                .current_dir(test_env.path())
                .output()?;

            // Verify we're in detached HEAD state
            let status_output = Command::new("git")
                .args(["status", "--porcelain=v1", "--branch"])
                .current_dir(test_env.path())
                .output()?;
            let status_str = String::from_utf8_lossy(&status_output.stdout);
            assert!(status_str.contains("HEAD detached") || status_str.contains("no branch"));

            // Try to add environment in detached HEAD state
            let output = run_hitch_command_expect_failure(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about not being on a branch
            assert!(
                stderr.contains("branch")
                    || stderr.contains("HEAD")
                    || stderr.contains("detached")
                    || stderr.contains("checkout")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with uncommitted changes
    #[test]
    fn test_hitch_with_uncommitted_changes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create uncommitted changes
            std::fs::write(test_env.path().join("dirty.txt"), "dirty content")?;

            // Try to add environment with uncommitted changes
            let output = run_hitch_command_expect_failure(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about dirty working tree
            assert!(
                stderr.contains("clean")
                    || stderr.contains("commit")
                    || stderr.contains("stashed")
                    || stderr.contains("changes")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with staged but uncommitted changes
    #[test]
    fn test_hitch_with_staged_changes() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create staged changes
            std::fs::write(test_env.path().join("staged.txt"), "staged content")?;
            Command::new("git")
                .args(["add", "staged.txt"])
                .current_dir(test_env.path())
                .output()?;

            // Try to add environment with staged changes
            let output = run_hitch_command_expect_failure(test_env, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about staged changes
            assert!(
                stderr.contains("clean")
                    || stderr.contains("commit")
                    || stderr.contains("staged")
                    || stderr.contains("changes")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with git worktrees
    #[test]
    fn test_hitch_with_git_worktrees() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a worktree
            let worktree_path = test_env.path().join("worktree");
            std::fs::create_dir_all(&worktree_path)?;

            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-b",
                    "worktree-branch",
                    worktree_path.to_str().unwrap(),
                ])
                .current_dir(test_env.path())
                .output()?;

            // Try to run hitch command from worktree
            let output =
                run_hitch_command_expect_failure_from_path(&worktree_path, &["add", "dev"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Hitch should work from worktrees or give clear error
            // The exact behavior depends on implementation
            assert!(
                stderr.contains("worktree")
                    || stderr.contains("main")
                    || stderr.contains("repository")
                    || output.status.success()
            ); // Or it might work fine

            Ok(())
        })
    }

    /// Helper to run hitch command from specific path
    fn run_hitch_command_expect_failure_from_path(
        path: &std::path::Path,
        args: &[&str],
    ) -> Result<std::process::Output> {
        let binary_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("hitch");

        let output = Command::new(&binary_path)
            .args(args)
            .current_dir(path)
            .output()?;

        if output.status.success() {
            return Err(anyhow::anyhow!(
                "Expected hitch command to fail, but it succeeded: hitch {}",
                args.join(" ")
            ));
        }

        Ok(output)
    }

    /// Test Hitch behavior with empty repository (no commits)
    #[test]
    fn test_hitch_with_empty_repository() -> Result<()> {
        with_test_env(SetupLevel::Basic, |test_env| {
            // Initialize empty git repository
            Command::new("git")
                .args(["init"])
                .current_dir(test_env.path())
                .output()?;

            // Try to initialize Hitch in empty repository
            let output = run_hitch_command_expect_failure(test_env, &["init"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about empty repository or missing commits
            assert!(
                stderr.contains("commit")
                    || stderr.contains("empty")
                    || stderr.contains("HEAD")
                    || stderr.contains("exists")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with bare repository
    #[test]
    fn test_hitch_with_bare_repository() -> Result<()> {
        with_test_env(SetupLevel::Basic, |test_env| {
            // Initialize bare git repository
            Command::new("git")
                .args(["init", "--bare"])
                .current_dir(test_env.path())
                .output()?;

            // Try to initialize Hitch in bare repository
            let output = run_hitch_command_expect_failure(test_env, &["init"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about bare repository
            assert!(
                stderr.contains("bare")
                    || stderr.contains("working tree")
                    || stderr.contains("not supported")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with corrupted git repository
    #[test]
    fn test_hitch_with_corrupted_git_repository() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Corrupt the git repository by removing .git/HEAD
            let git_head_path = test_env.path().join(".git").join("HEAD");
            std::fs::remove_file(&git_head_path)?;

            // Try to initialize Hitch in corrupted repository
            let output = run_hitch_command_expect_failure(test_env, &["init"])?;

            let stderr = String::from_utf8_lossy(&output.stderr);

            // Should show error about corrupted repository
            assert!(
                stderr.contains("repository")
                    || stderr.contains("corrupted")
                    || stderr.contains("git")
                    || stderr.contains("HEAD")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with submodules
    #[test]
    fn test_hitch_with_git_submodules() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Add a submodule (this might not work in test environment, but we test the behavior)
            let submodule_path = test_env.path().join("submodule");
            std::fs::create_dir_all(&submodule_path)?;

            Command::new("git")
                .args(["init"])
                .current_dir(&submodule_path)
                .output()?;

            create_and_commit_file_from_path(&submodule_path, "sub.txt", "sub content")?;

            let _submodule_add_output = Command::new("git")
                .args(["submodule", "add", "./submodule"])
                .current_dir(test_env.path())
                .output();

            // Try to add environment - Hitch should handle submodules gracefully
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Either succeeds or gives clear error about submodules
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            assert!(
                output.status.success()
                    || stderr.contains("submodule")
                    || stderr.contains("nested")
            );

            Ok(())
        })
    }

    /// Helper to create and commit file from specific path
    fn create_and_commit_file_from_path(
        path: &std::path::Path,
        filename: &str,
        content: &str,
    ) -> Result<()> {
        let file_path = path.join(filename);
        std::fs::write(file_path, content)?;

        Command::new("git")
            .args(["add", filename])
            .current_dir(path)
            .output()?;

        Command::new("git")
            .args(["commit", "-m", &format!("Add {}", filename)])
            .current_dir(path)
            .output()?;

        Ok(())
    }

    /// Test Hitch behavior with large number of branches
    #[test]
    fn test_hitch_with_many_branches() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create many branches
            for i in 0..50 {
                create_and_commit_file(
                    test_env,
                    &format!("file{}.txt", i),
                    &format!("content {}", i),
                )?;
                Command::new("git")
                    .args(["checkout", "-b", &format!("branch-{}", i)])
                    .current_dir(test_env.path())
                    .output()?;
                Command::new("git")
                    .args(["checkout", "main"])
                    .current_dir(test_env.path())
                    .output()?;
            }

            // Try to add environment with many branches
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Should handle many branches gracefully
            assert!(output.status.success());

            Ok(())
        })
    }

    /// Test Hitch behavior with git tags
    #[test]
    fn test_hitch_with_git_tags() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create some commits and tags
            create_and_commit_file(test_env, "v1.txt", "version 1")?;
            Command::new("git")
                .args(["tag", "v1.0.0"])
                .current_dir(test_env.path())
                .output()?;

            create_and_commit_file(test_env, "v2.txt", "version 2")?;
            Command::new("git")
                .args(["tag", "v2.0.0"])
                .current_dir(test_env.path())
                .output()?;

            // Try to add environment with tags present
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Should handle tags gracefully
            assert!(output.status.success());

            Ok(())
        })
    }

    /// Test Hitch behavior when on a non-main branch
    #[test]
    fn test_hitch_on_non_main_branch() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create and checkout a non-main branch
            Command::new("git")
                .args(["checkout", "-b", "feature-branch"])
                .current_dir(test_env.path())
                .output()?;

            // Try to add environment from non-main branch
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Should work from any branch or give clear warning
            assert!(output.status.success());

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Might warn about not being on main branch
            assert!(
                output.status.success()
                    || stdout.contains("main")
                    || stderr.contains("main")
                    || stdout.contains("branch")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with git hooks present
    #[test]
    fn test_hitch_with_git_hooks() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a pre-commit hook that always fails
            let hooks_dir = test_env.path().join(".git").join("hooks");
            std::fs::create_dir_all(&hooks_dir)?;

            let pre_commit_hook = hooks_dir.join("pre-commit");
            std::fs::write(&pre_commit_hook, "#!/bin/sh\necho 'Hook executed'; exit 1")?;

            // Make hook executable on Unix systems
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&pre_commit_hook)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&pre_commit_hook, perms)?;
            }

            // Try to add environment with failing pre-commit hook
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Hitch should handle git hooks gracefully
            // The exact behavior depends on how git hooks interact with Hitch
            assert!(
                output.status.success() || String::from_utf8_lossy(&output.stderr).contains("hook")
            );

            Ok(())
        })
    }

    /// Test Hitch behavior with git LFS files
    #[test]
    fn test_hitch_with_git_lfs_files() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create a .gitattributes file for LFS
            std::fs::write(
                test_env.path().join(".gitattributes"),
                "*.lfs filter=lfs diff=lfs merge=lfs -text",
            )?;

            // Create a large file that would be tracked by LFS
            let large_content = "x".repeat(1000);
            std::fs::write(test_env.path().join("large.lfs"), &large_content)?;

            // Try to add environment with LFS files
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Should handle LFS files gracefully (may warn about large files)
            assert!(output.status.success());

            Ok(())
        })
    }

    /// Test Hitch behavior with binary files
    #[test]
    fn test_hitch_with_binary_files() -> Result<()> {
        with_test_env(SetupLevel::GitOnly, |test_env| {
            // Ensure working tree is clean and initialize Hitch
            ensure_clean_working_tree(test_env)?;
            run_hitch_command(test_env, &["init"])?;
            cleanup_after_hitch_init(test_env)?;

            // Create binary files
            let binary_content = vec![0u8; 100];
            std::fs::write(test_env.path().join("binary.bin"), &binary_content)?;

            // Try to add environment with binary files
            let output = run_hitch_command(test_env, &["add", "dev"])?;

            // Should handle binary files gracefully
            assert!(output.status.success());

            Ok(())
        })
    }
}
