use crate::utils::confirm::{AlwaysYesConfirm, Confirm, StdinConfirm};
use crate::utils::git_operations::GitOperations;
use crate::utils::logging::Logger;
use crate::utils::output::{ConsoleOutputSink, OutputLevel, OutputSink};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    /// Answer every confirmation prompt with "yes" (global `--yes` / `HITCH_YES=1`).
    pub assume_yes: bool,
    pub git_ops: Rc<GitOperations>,
    pub logger: Arc<Logger>,
    pub output: Arc<dyn OutputSink>,
    pub confirm: Arc<dyn Confirm>,
}

#[allow(dead_code)]
impl GlobalContext {
    pub fn new(
        verbose: bool,
        no_push: bool,
        assume_yes: bool,
        logger: Arc<Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Rc::new(GitOperations::new()?);
        Ok(GlobalContext {
            verbose,
            no_push,
            assume_yes,
            git_ops,
            logger,
            output: Arc::new(ConsoleOutputSink),
            confirm: default_confirm(assume_yes),
        })
    }

    pub fn new_at_path(
        repo_path: &str,
        verbose: bool,
        no_push: bool,
        assume_yes: bool,
        logger: Arc<Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Rc::new(GitOperations::new_at_path(repo_path)?);
        Ok(GlobalContext {
            verbose,
            no_push,
            assume_yes,
            git_ops,
            logger,
            output: Arc::new(ConsoleOutputSink),
            confirm: default_confirm(assume_yes),
        })
    }

    pub fn with_output(mut self, output: Arc<dyn OutputSink>) -> Self {
        self.output = output;
        self
    }

    pub fn with_confirm(mut self, confirm: Arc<dyn Confirm>) -> Self {
        self.confirm = confirm;
        self
    }

    /// Create a new GlobalContext for testing
    #[cfg(test)]
    pub fn new_test(verbose: bool, no_push: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let logger = Arc::new(Logger::for_command("test", verbose));
        Self::new(verbose, no_push, false, logger)
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
        self.output.log(OutputLevel::Info, message);
    }

    pub fn log_success(&self, message: &str) {
        self.output.log(OutputLevel::Success, message);
    }

    pub fn log_warning(&self, message: &str) {
        self.output.log(OutputLevel::Warning, message);
    }

    pub fn log_error(&self, message: &str) {
        self.output.log(OutputLevel::Error, message);
    }

    pub fn should_push(&self) -> bool {
        !self.no_push
    }

    /// Ask the user to confirm a destructive action.
    ///
    /// Returns `Ok(true)` when the action may proceed. With `--yes` this never
    /// prompts; without a terminal it errors instead of blocking on stdin.
    pub fn confirm(&self, prompt: &str) -> Result<bool, anyhow::Error> {
        self.confirm.confirm(prompt)
    }
}

fn default_confirm(assume_yes: bool) -> Arc<dyn Confirm> {
    if assume_yes {
        Arc::new(AlwaysYesConfirm)
    } else {
        Arc::new(StdinConfirm)
    }
}
