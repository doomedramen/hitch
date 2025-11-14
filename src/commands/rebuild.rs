use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct RebuildCommand {
    #[arg()]
    pub env_name: String,
}

pub fn run(
    _args: RebuildCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Rebuild command not yet implemented");
    Ok(())
}
