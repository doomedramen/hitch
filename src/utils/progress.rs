//! Progress reporting utilities for long-running operations
//!
//! Provides a simple callback-based progress reporting system
//! that works without requiring async/await.

use std::fmt;
use std::io::Write;
use std::sync::{Arc, RwLock};

/// Progress information for long-running operations
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// Current step number (0-based)
    pub current_step: usize,
    /// Total number of steps
    pub total_steps: usize,
    /// Description of current step
    pub step_description: String,
    /// Overall operation description
    pub operation: String,
    /// Optional percentage complete (0.0 to 1.0)
    pub percentage: Option<f32>,
}

impl ProgressInfo {
    /// Create a new progress info
    pub fn new(
        operation: String,
        current_step: usize,
        total_steps: usize,
        step_description: String,
    ) -> Self {
        let percentage = if total_steps > 0 {
            Some(current_step as f32 / total_steps as f32)
        } else {
            None
        };

        Self {
            current_step,
            total_steps,
            step_description,
            operation,
            percentage,
        }
    }

    /// Create a progress info for a single step operation
    #[allow(dead_code)]
    pub fn single_step(operation: String, step_description: String) -> Self {
        Self {
            current_step: 0,
            total_steps: 1,
            step_description,
            operation,
            percentage: Some(0.0),
        }
    }

    /// Update to the next step
    #[allow(dead_code)]
    pub fn next_step(&self, step_description: String) -> Self {
        Self::new(
            self.operation.clone(),
            self.current_step + 1,
            self.total_steps,
            step_description,
        )
    }

    /// Mark as complete
    #[allow(dead_code)]
    pub fn complete(&self) -> Self {
        Self::new(
            self.operation.clone(),
            self.total_steps,
            self.total_steps,
            "Complete".to_string(),
        )
    }
}

impl fmt::Display for ProgressInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.total_steps > 1 {
            write!(
                f,
                "[{}/{}] {} - {}",
                self.current_step + 1,
                self.total_steps,
                self.operation,
                self.step_description
            )
        } else {
            write!(f, "{} - {}", self.operation, self.step_description)
        }
    }
}

/// Trait for progress reporters
pub trait ProgressReporter: Send + Sync {
    /// Report progress
    fn report(&self, progress: &ProgressInfo);
}

/// Console progress reporter that prints to stdout
pub struct ConsoleProgressReporter {
    /// Whether to use verbose output
    verbose: bool,
    /// Last reported progress (for resume capability)
    last_progress: RwLock<Option<ProgressInfo>>,
    /// Whether progress bar is currently suspended
    suspended: RwLock<bool>,
}

impl ConsoleProgressReporter {
    /// Create a new console progress reporter
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            last_progress: RwLock::new(None),
            suspended: RwLock::new(false),
        }
    }

    /// Suspend the progress bar (clears the current line)
    /// Call this before printing log messages to avoid interleaved output
    pub fn suspend(&self) {
        if self.verbose {
            return; // Verbose mode doesn't use progress bar
        }

        let has_progress = self
            .last_progress
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);
        let already_suspended = self.suspended.read().map(|g| *g).unwrap_or(false);

        if has_progress && !already_suspended {
            // Clear the current line by overwriting with spaces and returning to start
            print!("\r{}\r", " ".repeat(120));
            let _ = std::io::stdout().flush();
            if let Ok(mut guard) = self.suspended.write() {
                *guard = true;
            }
        }
    }

    /// Resume the progress bar (redraws the last progress state)
    /// Call this after printing log messages
    pub fn resume(&self) {
        if self.verbose {
            return; // Verbose mode doesn't use progress bar
        }

        let is_suspended = self.suspended.read().map(|g| *g).unwrap_or(false);
        if is_suspended {
            if let Ok(guard) = self.last_progress.read() {
                if let Some(ref progress) = *guard {
                    // Don't resume if we're already complete
                    if progress.percentage.is_some_and(|p| p >= 1.0) {
                        drop(guard);
                        if let Ok(mut suspended) = self.suspended.write() {
                            *suspended = false;
                        }
                        return;
                    }
                    self.draw_progress_bar(progress);
                }
            }
            if let Ok(mut suspended) = self.suspended.write() {
                *suspended = false;
            }
        }
    }

    /// Draw the progress bar without storing state
    fn draw_progress_bar(&self, progress: &ProgressInfo) {
        if let Some(percentage) = progress.percentage {
            let bar_width = 40;
            let filled = (percentage * bar_width as f32) as usize;
            let empty = bar_width - filled;

            print!("\r{}: [", progress.operation);
            for _ in 0..filled {
                print!("=");
            }
            for _ in 0..empty {
                print!(" ");
            }
            print!(
                "] {:.0}% - {}",
                percentage * 100.0,
                progress.step_description
            );
            let _ = std::io::stdout().flush();

            // Move to next line when complete
            if percentage >= 1.0 {
                println!();
            }
        } else {
            println!("{}", progress);
        }
    }
}

