use crate::utils::confirm::{AlwaysYesConfirm, Confirm, StdinConfirm};
use crate::utils::git_operations::GitOperations;
use crate::utils::logging::Logger;
use crate::utils::output::{ConsoleOutputSink, DiagnosticOutputSink, OutputLevel, OutputSink};
use std::rc::Rc;
use std::sync::Arc;

/// The global flags, named.
///
/// A struct rather than four positional `bool`s because a call site like
/// `new_at_path(path, false, true, true, logger)` is unreadable in a way that
/// hides real mistakes: transposing two of the middle arguments compiles, type
/// checks, and produces a context that logs verbosely and confirms
/// interactively. The fields say which is which, and the compiler catches the
/// transposition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobalFlags {
    pub verbose: bool,
    pub no_push: bool,
    /// Answer every confirmation prompt with "yes" (global `--yes` /
    /// `HITCH_YES=1`).
    pub assume_yes: bool,
    /// Emit a machine-readable document on stdout and nothing else.
    pub json: bool,
}

impl GlobalFlags {
    /// The settings for an in-process caller that is not a CLI invocation —
    /// the Tauri backend, and anything else embedding the library.
    pub fn defaults() -> Self {
        Self::default()
    }

    /// The settings every in-repo unit test wants.
    ///
    /// `no_push` and `assume_yes` because there is no origin and no TTY, and
    /// the tests that need either say so. One named value rather than four
    /// literals repeated at every call site, because the four literals were
    /// copied from each other and a change to one of them would have been a
    /// change to eleven lines that looked unrelated.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            verbose: false,
            no_push: true,
            assume_yes: true,
            json: false,
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalContext {
    pub verbose: bool,
    pub no_push: bool,
    /// Answer every confirmation prompt with "yes" (global `--yes` / `HITCH_YES=1`).
    pub assume_yes: bool,
    /// stdout is reserved for a document. See [`GlobalFlags::json`].
    pub json: bool,
    pub git_ops: Rc<GitOperations>,
    pub logger: Arc<Logger>,
    pub output: Arc<dyn OutputSink>,
    pub confirm: Arc<dyn Confirm>,
}

#[allow(dead_code)]
impl GlobalContext {
    pub fn new(
        flags: GlobalFlags,
        logger: Arc<Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Rc::new(GitOperations::new()?);
        Ok(GlobalContext::from_parts(flags, git_ops, logger))
    }

    pub fn new_at_path(
        repo_path: &str,
        flags: GlobalFlags,
        logger: Arc<Logger>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let git_ops = Rc::new(GitOperations::new_at_path(repo_path)?);
        Ok(GlobalContext::from_parts(flags, git_ops, logger))
    }

    fn from_parts(
        flags: GlobalFlags,
        git_ops: Rc<GitOperations>,
        logger: Arc<Logger>,
    ) -> GlobalContext {
        // The sink choice is the whole of the `--json` channel contract, and it
        // happens here rather than at each call site: a command that forgets
        // would otherwise print prose into a document the user is about to
        // parse. The colour override is cleared separately in `main.rs`, where
        // the `colored` control is actually touched.
        let output: Arc<dyn OutputSink> = if flags.json {
            Arc::new(DiagnosticOutputSink)
        } else {
            Arc::new(ConsoleOutputSink)
        };
        GlobalContext {
            verbose: flags.verbose,
            no_push: flags.no_push,
            assume_yes: flags.assume_yes,
            json: flags.json,
            git_ops,
            logger,
            output,
            confirm: default_confirm(flags.assume_yes),
        }
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
        Self::new(
            GlobalFlags {
                verbose,
                no_push,
                ..GlobalFlags::defaults()
            },
            logger,
        )
    }

    pub fn git(&self) -> &GitOperations {
        &self.git_ops
    }

    /// `--verbose` narration, through the sink rather than around it.
    ///
    /// It used to `println!` directly, which meant `--json` could not reserve
    /// stdout no matter what the sink was set to. Every byte hitch prints has
    /// to pass through one of these six methods for the channel guarantee to be
    /// a fact rather than an intention.
    pub fn log_verbose(&self, message: &str) {
        if self.verbose {
            self.output.log(OutputLevel::Verbose, message);
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
