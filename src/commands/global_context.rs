use crate::utils::git_operations::GitOperations;
use crate::utils::logging::Logger;
use crate::utils::progress::{ConsoleProgressReporter, ProgressReporter};
use colored::*;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    pub git_ops: Rc<GitOperations>,
    pub progress_reporter: Arc<dyn ProgressReporter>,
    pub logger: Arc<Logger>,
    /// Active progress reporter for suspend/resume during logging
    active_progress: Arc<RwLock<Option<Arc<ConsoleProgressReporter>>>>,
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
            active_progress: Arc::new(RwLock::new(None)),
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

    /// Set the active progress reporter for suspend/resume during logging
    pub fn set_active_progress(&self, reporter: Arc<ConsoleProgressReporter>) {
        *self.active_progress.write().unwrap() = Some(reporter);
    }

    /// Clear the active progress reporter
    pub fn clear_active_progress(&self) {
        *self.active_progress.write().unwrap() = None;
    }

    /// Suspend the active progress bar (clears the line)
    /// Use this before printing content that should appear cleanly without the progress bar
    pub fn suspend_progress(&self) {
        if let Some(ref reporter) = *self.active_progress.read().unwrap() {
            reporter.suspend();
        }
    }

    /// Resume the active progress bar (redraws it)
    /// Use this after printing content when you want the progress bar to reappear
    pub fn resume_progress(&self) {
        if let Some(ref reporter) = *self.active_progress.read().unwrap() {
            reporter.resume();
        }
    }

    pub fn log_verbose(&self, message: &str) {
        if self.verbose {
            self.suspend_progress();
            println!("{}", message);
            self.resume_progress();
        }
    }

    pub fn log_info(&self, message: &str) {
        self.suspend_progress();
        println!("{} {}", "ℹ️".blue(), message);
        self.resume_progress();
    }

    pub fn log_success(&self, message: &str) {
        self.suspend_progress();
        println!("{} {}", "✅".green(), message);
        self.resume_progress();
    }

    pub fn log_warning(&self, message: &str) {
        self.suspend_progress();
        println!("{} {}", "⚠️".yellow(), message);
        self.resume_progress();
    }

    pub fn log_error(&self, message: &str) {
        self.suspend_progress();
        eprintln!("{} {}", "❌".red(), message);
        self.resume_progress();
    }

    pub fn should_push(&self) -> bool {
        !self.no_push
    }

    /// Report progress for long-running operations
    pub fn report_progress(&self, progress: &crate::utils::progress::ProgressInfo) {
        self.progress_reporter.report(progress);
    }
}
