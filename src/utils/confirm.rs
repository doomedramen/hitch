use anyhow::{Context, Result};
use std::io::{self, IsTerminal, Write};

/// Abstraction over "ask the user to confirm a destructive action".
///
/// Every interactive prompt in Hitch goes through this trait so that a single
/// global `--yes` (or a non-interactive session) is handled in one place
/// instead of each command re-implementing a `read_line` loop.
pub trait Confirm: Send + Sync {
    /// Ask `prompt` and return whether the user agreed.
    ///
    /// Implementations must never block forever: when there is no terminal to
    /// read from they return an error telling the caller to pass `--yes`.
    fn confirm(&self, prompt: &str) -> Result<bool>;
}

pub struct StdinConfirm;

impl Confirm for StdinConfirm {
    fn confirm(&self, prompt: &str) -> Result<bool> {
        if !io::stdin().is_terminal() {
            return Err(anyhow::anyhow!(
                "Refusing to prompt for confirmation: no interactive terminal.\n  Prompt was: {}\n  Re-run with --yes (or set HITCH_YES=1) to confirm automatically.",
                prompt
            ));
        }

        print!("{} [y/N]: ", prompt);
        io::stdout()
            .flush()
            .context("Failed to flush stdout for user prompt")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read user input")?;
        let input = input.trim().to_lowercase();
        Ok(input == "y" || input == "yes")
    }
}

pub struct AlwaysYesConfirm;

impl Confirm for AlwaysYesConfirm {
    fn confirm(&self, _prompt: &str) -> Result<bool> {
        Ok(true)
    }
}

pub struct AlwaysNoConfirm;

#[allow(dead_code)]
impl Confirm for AlwaysNoConfirm {
    fn confirm(&self, _prompt: &str) -> Result<bool> {
        Ok(false)
    }
}
