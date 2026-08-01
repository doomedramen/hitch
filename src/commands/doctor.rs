use crate::commands::global_context::GlobalContext;
use crate::utils::{gh, publish_journal, resolutions};
use anyhow::Result;
use clap::Args;

/// Scopes `hitch pr` needs from a classic OAuth token (it shells out to
/// `gh pr create`, which reads/writes the repository).
const RECOMMENDED_SCOPES: &[&str] = &["repo"];

#[derive(Args)]
pub struct DoctorCommand {
    /// Treat any recorded conflict resolution older than this many days as a
    /// hard failure (nonzero exit), for CI gating. Recorded resolutions are
    /// convenience, not a durable fix — this is the forcing function that
    /// keeps them from quietly becoming permanent load-bearing
    /// infrastructure. Without it, resolution debt is only reported.
    #[arg(long)]
    pub max_resolution_age_days: Option<i64>,
}

/// Diagnose whether the environment can support `hitch pr` (i.e. whether
/// `gh` is installed, authenticated, and holds the scopes that command
/// needs), and report/gate recorded-resolution debt.
pub fn run(args: DoctorCommand, context: &GlobalContext) -> Result<()> {
    // Resolution-debt check first, so it runs even when gh is entirely
    // absent (the two concerns are independent).
    check_resolution_debt(context, args.max_resolution_age_days)?;
    check_pending_publishes(context)?;

    context.log_info("Checking GitHub CLI ('gh') setup for 'hitch pr'...");

    let mut problems = 0usize;

    let gh_path = match gh::find_gh() {
        Some(path) => {
            context.log_success(&format!("gh found on PATH ({})", path));
            path
        }
        None => {
            context.log_error(
                "gh not found on PATH. Install it from https://cli.github.com/ — \
                 'hitch pr' shells out to it to open pull requests.",
            );
            return summarize(context, problems + 1);
        }
    };

    let status = gh::check_auth_status(&gh_path);

    if !status.authenticated || status.accounts.is_empty() {
        context.log_error(
            "gh is not authenticated to any GitHub host. Run 'gh auth login' — \
             'hitch pr' will fail without it.",
        );
        if !status.raw_output.trim().is_empty() {
            context.log_info(&format!(
                "Raw output from 'gh auth status':\n{}",
                indent(&status.raw_output)
            ));
        }
        return summarize(context, problems + 1);
    }

    for account in &status.accounts {
        let who = account.account.as_deref().unwrap_or("unknown account");
        let active_note = if account.active { " (active)" } else { "" };
        context.log_success(&format!(
            "Authenticated to {} as {}{}",
            account.host, who, active_note
        ));

        if account.scopes.is_empty() {
            context.log_info(&format!(
                "  No classic token scopes reported for {} — likely a fine-grained \
                 token or GitHub App auth, which doesn't expose scopes this way. \
                 Skipping scope checks.",
                account.host
            ));
            continue;
        }

        context.log_info(&format!("  Scopes: {}", account.scopes.join(", ")));

        let missing: Vec<&str> = RECOMMENDED_SCOPES
            .iter()
            .filter(|needed| !account.scopes.iter().any(|s| s == *needed))
            .copied()
            .collect();

        if !missing.is_empty() {
            problems += 1;
            context.log_warning(&format!(
                "  Missing recommended scope(s) for {}: {} — 'hitch pr' may fail. \
                 Run: gh auth refresh -h {} -s {}",
                account.host,
                missing.join(", "),
                account.host,
                missing.join(",")
            ));
        }
    }

    summarize(context, problems)
}

/// Report every recorded resolution and its age. With
/// `--max-resolution-age-days`, any resolution older than the threshold is a
/// hard failure so CI can gate on stale conflict debt.
fn check_resolution_debt(context: &GlobalContext, max_age_days: Option<i64>) -> Result<()> {
    let all = resolutions::list_resolutions(context.git())?;
    if all.is_empty() {
        context.log_verbose("No recorded conflict resolutions.");
        return Ok(());
    }

    let now = chrono::Utc::now();
    let mut stale = Vec::new();
    context.log_info(&format!("{} recorded conflict resolution(s):", all.len()));
    for r in &all {
        let age_days = age_in_days(&r.meta.recorded_at, now);
        let age_str = age_days
            .map(|d| format!("{}d old", d))
            .unwrap_or_else(|| "age unknown".to_string());
        context.log_info(&format!(
            "  {} {} vs {} ({}, env {}) — {}, {}",
            &r.meta.key[..12.min(r.meta.key.len())],
            r.meta.branch,
            r.meta.conflicts_with,
            age_str,
            r.meta.env,
            r.meta.recorded_by,
            format_source_branch_head_note(&r.meta.source_branch_head)
        ));
        if let (Some(max), Some(age)) = (max_age_days, age_days) {
            if age > max {
                stale.push((r.meta.key.clone(), r.meta.branch.clone(), age));
            }
        }
    }

    context.log_info(
        "Recorded resolutions are a convenience, not a durable fix — carry the change back \
         into the feature branch and 'hitch resolutions forget' it to retire the debt.",
    );

    if let Some(max) = max_age_days {
        if !stale.is_empty() {
            let mut msg = format!(
                "{} recorded resolution(s) older than {} day(s) — retire them (carry the fix \
                 back into the branch, then 'hitch resolutions forget'):\n",
                stale.len(),
                max
            );
            for (key, branch, age) in &stale {
                msg.push_str(&format!(
                    "  {} ({}) — {} days old\n",
                    &key[..12.min(key.len())],
                    branch,
                    age
                ));
            }
            return Err(anyhow::anyhow!(msg));
        }
        context.log_success(&format!(
            "No recorded resolution is older than {} day(s).",
            max
        ));
    }

    Ok(())
}

