//! Structured logging utilities for Hitch
//!
//! Provides enhanced logging with:
//! - Log levels (debug, info, warn, error)
//! - Timestamps
//! - Structured fields
//! - JSON output option for machine processing

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::str::FromStr;

/// Log levels in order of severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl LogLevel {}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(format!("Invalid log level: {}", s)),
        }
    }
}

impl LogLevel {
    /// Get the emoji for this log level
    pub fn emoji(self) -> &'static str {
        match self {
            LogLevel::Debug => "🔍",
            LogLevel::Info => "ℹ️",
            LogLevel::Warn => "⚠️",
            LogLevel::Error => "❌",
        }
    }

    /// Get the ANSI color code for this log level
    pub fn color(self) -> &'static str {
        match self {
            LogLevel::Debug => "\x1b[36m", // Cyan
            LogLevel::Info => "\x1b[34m",  // Blue
            LogLevel::Warn => "\x1b[33m",  // Yellow
            LogLevel::Error => "\x1b[31m", // Red
        }
    }
}

/// A structured log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Timestamp in UTC
    pub timestamp: DateTime<Utc>,
    /// Log level
    pub level: String,
    /// Message
    pub message: String,
    /// Additional structured fields
    pub fields: HashMap<String, serde_json::Value>,
    /// Optional operation context
    pub operation: Option<String>,
    /// Optional environment context
    pub environment: Option<String>,
    /// Optional branch context
    pub branch: Option<String>,
    /// Optional command context
    pub command: Option<String>,
}

impl LogEntry {
    /// Create a new log entry
    pub fn new(level: LogLevel, message: String) -> Self {
        Self {
            timestamp: Utc::now(),
            level: format!("{:?}", level),
            message,
            fields: HashMap::new(),
            operation: None,
            environment: None,
            branch: None,
            command: None,
        }
    }

    /// Add a field to the log entry
    pub fn field<K: Into<String>, V: Into<serde_json::Value>>(mut self, key: K, value: V) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// Set the operation context
    pub fn operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// Set the environment context
    pub fn environment(mut self, env: impl Into<String>) -> Self {
        self.environment = Some(env.into());
        self
    }

    /// Set the branch context
    pub fn branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Set the command context
    pub fn command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

/// Logger configuration
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    /// Minimum log level to output
    pub min_level: LogLevel,
    /// Whether to output JSON format
    pub json_output: bool,
    /// Whether to include timestamps
    pub include_timestamp: bool,
    /// Whether to include colors in console output
    pub use_colors: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            min_level: LogLevel::Info,
            json_output: env::var("HITCH_LOG_JSON").is_ok(),
            include_timestamp: env::var("HITCH_LOG_TIMESTAMP").is_ok(),
            use_colors: true, // Default to true for better UX
        }
    }
}

/// Structured logger
pub struct Logger {
    config: LoggerConfig,
    context: LogContext,
}

/// Context that's applied to all log entries
#[derive(Debug, Clone, Default)]
pub struct LogContext {
    operation: Option<String>,
    environment: Option<String>,
    branch: Option<String>,
    command: Option<String>,
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

impl Logger {
    /// Create a new logger with default config
    pub fn new() -> Self {
        Self::with_config(LoggerConfig::default())
    }

    /// Create a new logger with custom config
    pub fn with_config(config: LoggerConfig) -> Self {
        Self {
            config,
            context: LogContext::default(),
        }
    }

    /// Create a logger for a specific command
    pub fn for_command(command: &str, verbose: bool) -> Self {
        let mut config = LoggerConfig::default();

        // Enable debug logging if verbose is set
        if verbose {
            config.min_level = LogLevel::Debug;
        }

        // Check environment variables
        if let Ok(level_str) = env::var("HITCH_LOG_LEVEL") {
            if let Ok(level) = LogLevel::from_str(&level_str) {
                config.min_level = level;
            }
        }

        let mut logger = Self::with_config(config);
        logger.context.command = Some(command.to_string());
        logger
    }

    /// Set the current operation context
    pub fn set_operation(&mut self, operation: impl Into<String>) {
        self.context.operation = Some(operation.into());
    }

    /// Set the current environment context
    pub fn set_environment(&mut self, env: impl Into<String>) {
        self.context.environment = Some(env.into());
    }

    /// Set the current branch context
    pub fn set_branch(&mut self, branch: impl Into<String>) {
        self.context.branch = Some(branch.into());
    }

