//! Comprehensive testing framework for Hitch CLI
//!
//! This module provides a complete testing framework with:
//! - Complete Git isolation using temporary directories
//! - Closure-based test API for easy test writing
//! - Command runners for hitch and git operations
//! - File system helpers for test setup
//! - Assertion helpers for common validations
//! - Mocking capabilities for external dependencies

pub mod assertions;
pub mod command_runners;
pub mod file_system_helpers;
pub mod fixtures;
pub mod framework;
pub mod mocking;

pub use assertions::AssertionHelpers;
pub use command_runners::{GitCommandRunner, HitchCommandRunner};
pub use file_system_helpers::FileSystemHelpers;
pub use framework::{HitchTestFramework, TestEnvironment};
pub use mocking::MockCapabilities;
