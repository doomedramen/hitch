use clap::Args;
use crate::commands::global_context::GlobalContext;

#[derive(Args)]
pub struct GuardCommand {
    #[arg()]
    pub env_name: String,
}

pub fn run(_args: GuardCommand, _context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    println!("Guard command not yet implemented");
    Ok(())
}