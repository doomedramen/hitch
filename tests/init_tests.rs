use anyhow::Result;

mod common;
use common::run_init_tests;

fn main() -> Result<()> {
    println!("🚀 Running Hitch init command tests...");

    match run_init_tests() {
        Ok(()) => {
            println!("\n🎉 All init command tests passed!");
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ Init command tests failed: {}", e);
            std::process::exit(1);
        }
    }
}