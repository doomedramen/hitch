use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct LockCommand {
    #[arg()]
    pub env_name: String,
}

pub fn run(_args: LockCommand, _context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    println!("Lock command not yet implemented");
    Ok(())
}
