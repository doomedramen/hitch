use crate::commands::global_context::GlobalContext;
use crate::types::{ApprovalRequest, ApprovalStatus, HitchConfig};
use anyhow::Result;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineKind {
    GitCommit,
    HitchEvent,
}

#[derive(Debug, Clone)]
pub struct TimelineItem {
    pub when: DateTime<Utc>,
    pub kind: TimelineKind,
    pub summary: String,
    #[allow(dead_code)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitchEventScope {
    #[allow(dead_code)]
    Any,
    Environment,
    Branch,
}

#[derive(Debug, Clone)]
pub struct HitchEventFilter<'a> {
    pub scope: HitchEventScope,
    pub environment: Option<&'a str>,
    pub branch: Option<&'a str>,
}

impl<'a> HitchEventFilter<'a> {
    #[allow(dead_code)]
    pub fn any() -> Self {
        Self {
            scope: HitchEventScope::Any,
            environment: None,
            branch: None,
        }
    }
}

pub fn build_combined_timeline(
    context: &GlobalContext,
    reference: &str,
    commit_limit: usize,
    hitch_limit: usize,
    hitch_filter: HitchEventFilter<'_>,
) -> Result<Vec<TimelineItem>> {
    let mut items = Vec::new();

    // Git commits
    if let Ok(commits) = context.git().list_commits(reference, commit_limit) {
        for c in commits {
            items.push(TimelineItem {
                when: c.timestamp,
                kind: TimelineKind::GitCommit,
                summary: format!("{} {}", &c.sha[..7.min(c.sha.len())], c.summary),
                detail: None,
            });
        }
    }

    // Hitch events from hitch-metadata history
    items.extend(build_hitch_events(context, hitch_limit, hitch_filter)?);

    items.sort_by(|a, b| b.when.cmp(&a.when));
    Ok(items)
}

pub fn build_hitch_events(
    context: &GlobalContext,
    max_commits: usize,
    filter: HitchEventFilter<'_>,
) -> Result<Vec<TimelineItem>> {
    // We rely on hitch-metadata existing locally (matches current CLI health checks).
    let shas = list_metadata_commits(context, max_commits)?;
    if shas.len() < 2 {
        return Ok(Vec::new());
    }

    let mut events = Vec::new();
    // `git log` returns newest -> oldest. Iterate oldest -> newest so diffs make sense, but
    // timestamp will be from the "new" commit.
    let mut shas = shas;
    shas.reverse();

    // Read/parse hitch.json once per commit (instead of twice per diff window).
    let mut prev_cfg = read_config_at(context, &shas[0])?;
    for sha in shas.iter().skip(1) {
        let next_cfg = read_config_at(context, sha)?;
        let when = context.git().get_commit_timestamp(sha)?;
        events.extend(diff_configs_to_events(&prev_cfg, &next_cfg, when, &filter));
        prev_cfg = next_cfg;
    }

    Ok(events)
}

