use crate::commands::global_context::GlobalContext;
use crate::utils::resolutions;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;

#[derive(Args)]
pub struct ResolutionsCommand {
    #[command(subcommand)]
    pub action: ResolutionsAction,
}

#[derive(Subcommand)]
pub enum ResolutionsAction {
    /// List every recorded conflict resolution reachable locally
    List,
    /// Show one resolution's details (metadata + resolved file contents)
    Show {
        /// The resolution key (as shown by `list`)
        key: String,
    },
    /// Delete a recorded resolution ref locally
    Forget {
        /// The resolution key to forget
        key: String,
    },
    /// Fetch shared resolutions from origin into local refs
    Fetch,
}

pub fn run(cmd: ResolutionsCommand, context: &GlobalContext) -> Result<()> {
    crate::utils::prelude::pre_check_repo_only(context)?;

    match cmd.action {
        ResolutionsAction::List => list(context),
        ResolutionsAction::Show { key } => show(context, &key),
        ResolutionsAction::Forget { key } => forget(context, &key),
        ResolutionsAction::Fetch => fetch(context),
    }
}

fn list(context: &GlobalContext) -> Result<()> {
    let all = resolutions::list_resolutions(context.git())?;
    if all.is_empty() {
        context.log_info("No recorded resolutions.");
        return Ok(());
    }

    println!(
        "\n{} recorded resolution{}\n",
        all.len().to_string().bright_cyan(),
        if all.len() == 1 { "" } else { "s" }
    );
    for r in &all {
        let m = &r.meta;
        println!(
            "  {} {} {} {}",
            m.key[..12.min(m.key.len())].dimmed(),
            m.branch.bold(),
            "vs".dimmed(),
            m.conflicts_with
        );
        println!(
            "      {} {} · recorded by {} at {}",
            format!("env {}", m.env).dimmed(),
            format!("{} file(s)", m.files.len()).dimmed(),
            m.recorded_by.dimmed(),
            m.recorded_at.dimmed()
        );
    }
    println!();
    Ok(())
}

fn show(context: &GlobalContext, key: &str) -> Result<()> {
    let key = resolve_key_prefix(context, key)?;
    let Some(r) = resolutions::load_resolution(context.git(), &key)? else {
        return Err(anyhow::anyhow!("No resolution with key '{}'", key));
    };
    let m = &r.meta;

    println!("\n{}", format!("Resolution {}", m.key).bold());
    println!("  environment:    {}", m.env);
    println!(
        "  branch:         {} (was @ {})",
        m.branch, m.source_branch_head
    );
    println!("  conflicts with: {}", m.conflicts_with);
    println!("  recorded by:    {}", m.recorded_by);
    println!("  recorded at:    {}", m.recorded_at);
    println!("  hitch version:  {}", m.hitch_version);
    println!("  commit:         {}", r.commit_oid);
    println!("\n  {} resolved file(s):", m.files.len());
    for (path, content) in &r.resolved {
        println!("\n  {} {}", "──".dimmed(), path.bold());
        match std::str::from_utf8(content) {
            Ok(text) => {
                for line in text.lines() {
                    println!("    {}", line);
                }
            }
            Err(_) => println!("    (binary, {} bytes)", content.len()),
        }
    }
    println!();
    Ok(())
}

fn forget(context: &GlobalContext, key: &str) -> Result<()> {
    let key = resolve_key_prefix(context, key)?;
    let refname = format!("{}{}", resolutions::RESOLUTIONS_REF_PREFIX, key);
    context.git().delete_ref(&refname)?;
    context.log_success(&format!("✓ Forgot resolution {}", key));
    context.log_info(
        "This only deletes the local ref. If it was shared, delete it on origin too: \
         git push origin --delete <refname>",
    );
    Ok(())
}

fn fetch(context: &GlobalContext) -> Result<()> {
    context.log_info("Fetching shared resolutions from origin...");
    context
        .git()
        .fetch_refspec(resolutions::RESOLUTIONS_REFSPEC)?;
    let all = resolutions::list_resolutions(context.git())?;
    context.log_success(&format!(
        "✓ {} resolution(s) now available locally.",
        all.len()
    ));
    Ok(())
}

/// Accept a unique key prefix (as `list` displays the first 12 chars) and
/// resolve it to the full key, erroring on no match or an ambiguous one.
fn resolve_key_prefix(context: &GlobalContext, prefix: &str) -> Result<String> {
    // Exact match short-circuits (avoids listing when the full key is given).
    let refname = format!("{}{}", resolutions::RESOLUTIONS_REF_PREFIX, prefix);
    if context.git().rev_parse_opt(&refname)?.is_some() {
        return Ok(prefix.to_string());
    }

    let matches: Vec<String> = resolutions::list_resolutions(context.git())?
        .into_iter()
        .map(|r| r.meta.key)
        .filter(|k| k.starts_with(prefix))
        .collect();

    match matches.len() {
        0 => Err(anyhow::anyhow!("No resolution matching '{}'", prefix)),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(anyhow::anyhow!(
            "'{}' is ambiguous — matches {} resolutions. Use more characters.",
            prefix,
            matches.len()
        )),
    }
}
