use clap::Args;
use anyhow::Result;
use crate::commands::global_context::GlobalContext;

#[derive(Args)]
pub struct StatusCommand {
}

pub fn run(_args: StatusCommand, _context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    println!("Command not yet implemented");
    Ok(())
}
