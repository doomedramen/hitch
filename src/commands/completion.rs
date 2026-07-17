use crate::cli::Cli;
use anyhow::Result;
use clap::{CommandFactory, Parser};

/// Generate shell completion scripts
#[derive(Parser)]
pub struct CompletionCommand {
    /// The shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

/// Takes no `GlobalContext` — unlike every other command, this doesn't touch
/// git at all, so it must stay callable without one (see `main.rs`, which
/// dispatches this before opening a repository).
pub fn run(args: CompletionCommand) -> Result<()> {
    // Generated straight from the real `Cli` command tree, so it can never
    // drift out of sync with the actual commands/subcommands/flags the way a
    // hand-maintained script per shell would.
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::Shell;

    #[test]
    fn test_completion_generates_for_all_shells() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ] {
            let mut cmd = Cli::command();
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut cmd, "hitch", &mut buf);
            let script = String::from_utf8(buf).unwrap();

            assert!(
                !script.is_empty(),
                "{shell:?} completion script should not be empty"
            );
            assert!(
                script.contains("hitch"),
                "{shell:?} completion script should reference the hitch binary"
            );
        }
    }

    #[test]
    fn test_completion_covers_all_subcommands() {
        // A regression guard for the exact bug this replaced: hand-written
        // per-shell scripts silently fell out of sync as subcommands were
        // added. Generating from `Cli` directly means every subcommand shows
        // up automatically, but this still checks nothing was lost in the
        // move (e.g. a subcommand missing its `ValueEnum`/`Args` derive).
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        clap_complete::generate(Shell::Bash, &mut cmd, "hitch", &mut buf);
        let script = String::from_utf8(buf).unwrap();

        for subcommand in [
            "init",
            "add",
            "remove",
            "promote",
            "demote",
            "rebuild",
            "release",
            "status",
            "tree",
            "lock",
            "unlock",
            "guard",
            "completion",
            "approvals",
            "branch",
            "cleanup",
            "diff",
            "set",
        ] {
            assert!(
                script.contains(subcommand),
                "bash completion script missing subcommand: {subcommand}"
            );
        }
    }
}
