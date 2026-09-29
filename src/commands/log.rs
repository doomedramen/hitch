//! `hitch log` — what happened to the environments, told from `hitch-metadata`
//! history (not a git log).
//!
//! Thin by design: the walk is [`crate::core::activity::build_activity`] and the
//! words are [`crate::core::render::render_activity`]. Read-only, so it does not
//! take the repo lock and stays usable while a rebuild is running.

use crate::commands::global_context::GlobalContext;
use crate::core::activity::{build_activity, ActivityLog, ActivityQuery};
use crate::core::render::{emit_json, render_activity, JSON_SCHEMA_VERSION};
use crate::types::HitchConfig;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct LogCommand {
    /// Show only events for this environment
    #[arg(long = "env")]
    pub environment: Option<String>,

    /// Show only events that name this branch
    #[arg(long)]
    pub branch: Option<String>,

    /// How many entries to show
    #[arg(long, short = 'n', default_value_t = 20)]
    pub limit: usize,

    /// Also show which metadata commit each entry came from
    #[arg(long)]
    pub verbose: bool,
}

/// The `--json` document: `{"schema_version", "log"}` — one half, like `why`.
#[derive(serde::Serialize)]
struct LogDocument {
    schema_version: u32,
    log: ActivityLog,
}

pub fn run(args: LogCommand, context: &GlobalContext) -> Result<()> {
    let mut context = context.clone();
    context.verbose = args.verbose;

    let query = ActivityQuery {
        environment: args.environment.clone(),
        branch: args.branch.clone(),
        limit: args.limit,
    };
    let log = build_activity(&context, &query)?;

    if let Some(name) = &args.environment {
        // A removed environment is valid: its history is what the reader wants.
        // So the check is "known now, or seen in an event", not "configured".
        let config =
            access_metadata_read_only(&context, |config: &HitchConfig| Ok(config.clone()))?;
        if !config.environments.contains_key(name) && !seen_in_history(&context, name)? {
            let mut names: Vec<&str> = config.environments.keys().map(|k| k.as_str()).collect();
            names.sort_unstable();
            let known = if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            };
            anyhow::bail!(
                "No environment named '{name}', now or in the history hitch scanned.\n\
                 Configured environments: {known}\n\n\
                 Run 'hitch status' to see what hitch knows about."
            );
        }
    }

    if context.json {
        emit_json(&LogDocument {
            schema_version: JSON_SCHEMA_VERSION,
            log,
        })
    } else {
        println!(
            "{}",
            render_activity(&log, chrono::Local::now().fixed_offset(), context.verbose)
        );
        Ok(())
    }
}

/// Whether any scanned event names `name`, ignoring the query's filters and
/// limit — a filtered or truncated log says nothing about whether the
/// environment ever existed.
fn seen_in_history(context: &GlobalContext, name: &str) -> Result<bool> {
    let everything = build_activity(
        context,
        &ActivityQuery {
            limit: usize::MAX,
            ..Default::default()
        },
    )?;
    Ok(everything
        .entries
        .iter()
        .flat_map(|e| &e.events)
        .any(|ev| ev.environment() == name))
}
