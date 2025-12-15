//! Progress reporting utilities for long-running operations
//!
//! Provides a simple callback-based progress reporting system
//! that works without requiring async/await.

use std::fmt;

/// Progress information for long-running operations
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
#[allow(dead_code)]
pub trait ProgressReporter: Send + Sync {
    /// Report progress
    fn report(&self, progress: &ProgressInfo);
}

/// Simple step logger for long-running operations
pub struct StepLogger {
    operation: String,
    current_step: usize,
    total_steps: usize,
}

impl StepLogger {
    /// Create a new step logger
    pub fn new(operation: String, total_steps: usize) -> Self {
        Self {
            operation,
            current_step: 0,
            total_steps,
        }
    }

    /// Log the current step
    pub fn step(&mut self, description: String) {
        self.current_step += 1;
        if self.total_steps > 1 {
            println!(
                "ℹ️ [{}/{}] {} - {}",
                self.current_step, self.total_steps, self.operation, description
            );
        } else {
            println!("ℹ️ {} - {}", self.operation, description);
        }
    }

    /// Mark the operation as complete
    pub fn complete(&self) {
        println!("✅ {}", self.operation);
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
    fn test_step_logger() {
        let mut logger = StepLogger::new("Test".to_string(), 2);

        logger.step("Step 1".to_string());
        logger.step("Step 2".to_string());
        logger.complete();

        // Simple test - just verify it doesn't panic
        assert!(logger.current_step == 2);
    }
}
