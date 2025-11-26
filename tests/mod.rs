//! Integration tests for Hitch CLI
//!
//! This module contains comprehensive tests for the Hitch CLI tool with complete
//! Git isolation and 80%+ test coverage target.

pub mod test_framework;

pub mod common;
pub mod integration;
pub mod scenarios;
pub mod unit;

// Re-export main test framework for convenience
pub use test_framework::*;