    /// Log a message at the specified level
    pub fn log(&self, level: LogLevel, message: &str) {
        if level < self.config.min_level {
            return;
        }

        let entry = LogEntry::new(level, message.to_string())
            .operation(self.context.operation.clone().unwrap_or_default())
            .environment(self.context.environment.clone().unwrap_or_default())
            .branch(self.context.branch.clone().unwrap_or_default())
            .command(self.context.command.clone().unwrap_or_default());

        if self.config.json_output {
            self.log_json(&entry);
        } else {
            self.log_console(&entry);
        }
    }

    /// Log in JSON format
    fn log_json(&self, entry: &LogEntry) {
        if let Ok(json) = serde_json::to_string(entry) {
            eprintln!("{}", json);
        }
    }

    /// Log in console format
    fn log_console(&self, entry: &LogEntry) {
        let reset = "\x1b[0m";
        let level_color = if self.config.use_colors {
            entry
                .level
                .parse::<LogLevel>()
                .map(|l| l.color())
                .unwrap_or("")
        } else {
            ""
        };

        let emoji = entry
            .level
            .parse::<LogLevel>()
            .map(|l| l.emoji())
            .unwrap_or("");

        let mut parts = Vec::new();

        // Add timestamp if enabled
        if self.config.include_timestamp {
            parts.push(entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string());
        }

        // Add level
        parts.push(format!("{}{}{}{}", level_color, emoji, entry.level, reset));

        // Add context
        if let Some(command) = &entry.command {
            parts.push(format!("[{}]", command));
        }
        if let Some(operation) = &entry.operation {
            parts.push(format!("op:{}", operation));
        }
        if let Some(env) = &entry.environment {
            parts.push(format!("env:{}", env));
        }
        if let Some(branch) = &entry.branch {
            parts.push(format!("branch:{}", branch));
        }

        // Build the output
        let prefix = parts.join(" ");

        // Add fields if any
        if !entry.fields.is_empty() {
            let field_strs: Vec<String> = entry
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            eprintln!("{} {}: {}", prefix, entry.message, field_strs.join(" "));
        } else {
            eprintln!("{} {}", prefix, entry.message);
        }
    }

    /// Convenience methods for different log levels
    pub fn debug(&self, message: &str) {
        self.log(LogLevel::Debug, message);
    }

    pub fn info(&self, message: &str) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&self, message: &str) {
        self.log(LogLevel::Warn, message);
    }

    pub fn error(&self, message: &str) {
        self.log(LogLevel::Error, message);
    }

    /// Log with additional fields
    pub fn debug_with<F>(&self, message: &str, fields: F)
    where
        F: FnOnce(&mut LogEntry) -> LogEntry,
    {
        let mut entry = LogEntry::new(LogLevel::Debug, message.to_string())
            .operation(self.context.operation.clone().unwrap_or_default())
            .environment(self.context.environment.clone().unwrap_or_default())
            .branch(self.context.branch.clone().unwrap_or_default())
            .command(self.context.command.clone().unwrap_or_default());

        fields(&mut entry);

        if LogLevel::Debug >= self.config.min_level {
            if self.config.json_output {
                self.log_json(&entry);
            } else {
                self.log_console(&entry);
            }
        }
    }

    pub fn info_with<F>(&self, message: &str, fields: F)
    where
        F: FnOnce(&mut LogEntry) -> LogEntry,
    {
        let mut entry = LogEntry::new(LogLevel::Info, message.to_string())
            .operation(self.context.operation.clone().unwrap_or_default())
            .environment(self.context.environment.clone().unwrap_or_default())
            .branch(self.context.branch.clone().unwrap_or_default())
            .command(self.context.command.clone().unwrap_or_default());

        fields(&mut entry);

        if LogLevel::Info >= self.config.min_level {
            if self.config.json_output {
                self.log_json(&entry);
            } else {
                self.log_console(&entry);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_parsing() {
        assert_eq!(LogLevel::from_str("debug"), Ok(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("INFO"), Ok(LogLevel::Info));
        assert_eq!(LogLevel::from_str("warning"), Ok(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("ERROR"), Ok(LogLevel::Error));
        assert!(LogLevel::from_str("invalid").is_err());
    }

    #[test]
    fn test_log_entry_builder() {
        let entry = LogEntry::new(LogLevel::Info, "Test message".to_string())
            .field("key", "value")
            .operation("test_op")
            .environment("test_env");

        assert_eq!(entry.message, "Test message");
        assert_eq!(
            entry.fields.get("key"),
            Some(&serde_json::Value::String("value".to_string()))
        );
        assert_eq!(entry.operation, Some("test_op".to_string()));
        assert_eq!(entry.environment, Some("test_env".to_string()));
    }
}
