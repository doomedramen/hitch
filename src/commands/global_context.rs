use crate::utils::git_operations::GitOperations;
use crate::utils::logging::Logger;
use colored::*;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    pub git_ops: Rc<GitOperations>,
    pub logger: Arc<Logger>,
}

#[allow(dead_code)]
impl GlobalContext {
    pub fn new(
        verbose: bool,
        no_push: bool,
        logger: Arc<Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Rc::new(GitOperations::new()?);
        Ok(GlobalContext {
            verbose,
            no_push,
            git_ops,
            logger,
        })
    }

    /// Create a new GlobalContext for testing
    #[cfg(test)]
    pub fn new_test(verbose: bool, no_push: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let logger = Arc::new(Logger::for_command("test", verbose));
        Self::new(verbose, no_push, logger)
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
