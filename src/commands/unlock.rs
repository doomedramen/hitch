use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct UnlockCommand {
    #[arg()]
    pub env_name: String,
}

pub fn run(
    _args: UnlockCommand,
    _context: &GlobalContext,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Unlock command not yet implemented");
    Ok(())
}
