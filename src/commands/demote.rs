use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct DemoteCommand {
    #[arg()]
    pub branch: String,
    #[arg()]
    pub env_name: String,
}

pub fn run(
    _args: DemoteCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Demote command not yet implemented");
    Ok(())
}
