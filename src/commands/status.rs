use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct StatusCommand {}

pub fn run(
    _args: StatusCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Command not yet implemented");
    Ok(())
}
