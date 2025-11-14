use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct PromoteCommand {
    #[arg()]
    pub branch: String,
    #[arg()]
    pub env_name: String,
}

pub fn run(
    _args: PromoteCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Promote command not yet implemented");
    Ok(())
}
