use crate::commands::global_context::GlobalContext;
use clap::Args;

#[derive(Args)]
pub struct AddCommand {
    /// Environment name to add
    pub env_name: String,

    /// Source branch for the environment (defaults to main)
    #[arg(long)]
    source: Option<String>,
}

pub fn run(_args: AddCommand, _context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    println!("Add command not yet implemented");
    Ok(())
}
