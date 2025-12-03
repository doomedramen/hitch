//! Progress reporting utilities for long-running operations
//!
//! Provides a simple callback-based progress reporting system
//! that works without requiring async/await.

#![allow(dead_code)]

use std::fmt;
use std::sync::Arc;

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
    pub fn next_step(&self, step_description: String) -> Self {
        Self::new(
            self.operation.clone(),
            self.current_step + 1,
            self.total_steps,
            step_description,
        )
    }

    /// Mark as complete
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
}

impl ConsoleProgressReporter {
    /// Create a new console progress reporter
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

impl ProgressReporter for ConsoleProgressReporter {
    fn report(&self, progress: &ProgressInfo) {
        if self.verbose {
            // Verbose mode shows all steps
            println!("{}", progress);
        } else {
            // Non-verbose mode shows a progress bar
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
                use std::io::Write;
                std::io::stdout().flush().unwrap();

                // Move to next line when complete
                if percentage >= 1.0 {
                    println!();
                }
            } else {
                println!("{}", progress);
            }
        }
    }
}

/// No-op progress reporter for tests or when progress is not needed
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
    reporter: Arc<dyn ProgressReporter>,
}

impl StepProgress {
    /// Create a new step progress
    pub fn new(operation: String, total_steps: usize, reporter: Box<dyn ProgressReporter>) -> Self {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestReporter {
        reports: std::sync::Arc<std::sync::Mutex<Vec<ProgressInfo>>>,
    }

    impl TestReporter {
        fn new() -> Self {
            Self {
                reports: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }

        fn get_reports(&self) -> Vec<ProgressInfo> {
            self.reports.lock().unwrap().clone()
        }
    }

    impl ProgressReporter for TestReporter {
        fn report(&self, progress: &ProgressInfo) {
            self.reports.lock().unwrap().push(progress.clone());
        }
    }

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
        let reporter = Box::new(TestReporter::new());
        let mut progress = StepProgress::new("Test".to_string(), 2, reporter);

        progress.step("Step 1".to_string());
        progress.step("Step 2".to_string());
        progress.complete();

        // Note: This test would need access to the inner reporter to verify
        // In practice, we'd use a mock or spy pattern
    }
}