fn list_metadata_commits(context: &GlobalContext, max_commits: usize) -> Result<Vec<String>> {
    let output = context.git().run_git_command(&[
        "log",
        "-n",
        &max_commits.to_string(),
        "--format=%H",
        "hitch-metadata",
    ])?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn read_config_at(context: &GlobalContext, spec: &str) -> Result<HitchConfig> {
    let json = context.git().read_file_from_branch(spec, "hitch.json")?;
    Ok(serde_json::from_str(&json)?)
}

fn matches_filter(summary: &str, filter: &HitchEventFilter<'_>) -> bool {
    match filter.scope {
        HitchEventScope::Any => true,
        HitchEventScope::Environment => filter.environment.is_some_and(|env| summary.contains(env)),
        HitchEventScope::Branch => filter.branch.is_some_and(|b| summary.contains(b)),
    }
}

fn diff_configs_to_events(
    old_cfg: &HitchConfig,
    new_cfg: &HitchConfig,
    when: DateTime<Utc>,
    filter: &HitchEventFilter<'_>,
) -> Vec<TimelineItem> {
    let mut out = Vec::new();

    // Environment-level diffs
    for (env_name, new_env) in &new_cfg.environments {
        let old_env = old_cfg.environments.get(env_name);

        // Added env
        if old_env.is_none() {
            push_if_match(
                &mut out,
                when,
                format!("Created environment '{}'", env_name),
                None,
                filter,
            );
            continue;
        }
        let old_env = old_env.unwrap();

        // Base change
        if old_env.base != new_env.base {
            push_if_match(
                &mut out,
                when,
                format!(
                    "Env '{}' base changed: {} → {}",
                    env_name, old_env.base, new_env.base
                ),
                None,
                filter,
            );
        }

        // Promote/demote diffs
        for added in new_env
            .branches
            .iter()
            .filter(|b| !old_env.branches.contains(b))
        {
            push_if_match(
                &mut out,
                when,
                format!("Promoted '{}' → {}", added, env_name),
                None,
                filter,
            );
        }
        for removed in old_env
            .branches
            .iter()
            .filter(|b| !new_env.branches.contains(b))
        {
            push_if_match(
                &mut out,
                when,
                format!("Demoted '{}' ← {}", removed, env_name),
                None,
                filter,
            );
        }

        // Lock/unlock
        if old_env.locked != new_env.locked {
            if new_env.locked {
                let by = new_env
                    .locked_by
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                push_if_match(
                    &mut out,
                    when,
                    format!("Locked env '{}' by {}", env_name, by),
                    None,
                    filter,
                );
            } else {
                push_if_match(
                    &mut out,
                    when,
                    format!("Unlocked env '{}'", env_name),
                    None,
                    filter,
                );
            }
        }

        // Rebuild/release timestamps
        if old_env.rebuilt_at != new_env.rebuilt_at {
            push_if_match(
                &mut out,
                when,
                format!("Rebuilt env '{}'", env_name),
                None,
                filter,
            );
        }
        if old_env.released_at != new_env.released_at {
            push_if_match(
                &mut out,
                when,
                format!("Released env '{}'", env_name),
                None,
                filter,
            );
        }
    }

    // Removed envs
    for env_name in old_cfg.environments.keys() {
        if !new_cfg.environments.contains_key(env_name) {
            push_if_match(
                &mut out,
                when,
                format!("Removed environment '{}'", env_name),
                None,
                filter,
            );
        }
    }

    // Approval request diffs
    out.extend(diff_approvals(old_cfg, new_cfg, when, filter));

    out
}

fn diff_approvals(
    old_cfg: &HitchConfig,
    new_cfg: &HitchConfig,
    when: DateTime<Utc>,
    filter: &HitchEventFilter<'_>,
) -> Vec<TimelineItem> {
    let mut out = Vec::new();

    let old_by_id: std::collections::HashMap<_, _> = old_cfg
        .approval_requests
        .iter()
        .map(|r| (r.id.clone(), r))
        .collect();
    let new_by_id: std::collections::HashMap<_, _> = new_cfg
        .approval_requests
        .iter()
        .map(|r| (r.id.clone(), r))
        .collect();

    for (id, req) in &new_by_id {
        if !old_by_id.contains_key(id) {
            push_if_match(
                &mut out,
                when,
                format!(
                    "Approval request created: {} {} → {} ({})",
                    req.operation, req.branch, req.environment, req.id
                ),
                None,
                filter,
            );
        }
    }

    for (id, new_req) in &new_by_id {
        if let Some(old_req) = old_by_id.get(id) {
            if old_req.status != new_req.status {
                push_if_match(
                    &mut out,
                    when,
                    format!(
                        "Approval {}: {} ({})",
                        status_word(new_req.status),
                        approval_short(new_req),
                        new_req.id
                    ),
                    None,
                    filter,
                );
            }

            if new_req.approvals.len() > old_req.approvals.len() {
                let delta = new_req.approvals.len() - old_req.approvals.len();
                push_if_match(
                    &mut out,
                    when,
                    format!(
                        "Approval +{} for {} ({})",
                        delta,
                        approval_short(new_req),
                        new_req.id
                    ),
                    None,
                    filter,
                );
            }

            if old_req.rejection.is_none() && new_req.rejection.is_some() {
                push_if_match(
                    &mut out,
                    when,
                    format!(
                        "Approval rejected: {} ({})",
                        approval_short(new_req),
                        new_req.id
                    ),
                    None,
                    filter,
                );
            }
        }
    }

    out
}

fn status_word(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Applied => "applied",
        ApprovalStatus::Rejected => "rejected",
        ApprovalStatus::Cancelled => "cancelled",
    }
}

fn approval_short(req: &ApprovalRequest) -> String {
    format!("{} {} → {}", req.operation, req.branch, req.environment)
}

fn push_if_match(
    out: &mut Vec<TimelineItem>,
    when: DateTime<Utc>,
    summary: String,
    detail: Option<String>,
    filter: &HitchEventFilter<'_>,
) {
    if matches_filter(&summary, filter) {
        out.push(TimelineItem {
            when,
            kind: TimelineKind::HitchEvent,
            summary,
            detail,
        });
    }
}
