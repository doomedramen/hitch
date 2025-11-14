use std::sync::Arc;
use crate::utils::git_operations::GitOperations;
use colored::*;

#[derive(Debug, Clone)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    pub git_ops: Arc<GitOperations>,
}

impl GlobalContext {
    pub fn new(verbose: bool, no_push: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Arc::new(GitOperations::new()?);
        Ok(GlobalContext {
            verbose,
            no_push,
            git_ops,
        })
    }

    pub fn git(&self) -> &GitOperations {
        &self.git_ops
    }

    pub fn log_verbose(&self, message: &str) {
        if self.verbose {
            println!("{}", message);
        }
    }

    pub fn log_info(&self, message: &str) {
        println!("{} {}", "ℹ️".blue(), message);
    }

    pub fn log_success(&self, message: &str) {
        println!("{} {}", "✅".green(), message);
    }

    pub fn log_warning(&self, message: &str) {
        println!("{} {}", "⚠️".yellow(), message);
    }

    pub fn log_error(&self, message: &str) {
        eprintln!("{} {}", "❌".red(), message);
    }

    pub fn should_push(&self) -> bool {
        !self.no_push
    }
}