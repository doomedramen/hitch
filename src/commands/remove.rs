use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct RemoveCommand {
    #[arg()]
    pub env_name: String,
    #[arg(long)]
    pub force: bool,
}

pub fn run(
    _args: RemoveCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Remove command not yet implemented");
    Ok(())
}
