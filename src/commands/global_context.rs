use crate::utils::git_operations::GitOperations;
use crate::utils::logging::Logger;
use crate::utils::progress::{ConsoleProgressReporter, ProgressReporter};
use colored::*;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    pub git_ops: Rc<GitOperations>,
    pub progress_reporter: Arc<dyn ProgressReporter>,
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
        let progress_reporter: Arc<dyn ProgressReporter> = if verbose {
            Arc::new(ConsoleProgressReporter::new(true))
        } else {
            Arc::new(ConsoleProgressReporter::new(false))
        };
        Ok(GlobalContext {
            verbose,
            no_push,
            git_ops,
            progress_reporter,
            logger,
        })
    }

    /// Create a new GlobalContext for testing (no-op progress reporter)
    #[cfg(test)]
    pub fn new_test(verbose: bool, no_push: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let _git_ops = Rc::new(GitOperations::new()?);
        let _progress_reporter: Arc<dyn ProgressReporter> =
            Arc::new(crate::utils::progress::NoOpProgressReporter);
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

    /// Report progress for long-running operations
    pub fn report_progress(&self, progress: &crate::utils::progress::ProgressInfo) {
        self.progress_reporter.report(progress);
    }
}
