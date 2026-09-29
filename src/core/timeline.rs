use crate::commands::global_context::GlobalContext;
use crate::core::activity::{build_activity, ActivityLog, ActivityQuery, HitchEvent};
use crate::core::render::render_event;
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
    /// The typed event behind `summary`; `None` for `GitCommit` items.
    #[allow(dead_code)]
    pub event: Option<HitchEvent>,
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

impl HitchEventFilter<'_> {
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
                event: None,
            });
        }
    }

    // Hitch events from hitch-metadata history
    items.extend(build_hitch_events(context, hitch_limit, hitch_filter)?);

    items.sort_by_key(|item| std::cmp::Reverse(item.when));
    Ok(items)
}

/// Compatibility adapter over `build_activity`: one item per event, newest
/// entry first, worded by `render_event`.
///
/// `max_commits` (the desktop passes 80) now bounds activity *entries*, not
/// metadata commits scanned. The desktop's timeline content changes with the
/// event model: operation locks are collapsed (no Locked/Unlocked noise around
/// mutations), a rejection is one line, and sentences use the new wording.
/// Filtering is by exact environment/branch, not summary substring.
pub fn build_hitch_events(
    context: &GlobalContext,
    max_commits: usize,
    filter: HitchEventFilter<'_>,
) -> Result<Vec<TimelineItem>> {
    let query = match filter.scope {
        HitchEventScope::Any => ActivityQuery {
            limit: max_commits,
            ..Default::default()
        },
        HitchEventScope::Environment => match filter.environment {
            Some(env) => ActivityQuery {
                environment: Some(env.to_string()),
                limit: max_commits,
                ..Default::default()
            },
            None => return Ok(Vec::new()),
        },
        HitchEventScope::Branch => match filter.branch {
            Some(b) => ActivityQuery {
                branch: Some(b.to_string()),
                limit: max_commits,
                ..Default::default()
            },
            None => return Ok(Vec::new()),
        },
    };
    let log = build_activity(context, &query)?;
    Ok(items_from_log(&log, &filter))
}

fn items_from_log(log: &ActivityLog, filter: &HitchEventFilter<'_>) -> Vec<TimelineItem> {
    log.entries
        .iter()
        .flat_map(|entry| {
            entry
                .events
                .iter()
                .filter(|e| event_matches(e, filter))
                .map(|e| TimelineItem {
                    when: entry.when,
                    kind: TimelineKind::HitchEvent,
                    summary: render_event(e),
                    detail: None,
                    event: Some(e.clone()),
                })
        })
        .collect()
}

fn event_matches(event: &HitchEvent, filter: &HitchEventFilter<'_>) -> bool {
    match filter.scope {
        HitchEventScope::Any => true,
        HitchEventScope::Environment => {
            filter.environment.is_some_and(|e| event.environment() == e)
        }
        HitchEventScope::Branch => filter.branch.is_some_and(|b| event.branches().contains(&b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::activity::ActivityEntry;

    fn log_with_promotion() -> ActivityLog {
        ActivityLog {
            entries: vec![ActivityEntry {
                commit: "abc".into(),
                when: Utc::now(),
                actor: "a@b.c".into(),
                events: vec![HitchEvent::Promoted {
                    environment: "qa".into(),
                    branch: "feature/devtools".into(),
                }],
            }],
            skipped: Vec::new(),
            truncated: false,
            branch_filtered: false,
        }
    }

    fn filter<'a>(
        scope: HitchEventScope,
        environment: Option<&'a str>,
        branch: Option<&'a str>,
    ) -> HitchEventFilter<'a> {
        HitchEventFilter {
            scope,
            environment,
            branch,
        }
    }

    #[test]
    fn environment_scope_matches_exactly_not_by_substring() {
        let log = log_with_promotion();
        // "dev" is a substring of the branch name but not the environment.
        let f = filter(HitchEventScope::Environment, Some("dev"), None);
        assert!(items_from_log(&log, &f).is_empty());

        let f = filter(HitchEventScope::Environment, Some("qa"), None);
        let items = items_from_log(&log, &f);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, TimelineKind::HitchEvent);
        assert!(items[0].event.is_some());
        assert_eq!(
            items[0].summary,
            render_event(items[0].event.as_ref().unwrap())
        );
    }

    #[test]
    fn branch_scope_and_any_scope() {
        let log = log_with_promotion();
        let f = filter(HitchEventScope::Branch, None, Some("feature/devtools"));
        assert_eq!(items_from_log(&log, &f).len(), 1);
        let f = filter(HitchEventScope::Branch, None, Some("devtools"));
        assert!(items_from_log(&log, &f).is_empty());
        assert_eq!(items_from_log(&log, &HitchEventFilter::any()).len(), 1);
    }
}
