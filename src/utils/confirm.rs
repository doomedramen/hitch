use anyhow::{Context, Result};
use std::io::{self, Write};

pub trait Confirm: Send + Sync {
    fn confirm_force_push_rebuild(&self, env_name: &str) -> Result<bool>;
}

pub struct StdinConfirm;

impl Confirm for StdinConfirm {
    fn confirm_force_push_rebuild(&self, _env_name: &str) -> Result<bool> {
        print!("Do you want to proceed? [y/N]: ");
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
    fn confirm_force_push_rebuild(&self, _env_name: &str) -> Result<bool> {
        Ok(true)
    }
}

pub struct AlwaysNoConfirm;

#[allow(dead_code)]
impl Confirm for AlwaysNoConfirm {
    fn confirm_force_push_rebuild(&self, _env_name: &str) -> Result<bool> {
        Ok(false)
    }
}
