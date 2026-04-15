#![allow(dead_code)]

use colored::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub trait OutputSink: Send + Sync {
    fn log(&self, level: OutputLevel, message: &str);
}

#[derive(Default)]
pub struct ConsoleOutputSink;

impl OutputSink for ConsoleOutputSink {
    fn log(&self, level: OutputLevel, message: &str) {
        match level {
            OutputLevel::Info => println!("{} {}", "ℹ️".blue(), message),
            OutputLevel::Success => println!("{} {}", "✅".green(), message),
            OutputLevel::Warning => println!("{} {}", "⚠️".yellow(), message),
            OutputLevel::Error => eprintln!("{} {}", "❌".red(), message),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BufferedLine {
    pub level: OutputLevel,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct BufferedOutputSink {
    lines: Mutex<Vec<BufferedLine>>,
}

impl BufferedOutputSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn snapshot(&self) -> Vec<BufferedLine> {
        self.lines.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.clear();
        }
    }
}

impl OutputSink for BufferedOutputSink {
    fn log(&self, level: OutputLevel, message: &str) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(BufferedLine {
                level,
                message: message.to_string(),
            });
        }
    }
}
