use anyhow::{Context, Result};
use colored::*;
use git2::{Repository, ObjectType, Oid};
use std::process::Command;

pub struct GitOperations {
    repo: Repository,
}

impl GitOperations {
    pub fn new() -> Result<Self> {
        let repo = Repository::discover(".")
            .context("Not in a git repository")?;
        Ok(GitOperations { repo })
    }

    pub fn get_current_branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        let name = head.shorthand()
            .ok_or_else(|| anyhow::anyhow!("HEAD is not on a branch"))?;
        Ok(name.to_string())
    }

    pub fn checkout_branch(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(&["checkout", branch])
            .output()
            .context(format!("Failed to checkout branch '{}'", branch))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to checkout branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn create_orphan_branch(&self, branch_name: &str) -> Result<()> {
        let output = Command::new("git")
            .args(&["checkout", "--orphan", branch_name])
            .output()
            .context(format!("Failed to create orphan branch '{}'", branch_name))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create orphan branch '{}': {}",
                branch_name,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Clean working directory
        let _ = Command::new("git")
            .args(&["rm", "-rf", "."])
            .output()?;

        Ok(())
    }

    pub fn add_and_commit(&self, files: &[&str], message: &str) -> Result<()> {
        // Add files
        for file in files {
            let output = Command::new("git")
                .args(&["add", file])
                .output()
                .context(format!("Failed to add file '{}'", file))?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to add file '{}': {}",
                    file,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        // Commit
        let output = Command::new("git")
            .args(&["commit", "-m", message])
            .output()
            .context("Failed to commit files")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to commit: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn read_file_from_branch(&self, branch: &str, file: &str) -> Result<String> {
        let output = Command::new("git")
            .args(&["show", &format!("{}:{}", branch, file)])
            .output()
            .context(format!("Failed to read '{}' from branch '{}'", file, branch))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "File '{}' not found in branch '{}'",
                file,
                branch
            ));
        }

        let content = String::from_utf8(output.stdout)
            .context("Failed to parse file content")?;

        Ok(content)
    }

    pub fn write_file(&self, file: &str, content: &str) -> Result<()> {
        std::fs::write(file, content)
            .context(format!("Failed to write file '{}'", file))?;
        Ok(())
    }

    pub fn fetch_branch(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(&["fetch", "origin", branch])
            .output()
            .context(format!("Failed to fetch branch '{}'", branch))?;

        if !output.status.success() {
            // Don't fail if fetch fails (might not exist remotely)
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("couldn't find remote ref") && !stderr.contains("no such remote ref") {
                return Err(anyhow::anyhow!("Fetch failed: {}", stderr));
            }
        }

        Ok(())
    }

    pub fn push_branch(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(&["push", "origin", branch])
            .output()
            .context(format!("Failed to push branch '{}'", branch))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to push branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(&["branch", "--list", branch])
            .output()
            .context("Failed to list branches")?;

        Ok(output.status.success() && !output.stdout.is_empty())
    }

    pub fn is_working_directory_clean(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(&["status", "--porcelain"])
            .output()
            .context("Failed to check git status")?;

        Ok(output.stdout.is_empty())
    }

    pub fn get_user_email(&self) -> Result<String> {
        let output = Command::new("git")
            .args(&["config", "user.email"])
            .output()
            .context("Failed to get user email")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Git user.email not configured"));
        }

        let email = String::from_utf8(output.stdout)
            .context("Failed to parse user email")?;
        Ok(email.trim().to_string())
    }
}