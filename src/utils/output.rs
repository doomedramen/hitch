#![allow(dead_code)]

use colored::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLevel {
    /// `--verbose` narration. No glyph even on the console: it is the channel
    /// that explains the mechanism, and a glyph on it would put it on the same
    /// footing as a result.
    Verbose,
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
            OutputLevel::Verbose => println!("{message}"),
            OutputLevel::Info => println!("{} {}", "ℹ️".blue(), message),
            OutputLevel::Success => println!("{} {}", "✅".green(), message),
            OutputLevel::Warning => println!("{} {}", "⚠️".yellow(), message),
            OutputLevel::Error => eprintln!("{} {}", "❌".red(), message),
        }
    }
}

/// Every level to stderr, with no glyph and no colour.
///
/// This is what `--json` swaps in, and it is the whole of spec §18's channel
/// split: with it, stdout carries the document and nothing else, so a consumer
/// can pipe it into `jq` without filtering. The console sink sends `Info`,
/// `Success`, and `Warning` to stdout, which is right for a human and fatal
/// for a program — so under `--json` the two sets of output cannot interleave
/// by accident, only by someone routing around the sink.
///
/// The glyphs go with the stdout: an escape sequence inside a JSON string is
/// not "pretty", it is a parse hazard in some consumers and a display artefact
/// in all of them.
#[derive(Default)]
pub struct DiagnosticOutputSink;

impl OutputSink for DiagnosticOutputSink {
    fn log(&self, _level: OutputLevel, message: &str) {
        eprintln!("{message}");
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
