//! `hitch why` — explain one branch, or one environment, or one branch in one
//! environment (spec §14).
//!
//! Thin by design. Everything about *what* to say lives in
//! [`crate::core::why::build_why`], and everything about *how* to say it lives
//! in [`crate::core::render::render_why`]; what is left here is the part only a
//! command can do: deciding what the reader typed, and the single
//! `rev_parse_opt` that distinguishes a branch hitch does not know from a name
//! that is not a ref at all.

use crate::commands::global_context::GlobalContext;
use crate::core::render::{emit_json, render_why};
use crate::core::state::build_state_snapshot;
use crate::core::why::{build_why, WhyExplanation, WhySubject};
use crate::types::HitchConfig;
use crate::utils::prelude::access_metadata_read_only;
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct WhyCommand {
    /// The branch or environment to explain.
    ///
    /// A name that is both an environment and a promoted branch is an error
    /// naming both readings, not a guess.
    pub target: String,

    /// Narrow a branch question to one environment.
    ///
    /// `hitch why <branch> <environment>`. Omitted, the question is about the
    /// branch everywhere, or about the environment if that is what the name is.
    pub environment: Option<String>,

    /// Print detailed step-by-step logs
    #[arg(long)]
    pub verbose: bool,
}

pub fn run(args: WhyCommand, context: &GlobalContext) -> Result<()> {
    let mut context = context.clone();
    context.verbose = args.verbose;

    context.log_verbose("Starting why command...");

    let config = access_metadata_read_only(&context, |config: &HitchConfig| Ok(config.clone()))?;

    // One snapshot for the whole command, as `hitch status` does. `why` is a
    // read-only question and it must be answerable without holding a lock: an
    // explanatory tool that blocks on a mutation in progress is the wrong kind
    // of explanatory tool.
    let snapshot = build_state_snapshot(&context)?;

    let subject = resolve_subject(&context, &config, &args.target, args.environment.as_deref())?;
    context.log_verbose(&format!("Resolved subject: {subject:?}"));

    let explanation = build_why(&snapshot, &subject)?;

    if context.json {
        emit_json(&WhyDocument {
            schema_version: crate::core::render::JSON_SCHEMA_VERSION,
            why: explanation,
        })
    } else {
        println!("{}", render_why(&explanation));
        Ok(())
    }
}

/// The `--json` document: `{"schema_version", "why"}`.
///
/// Two keys, not P6's `{plan, receipt}`, for the reason recorded in the P7
/// plan: a mutation has two halves and a read-only view has one, and a `null`
/// receipt would say "nothing happened" — true, and useless.
#[derive(serde::Serialize)]
struct WhyDocument {
    schema_version: u32,
    why: WhyExplanation,
}

/// Decide what the reader asked about.
///
/// This is the only place in `why` that touches git, and it touches it once: a
/// name that is neither a configured environment nor a declared feature might
/// still be a real branch hitch simply has not been asked to promote, and
/// "not promoted to any environment" is a much better answer than "no such
/// branch" for a name that resolves. The reverse is not true — a name that does
/// not resolve anywhere is not a branch, and saying so beats printing an empty
/// grid.
fn resolve_subject(
    context: &GlobalContext,
    config: &HitchConfig,
    target: &str,
    environment: Option<&str>,
) -> Result<WhySubject> {
    if let Some(environment) = environment {
        if !config.environments.contains_key(environment) {
            anyhow::bail!(
                "No environment named '{}'.\n\
                 Configured environments: {}\n\n\
                 Run 'hitch status' to see what hitch knows about.",
                environment,
                known_environments(config)
            );
        }
        return Ok(WhySubject::FeatureIn(
            target.to_string(),
            environment.to_string(),
        ));
    }

    let is_environment = config.environments.contains_key(target);
    let is_declared_feature = config
        .environments
        .values()
        .any(|e| e.branches.iter().any(|b| b == target));

    match (is_environment, is_declared_feature) {
        // A name that answers to two questions gets both of them named, and
        // each one's way of being asked. Guessing would be the opposite of
        // resolving the ambiguity explicitly, and the two readings show
        // genuinely different things.
        //
        // Note what this error *cannot* do: it cannot offer `hitch why <name>`
        // for the environment reading, because that is the command that just
        // failed. The second positional is the only disambiguator §14.1's forms
        // allow, and it disambiguates in one direction only — so the
        // environment reading points at the view that does show it. Offering a
        // command here that would hit this same error would be a worse answer
        // than saying plainly that there is not one.
        (true, true) => anyhow::bail!(
            "'{target}' is both an environment and a promoted branch, so 'hitch why {target}' is ambiguous.\n\n\
             To explain the branch {target} in an environment:\n  \
               hitch why {target} <environment>\n\
             To explain the environment {target}:\n  \
               hitch status --environments {target}\n\n\
             Renaming one of the two would make both reachable here.",
            target = target
        ),
        (true, false) => Ok(WhySubject::Environment(target.to_string())),
        // Not an environment, so the only remaining question is whether it is a
        // branch at all. A declared branch does not need this check — it is
        // known to exist as far as the declaration is concerned — but a name
        // hitch has never heard of does, because the difference between "a
        // branch hitch has not been asked about" and "not a branch" is the whole
        // answer.
        (false, false) => {
            let resolves = context
                .git()
                .rev_parse_opt(&format!("refs/heads/{target}"))?
                .is_some();
            if resolves {
                Ok(WhySubject::Feature(target.to_string()))
            } else {
                anyhow::bail!(
                    "'{}' is not an environment and not a branch.\n\
                     Configured environments: {}\n\n\
                     Run 'hitch status' to see what hitch knows about.",
                    target,
                    known_environments(config)
                )
            }
        }
        (false, true) => Ok(WhySubject::Feature(target.to_string())),
    }
}

fn known_environments(config: &HitchConfig) -> String {
    let mut names: Vec<&str> = config.environments.keys().map(|k| k.as_str()).collect();
    names.sort_unstable();
    if names.is_empty() {
        "(none)".to_string()
    } else {
        names.join(", ")
    }
}
