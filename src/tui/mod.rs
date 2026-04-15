#[cfg(feature = "tui")]
pub mod app;

#[cfg(not(feature = "tui"))]
pub mod app {
    pub fn run_tui(_verbose: bool, _no_push: bool) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "TUI support is not enabled in this build (compile with default features or --features tui)"
        ))
    }
}

#[cfg(feature = "tui")]
pub mod terminal;
