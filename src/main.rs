use clap::Parser;
use std::sync::Arc;

use hitch::cli::{Cli, Commands};
use hitch::commands;
use hitch::commands::global_context::GlobalContext;
use hitch::utils::logging::Logger;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize colored output
    colored::control::set_override(true);

    // True no-args invocation should behave like `--help` and exit 0.
    if std::env::args_os().len() == 1 {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    let cli = Cli::parse();

    // No subcommand => print help (even if global flags are present).
    if cli.command.is_none() {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    let command = cli.command.expect("checked above");

    // `completion` generates a static script from the clap command tree and
    // has no git dependency at all, so handle it before constructing
    // `GlobalContext` below (which requires an actual git repository) —
    // otherwise `hitch completion zsh > ~/.zsh/completions/_hitch` fails with
    // "Not in a git repository" when run from outside one.
    let command = match command {
        Commands::Completion(args) => {
            return commands::completion::run(args).map_err(|e| e.into());
        }
        other => other,
    };

    // Configure logger for the specific command
    let command_name = match &command {
        Commands::Init(_) => "init",
        Commands::Add(_) => "add",
        Commands::Remove(_) => "remove",
        Commands::Promote(_) => "promote",
        Commands::Push(_) => "push",
        Commands::Demote(_) => "demote",
        Commands::Rebuild(_) => "rebuild",
        Commands::Release(_) => "release",
        Commands::Status(_) => "status",
        Commands::Tree(_) => "tree",
        Commands::Lock(_) => "lock",
        Commands::Unlock(_) => "unlock",
        Commands::Pr(_) => "pr",
        Commands::Doctor(_) => "doctor",
        Commands::Guard(_) => "guard",
        Commands::Completion(_) => unreachable!("handled above before GlobalContext is created"),
        Commands::Approvals(_) => "approvals",
        Commands::Cleanup(_) => "cleanup",
        Commands::Diff(_) => "diff",
        Commands::Set(_) => "set",
        Commands::Branch(_) => "branch",
        Commands::Setup(_) => "setup",
        Commands::Conflicts(_) => "conflicts",
        Commands::Resolve(_) => "resolve",
        Commands::Resolutions(_) => "resolutions",
    };

    // Create a new logger configured for this command
    let logger = Arc::new(Logger::for_command(command_name, cli.verbose));

    // Create global context with flags
    let context = GlobalContext::new(cli.verbose, cli.no_push, cli.yes, logger)?;

    // Acquire a repository-wide lock for mutating commands so two concurrent
    // Hitch operations on the same repo can't clobber each other's branch
    // switches and hitch-metadata rewrites. Read-only commands skip the lock.
    // The guard is held until `main` returns, which releases the lock.
    let _repo_lock = if command_is_mutating(&command) {
        Some(
            hitch::utils::repo_lock::RepoLock::acquire(
                std::path::Path::new(&context.git().get_git_dir()),
                command_name,
            )
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
        )
    } else {
        None
    };

    // Finish any publish that died between moving a branch ref and updating
    // the checkouts standing on it. Only under the repo lock, so this can
    // never race a publish that is still in flight, and only ever repairs a
    // working tree it can prove is stale rather than edited.
    if _repo_lock.is_some() {
        if let Err(e) = hitch::utils::publish_journal::recover(&context) {
            context.log_verbose(&format!("Could not check for interrupted publishes: {}", e));
        }
    }

    // Execute the appropriate command
    match command {
        Commands::Init(args) => commands::init::run(args, &context).map_err(|e| e.into()),
        Commands::Add(args) => commands::add::run(args, &context).map_err(|e| e.into()),
        Commands::Remove(args) => commands::remove::run(args, &context).map_err(|e| e.into()),
        Commands::Promote(args) => commands::promote::run(args, &context).map_err(|e| e.into()),
        Commands::Push(args) => commands::push::run(args, &context).map_err(|e| e.into()),
        Commands::Demote(args) => commands::demote::run(args, &context).map_err(|e| e.into()),
        Commands::Rebuild(args) => {
            // hitch rebuild distinguishes a fully clean rebuild (exit 0) from
            // one that succeeded but held a conflicting branch (exit 2), so
            // CI can warn on holds without failing the build. A halt-policy
            // refusal or any other failure both take the normal Err path
            // below (exit 1), same as every other command.
            let held = commands::rebuild::run(args, &context)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            if held {
                use std::io::Write;
                // process::exit skips normal shutdown (which is what flushes
                // stdout on a plain `return` from main), so flush explicitly
                // or buffered output can be lost when stdout isn't a TTY.
                let _ = std::io::stdout().flush();
                std::process::exit(2);
            }
            Ok(())
        }
        Commands::Release(args) => commands::release::run(args, &context).map_err(|e| e.into()),
        Commands::Status(args) => commands::status::run(args, &context).map_err(|e| e.into()),
        Commands::Tree(args) => commands::tree::run(args, &context).map_err(|e| e.into()),
        Commands::Lock(args) => commands::lock::run(args, &context).map_err(|e| e.into()),
        Commands::Unlock(args) => commands::unlock::run(args, &context).map_err(|e| e.into()),
        Commands::Pr(args) => commands::pr::run(args, &context).map_err(|e| e.into()),
        Commands::Doctor(args) => commands::doctor::run(args, &context).map_err(|e| e.into()),
        Commands::Guard(args) => commands::guard::run(args, &context).map_err(|e| e.into()),
        Commands::Completion(_) => unreachable!("handled above before GlobalContext is created"),
        Commands::Approvals(args) => commands::approvals::run(args, &context).map_err(|e| e.into()),
        Commands::Cleanup(args) => commands::cleanup::run(args, &context).map_err(|e| e.into()),
        Commands::Diff(args) => commands::diff::run(args, &context).map_err(|e| e.into()),
        Commands::Set(args) => commands::set::run(args, &context).map_err(|e| e.into()),
        Commands::Branch(args) => commands::branch::run(args, &context).map_err(|e| e.into()),
        Commands::Setup(args) => commands::setup::run(args, &context).map_err(|e| e.into()),
        Commands::Conflicts(args) => commands::conflicts::run(args, &context).map_err(|e| e.into()),
        Commands::Resolve(args) => commands::resolve::run(args, &context).map_err(|e| e.into()),
        Commands::Resolutions(args) => {
            commands::resolutions::run(args, &context).map_err(|e| e.into())
        }
    }
}

/// Whether a command mutates repository state and therefore needs the
/// repository-wide lock. Read-only commands (status, tree, diff, and the
/// read-only approvals subcommands) skip it so they remain usable while a
/// mutating operation is in progress. `completion` never reaches this
/// function — it's handled in `main` before a `GlobalContext` even exists.
fn command_is_mutating(command: &Commands) -> bool {
    match command {
        Commands::Status(_)
        | Commands::Tree(_)
        | Commands::Diff(_)
        | Commands::Doctor(_)
        | Commands::Setup(_)
        | Commands::Conflicts(_)
        // resolutions only ever touches refs/hitch/resolutions/* — never
        // hitch-metadata, environment branches, or the working tree — so it
        // doesn't contend for the repo-wide lock and stays usable during a
        // rebuild.
        | Commands::Resolutions(_) => false,
        Commands::Approvals(args) => args.is_mutating(),
        _ => true,
    }
}