/// Report every unresolved publish-journal record — a publish that moved a
/// branch but never finished resyncing or pushing. Each is attributed to the
/// process that made it, so a wedge is traceable without correlating logs.
fn check_pending_publishes(context: &GlobalContext) -> Result<()> {
    let pending = publish_journal::list(context.git())?;
    if pending.is_empty() {
        context.log_verbose("No pending publish-journal records.");
        return Ok(());
    }

    context.log_warning(&format!(
        "{} unresolved publish record(s) — a previous publish did not finish:",
        pending.len()
    ));
    for (_, record) in &pending {
        let who = record
            .initiated_by
            .as_ref()
            .map(|a| format!("pid {} on {} (started {})", a.pid, a.hostname, a.started_at))
            .unwrap_or_else(|| "unknown process".to_string());
        context.log_warning(&format!(
            "  '{}' generation {} — {} — from {} to {}{}",
            record.branch,
            record.generation,
            who,
            record.from_sha.as_deref().unwrap_or("(none)"),
            record.to_sha,
            if record.push_owed {
                ", push still owed"
            } else {
                ""
            }
        ));
    }
    context.log_info(
        "Recovery for these runs automatically on the next mutating hitch command. If one \
         keeps reappearing, the process attributed above is worth checking.",
    );

    Ok(())
}

/// A short note naming the commit a resolution was recorded against, for
/// the per-resolution report line in `check_resolution_debt` — makes the
/// lineage check Task 1 of this plan added to replay visible at a glance,
/// without requiring a separate doctor pass.
fn format_source_branch_head_note(source_branch_head: &str) -> String {
    format!(
        "recorded against {}",
        &source_branch_head[..12.min(source_branch_head.len())]
    )
}

/// Age in whole days of an RFC3339 timestamp relative to `now`, or `None` if
/// it doesn't parse (a malformed timestamp is treated as "unknown age", never
/// as stale, so a parse quirk can't trip the CI gate).
fn age_in_days(recorded_at: &str, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(recorded_at)
        .ok()
        .map(|t| (now - t.with_timezone(&chrono::Utc)).num_days())
}

fn summarize(context: &GlobalContext, problems: usize) -> Result<()> {
    if problems == 0 {
        context.log_success("All checks passed — 'hitch pr' should work.");
    } else {
        context.log_warning(&format!(
            "{} issue(s) found above; 'hitch pr' may not work until they're resolved.",
            problems
        ));
    }
    Ok(())
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::age_in_days;
    use chrono::{Duration, Utc};

    #[test]
    fn source_branch_head_report_line_names_the_recorded_commit() {
        // format_source_branch_head_note is a pure formatting function —
        // test its output shape directly rather than standing up a full
        // repo + GlobalContext for a one-line report addition.
        let note = super::format_source_branch_head_note("abcdef1234567890");
        assert!(note.contains("abcdef123456"));
    }

    #[test]
    fn age_in_days_computes_and_gates() {
        let now = Utc::now();
        let ten_days_ago = (now - Duration::days(10)).to_rfc3339();
        let age = age_in_days(&ten_days_ago, now).expect("parses");
        assert_eq!(age, 10);

        // Gate logic: age > max is stale.
        assert!(age > 7, "10 days should breach a 7-day SLA");
        assert!(age <= 30, "10 days should pass a 30-day SLA");

        // A fresh resolution is 0 days old and never breaches.
        let fresh = now.to_rfc3339();
        assert_eq!(age_in_days(&fresh, now), Some(0));

        // Unparseable timestamp is "unknown age" (None), never stale.
        assert_eq!(age_in_days("not-a-date", now), None);
    }
}