impl ProgressReporter for ConsoleProgressReporter {
    fn report(&self, progress: &ProgressInfo) {
        if self.verbose {
            // Verbose mode shows all steps
            println!("{}", progress);
        } else {
            // Store the progress for potential resume
            if let Ok(mut guard) = self.last_progress.write() {
                *guard = Some(progress.clone());
            }
            if let Ok(mut guard) = self.suspended.write() {
                *guard = false;
            }

            // Draw the progress bar
            self.draw_progress_bar(progress);
        }
    }
}

/// No-op progress reporter for tests or when progress is not needed
#[allow(dead_code)]
pub struct NoOpProgressReporter;

impl ProgressReporter for NoOpProgressReporter {
    fn report(&self, _progress: &ProgressInfo) {
        // Do nothing
    }
}

/// Helper struct for reporting progress through a sequence of steps
pub struct StepProgress {
    operation: String,
    current_step: usize,
    total_steps: usize,
    reporter: Arc<ConsoleProgressReporter>,
}

impl StepProgress {
    /// Create a new step progress
    pub fn new(
        operation: String,
        total_steps: usize,
        reporter: Box<ConsoleProgressReporter>,
    ) -> Self {
        Self {
            operation,
            current_step: 0,
            total_steps,
            reporter: Arc::from(reporter),
        }
    }

    /// Report the current step
    pub fn step(&mut self, description: String) {
        let progress = ProgressInfo::new(
            self.operation.clone(),
            self.current_step,
            self.total_steps,
            description,
        );
        self.reporter.report(&progress);
        self.current_step += 1;
    }

    /// Mark the operation as complete
    pub fn complete(&self) {
        let progress = ProgressInfo::new(
            self.operation.clone(),
            self.total_steps,
            self.total_steps,
            "Complete".to_string(),
        );
        self.reporter.report(&progress);
    }

    /// Suspend the progress bar (clears the current line)
    /// Call this before printing log messages to avoid interleaved output
    #[allow(dead_code)]
    pub fn suspend(&self) {
        self.reporter.suspend();
    }

    /// Resume the progress bar (redraws the last progress state)
    /// Call this after printing log messages
    #[allow(dead_code)]
    pub fn resume(&self) {
        self.reporter.resume();
    }

    /// Get a reference to the reporter for sharing with other components
    pub fn reporter(&self) -> Arc<ConsoleProgressReporter> {
        Arc::clone(&self.reporter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_info() {
        let progress = ProgressInfo::new("Test Operation".to_string(), 1, 3, "Step 1".to_string());

        assert_eq!(progress.current_step, 1);
        assert_eq!(progress.total_steps, 3);
        assert_eq!(progress.step_description, "Step 1");
        assert_eq!(progress.percentage, Some(1.0 / 3.0));
    }

    #[test]
    fn test_step_progress() {
        // Use verbose mode to avoid terminal output during tests
        let reporter = Box::new(ConsoleProgressReporter::new(true));
        let mut progress = StepProgress::new("Test".to_string(), 2, reporter);

        progress.step("Step 1".to_string());
        progress.step("Step 2".to_string());
        progress.complete();

        // Note: This test would need access to the inner reporter to verify
        // In practice, we'd use a mock or spy pattern
    }
}
