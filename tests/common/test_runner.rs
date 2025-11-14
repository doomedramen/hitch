use colored::*;
use anyhow::Result;
use crate::testing::TestEnvironment;
use std::sync::Arc;
use parking_lot::Mutex;

/// Test runner for Hitch CLI testing
pub struct TestRunner {
    test_count: Arc<Mutex<usize>>,
    passed_count: Arc<Mutex<usize>>,
    failed_count: Arc<Mutex<usize>>,
}

impl TestRunner {
    pub fn new() -> Self {
        Self {
            test_count: Arc::new(Mutex::new(0)),
            passed_count: Arc::new(Mutex::new(0)),
            failed_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Run a test suite
    pub fn run_suite<F>(&self, suite_name: &str, test_fn: F) -> Result<()>
    where
        F: FnOnce(&TestRunner) -> Result<()>,
    {
        println!("\n🧪 Running test suite: {}", suite_name.bold());
        println!("{}", "=".repeat(50));

        let result = test_fn(self);

        self.print_summary();
        result
    }

    /// Run a single test
    pub fn test<F>(&self, test_name: &str, test_fn: F) -> Result<()> {
        *self.test_count.lock().unwrap() += 1;

        print!("  {} ... ", test_name);
        std::io::stdout().flush()?;

        match test_fn() {
            Ok(()) => {
                *self.passed_count.lock().unwrap() += 1;
                println!("{}", "✓".green());
            }
            Err(e) => {
                *self.failed_count.lock().unwrap() += 1;
                println!("{} {}", "✗".red(), e);
                return Err(e);
            }
        }

        Ok(())
    }

    /// Assert a condition is true
    pub fn assert_true(&self, condition: bool, message: &str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            return Err(anyhow::anyhow!("Assertion failed: {}", message));
        }
    }

    /// Assert a condition is false
    pub fn assert_false(&self, condition: bool, message: &str) -> Result<()> {
        self.assert_true(!condition, message)
    }

    /// Assert two values are equal
    pub fn assert_eq<T: std::fmt::Debug + PartialEq>(&self, left: T, right: T, message: &str) -> Result<()> {
        if left == right {
            Ok(())
        } else {
            return Err(anyhow::anyhow!(
                "Assertion failed: {} - expected {:?}, got {:?}",
                message, left, right
            ));
        }
    }

    /// Assert a string contains another string
    pub fn assert_contains(&self, haystack: &str, needle: &str, message: &str) -> Result<()> {
        if haystack.contains(needle) {
            Ok(())
        } else {
            return Err(anyhow::anyhow!(
                "Assertion failed: {} - expected '{}' to contain '{}'",
                message, haystack, needle
            ));
        }
    }

    /// Print test summary
    fn print_summary(&self) {
        let total = *self.test_count.lock().unwrap();
        let passed = *self.passed_count.lock().unwrap();
        let failed = *self.failed_count.lock().unwrap();

        println!("\n{}", "=".repeat(50));
        if failed == 0 {
            println!("🎉 {} All tests passed! ({}/{} total)",
                "SUCCESS".green().bold(), passed, total);
        } else {
            println!("❌ {} {} tests passed, {} failed ({}/{} total)",
                "FAILED".red().bold(), passed, failed, total);
            println!("See above for details");
        }
    }

    /// Get test statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            *self.test_count.lock().unwrap(),
            *self.passed_count.lock().unwrap(),
            *self.failed_count.lock().unwrap(),
        )
    }
}

impl Drop for TestRunner {
    fn drop(&mut &mut self) {
        let (total, passed, failed) = self.stats();
        if failed > 0 {
            eprintln!("\n❌ Test suite completed with failures: {}/{} tests passed", passed, total);
        }
    }
}