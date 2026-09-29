//! The typed event model for `hitch log`: what changed between two configs.
//!
//! Pure: two `HitchConfig` values in, events out. No git, no clock, no words —
//! wording belongs to `core::render`.

use crate::commands::global_context::GlobalContext;
use crate::operations::model::HoldPair;
use crate::types::{ApprovalRequest, ApprovalStatus, HitchConfig, LockPurpose, Operation};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitchEvent {
    EnvironmentCreated {
        environment: String,
        base: String,
    },
    EnvironmentRemoved {
        environment: String,
    },
    BaseChanged {
        environment: String,
        from: String,
        to: String,
    },
    Promoted {
        environment: String,
        branch: String,
    },
    Demoted {
        environment: String,
        branch: String,
    },
    Locked {
        environment: String,
        by: Option<String>,
    },
    Unlocked {
        environment: String,
    },
    Rebuilt {
        environment: String,
        outcome: RebuildOutcome,
    },
    Released {
        environment: String,
    },
    ApprovalRequested {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
    ApprovalVoted {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
        approvals: usize,
        required: usize,
    },
    ApprovalGranted {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
    ApprovalRejected {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
    ApprovalApplied {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
    ApprovalCancelled {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
}

/// The event's own vocabulary for `Operation`, which serializes PascalCase
/// because it is persisted in `hitch.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDirection {
    Promote,
    Demote,
}

impl From<Operation> for ApprovalDirection {
    fn from(op: Operation) -> Self {
        match op {
            Operation::Promote => ApprovalDirection::Promote,
            Operation::Demote => ApprovalDirection::Demote,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    /// No build record can be proven to describe this rebuild. Never read as "clean".
    Unrecorded,
    Clean {
        included: Vec<String>,
    },
    WithHolds {
        included: Vec<String>,
        held: Vec<HoldPair>,
    },
}

impl HitchEvent {
    pub fn environment(&self) -> &str {
        match self {
            HitchEvent::EnvironmentCreated { environment, .. }
            | HitchEvent::EnvironmentRemoved { environment }
            | HitchEvent::BaseChanged { environment, .. }
            | HitchEvent::Promoted { environment, .. }
            | HitchEvent::Demoted { environment, .. }
            | HitchEvent::Locked { environment, .. }
            | HitchEvent::Unlocked { environment }
            | HitchEvent::Rebuilt { environment, .. }
            | HitchEvent::Released { environment }
            | HitchEvent::ApprovalRequested { environment, .. }
            | HitchEvent::ApprovalVoted { environment, .. }
            | HitchEvent::ApprovalGranted { environment, .. }
            | HitchEvent::ApprovalRejected { environment, .. }
            | HitchEvent::ApprovalApplied { environment, .. }
            | HitchEvent::ApprovalCancelled { environment, .. } => environment,
        }
    }

    pub fn branches(&self) -> Vec<&str> {
        match self {
            HitchEvent::Promoted { branch, .. }
            | HitchEvent::Demoted { branch, .. }
            | HitchEvent::ApprovalRequested { branch, .. }
            | HitchEvent::ApprovalVoted { branch, .. }
            | HitchEvent::ApprovalGranted { branch, .. }
            | HitchEvent::ApprovalRejected { branch, .. }
            | HitchEvent::ApprovalApplied { branch, .. }
            | HitchEvent::ApprovalCancelled { branch, .. } => vec![branch],
            HitchEvent::Rebuilt { outcome, .. } => match outcome {
                RebuildOutcome::Unrecorded => Vec::new(),
                RebuildOutcome::Clean { included } => included.iter().map(String::as_str).collect(),
                RebuildOutcome::WithHolds { included, held } => included
                    .iter()
                    .map(String::as_str)
                    .chain(held.iter().map(|h| h.branch.as_str()))
                    .collect(),
            },
            HitchEvent::EnvironmentCreated { .. }
            | HitchEvent::EnvironmentRemoved { .. }
            | HitchEvent::BaseChanged { .. }
            | HitchEvent::Locked { .. }
            | HitchEvent::Unlocked { .. }
            | HitchEvent::Released { .. } => Vec::new(),
        }
    }
}

/// Every event that turns `old` into `new`, in a fixed order: environments by
/// name, then removed environments by name, then approval requests by id.
/// `HitchConfig::environments` is a `HashMap`, so map order would make the same
/// commit list its events differently between runs.
///
/// Lock changes are emitted faithfully; deciding whether a lock belongs to an
/// operation is the reader's job, since it can see neighbouring commits.
pub fn derive_events(old: &HitchConfig, new: &HitchConfig) -> Vec<HitchEvent> {
    let mut out = Vec::new();

    let mut names: Vec<&String> = new.environments.keys().collect();
    names.sort();
    for name in names {
        let new_env = &new.environments[name];
        let Some(old_env) = old.environments.get(name) else {
            out.push(HitchEvent::EnvironmentCreated {
                environment: name.clone(),
                base: new_env.base.clone(),
            });
            continue;
        };

        if old_env.base != new_env.base {
            out.push(HitchEvent::BaseChanged {
                environment: name.clone(),
                from: old_env.base.clone(),
                to: new_env.base.clone(),
            });
        }
        for added in new_env
            .branches
            .iter()
            .filter(|b| !old_env.branches.contains(b))
        {
            out.push(HitchEvent::Promoted {
                environment: name.clone(),
                branch: added.clone(),
            });
        }
        for removed in old_env
            .branches
            .iter()
            .filter(|b| !new_env.branches.contains(b))
        {
            out.push(HitchEvent::Demoted {
                environment: name.clone(),
                branch: removed.clone(),
            });
        }
        if old_env.locked != new_env.locked {
            out.push(if new_env.locked {
                HitchEvent::Locked {
                    environment: name.clone(),
                    by: new_env.locked_by.clone(),
                }
            } else {
                HitchEvent::Unlocked {
                    environment: name.clone(),
                }
            });
        }
        if old_env.rebuilt_at != new_env.rebuilt_at {
            out.push(HitchEvent::Rebuilt {
                environment: name.clone(),
                outcome: RebuildOutcome::Unrecorded,
            });
        }
        if old_env.released_at != new_env.released_at {
            out.push(HitchEvent::Released {
                environment: name.clone(),
            });
        }
    }

    let mut removed: Vec<&String> = old
        .environments
        .keys()
        .filter(|n| !new.environments.contains_key(*n))
        .collect();
    removed.sort();
    for name in removed {
        out.push(HitchEvent::EnvironmentRemoved {
            environment: name.clone(),
        });
    }

    let mut requests: Vec<&ApprovalRequest> = new.approval_requests.iter().collect();
    requests.sort_by(|a, b| a.id.cmp(&b.id));
    for req in requests {
        match old.approval_requests.iter().find(|r| r.id == req.id) {
            None => out.push(HitchEvent::ApprovalRequested {
                request_id: req.id.clone(),
                environment: req.environment.clone(),
                branch: req.branch.clone(),
                direction: req.operation.into(),
            }),
            Some(old_req) => approval_transition(old_req, req, new, &mut out),
        }
    }

    out
}

fn approval_transition(
    old_req: &ApprovalRequest,
    req: &ApprovalRequest,
    new: &HitchConfig,
    out: &mut Vec<HitchEvent>,
) {
    let id = || req.id.clone();
    let environment = || req.environment.clone();
    let branch = || req.branch.clone();

    if req.approvals.len() > old_req.approvals.len() {
        // Falls back to the vote count when the environment has been removed,
        // so a vote is never shown as "n of 0".
        let required = new
            .environments
            .get(&req.environment)
            .map_or(req.approvals.len(), |e| e.min_approvals);
        out.push(HitchEvent::ApprovalVoted {
            request_id: id(),
            environment: environment(),
            branch: branch(),
            direction: req.operation.into(),
            approvals: req.approvals.len(),
            required,
        });
    }

    // A rejection sets both `status` and `rejection`; one rejection is one event.
    let rejected_now = (old_req.status != req.status && req.status == ApprovalStatus::Rejected)
        || (old_req.rejection.is_none() && req.rejection.is_some());
    if rejected_now {
        out.push(HitchEvent::ApprovalRejected {
            request_id: id(),
            environment: environment(),
            branch: branch(),
            direction: req.operation.into(),
        });
    }

    if old_req.status != req.status {
        match req.status {
            ApprovalStatus::Approved => out.push(HitchEvent::ApprovalGranted {
                request_id: id(),
                environment: environment(),
                branch: branch(),
                direction: req.operation.into(),
            }),
            ApprovalStatus::Applied => out.push(HitchEvent::ApprovalApplied {
                request_id: id(),
                environment: environment(),
                branch: branch(),
                direction: req.operation.into(),
            }),
            ApprovalStatus::Cancelled => out.push(HitchEvent::ApprovalCancelled {
                request_id: id(),
                environment: environment(),
                branch: branch(),
                direction: req.operation.into(),
            }),
            ApprovalStatus::Rejected | ApprovalStatus::Pending => {}
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityEntry {
    pub commit: String,
    pub when: chrono::DateTime<chrono::Utc>,
    pub actor: String,
    pub events: Vec<HitchEvent>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkippedCommit {
    pub commit: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityLog {
    /// Newest first.
    pub entries: Vec<ActivityEntry>,
    pub skipped: Vec<SkippedCommit>,
    /// True when the walk stopped at `limit` before reaching the first commit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ActivityQuery {
    pub environment: Option<String>,
    pub branch: Option<String>,
    /// Maximum number of entries returned (not commits scanned). With `branch`
    /// set, rebuild entries that end up filtered out still count toward it.
    pub limit: usize,
}

pub(crate) fn read_config_at(context: &GlobalContext, spec: &str) -> anyhow::Result<HitchConfig> {
    let json = context.git().read_file_from_branch(spec, "hitch.json")?;
    Ok(serde_json::from_str(&json)?)
}

/// Lazily reads each commit's config at most once. An unreadable one is
/// recorded in `skipped` the first time it fails and never again.
struct ConfigReader<'a> {
    context: &'a GlobalContext,
    shas: Vec<&'a str>,
    cache: Vec<Option<Option<HitchConfig>>>,
    skipped: Vec<SkippedCommit>,
}

impl ConfigReader<'_> {
    fn get(&mut self, i: usize) -> Option<&HitchConfig> {
        if self.cache[i].is_none() {
            let read = match read_config_at(self.context, self.shas[i]) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    self.skipped.push(SkippedCommit {
                        commit: self.shas[i].to_string(),
                        reason: format!("{e:#}"),
                    });
                    None
                }
            };
            self.cache[i] = Some(read);
        }
        self.cache[i].as_ref().and_then(|c| c.as_ref())
    }
}

/// History written before `lock_purpose` existed cannot say which kind of lock
/// it recorded, so a lock released this quickly is assumed to be an operation's
/// own bracket. Only consulted when `lock_purpose` is absent.
const LEGACY_OPERATION_LOCK_WINDOW: chrono::Duration = chrono::Duration::seconds(60);

/// An entry under construction: the events a later, older commit may still
/// retract when it turns out to open a legacy operation bracket.
struct Draft {
    entry: ActivityEntry,
    alive: Vec<bool>,
    alive_count: usize,
}

fn lock_purpose_in(config: &HitchConfig, environment: &str) -> Option<LockPurpose> {
    config
        .environments
        .get(environment)
        .and_then(|e| e.lock_purpose)
}

fn drop_event(drafts: &mut [Draft], live_entries: &mut usize, draft: usize, event: usize) {
    let d = &mut drafts[draft];
    if d.alive[event] {
        d.alive[event] = false;
        d.alive_count -= 1;
        if d.alive_count == 0 {
            *live_entries -= 1;
        }
    }
}

/// Fills in the outcome of the newest `Rebuilt` event per environment, and only
/// when the live build record provably came from that rebuild. The record's
/// `metadata_sha` is a transient tip written after the rebuild's lock commit and
/// before its `rebuilt_at` stamp (`c1`), so it must be `c1` or an ancestor on
/// the first-parent line, and no commit strictly between the two may have
/// stamped `rebuilt_at` again (that would be a later rebuild that wrote no
/// record). The range check reads history beyond the walk, so a truncated walk
/// still attaches. Any doubt (unreadable commit, non-`Known` record, a
/// `metadata_sha` off the first-parent line) leaves the event `Unrecorded`; a
/// wrong attachment is worse than none.
fn attach_build_records(
    context: &GlobalContext,
    reader: &mut ConfigReader<'_>,
    entries: &mut [ActivityEntry],
) {
    use crate::utils::build_record::{read_state, EnvironmentBuildState};

    let git = context.git();
    let mut newest: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for (k, ev) in entry.events.iter().enumerate() {
            if let HitchEvent::Rebuilt { environment, .. } = ev {
                newest.entry(environment.clone()).or_insert((i, k));
            }
        }
    }
    for (environment, (i1, k1)) in newest {
        let Ok(EnvironmentBuildState::Known(record)) = read_state(git, &environment) else {
            continue;
        };
        let c1 = entries[i1].commit.as_str();
        let Some(pos1) = reader.shas.iter().position(|s| *s == c1) else {
            continue;
        };
        let Some(pos_m) = reader.shas.iter().position(|s| *s == record.metadata_sha) else {
            continue;
        };
        if pos_m < pos1 {
            continue;
        }
        let stamp = |cfg: &HitchConfig| cfg.environments.get(&environment).map(|e| e.rebuilt_at);
        let mut clean = true;
        for m in pos1 + 1..pos_m {
            let (Some(new), Some(old)) = (reader.get(m).map(stamp), reader.get(m + 1).map(stamp))
            else {
                clean = false;
                break;
            };
            if new != old {
                clean = false;
                break;
            }
        }
        if !clean {
            continue;
        }
        let included = record
            .included_branches
            .iter()
            .map(|b| b.branch.clone())
            .collect();
        let outcome = if record.held.is_empty() {
            RebuildOutcome::Clean { included }
        } else {
            RebuildOutcome::WithHolds {
                included,
                held: record.held.iter().map(HoldPair::from).collect(),
            }
        };
        if let HitchEvent::Rebuilt { outcome: slot, .. } = &mut entries[i1].events[k1] {
            *slot = outcome;
        }
    }
}

pub fn build_activity(
    context: &GlobalContext,
    query: &ActivityQuery,
) -> anyhow::Result<ActivityLog> {
    let history = context.git().list_first_parent_history("hitch-metadata")?;
    let mut reader = ConfigReader {
        context,
        shas: history.iter().map(|c| c.sha.as_str()).collect(),
        cache: vec![None; history.len()],
        skipped: Vec::new(),
    };
    let mut drafts: Vec<Draft> = Vec::new();
    let mut live_entries = 0usize;
    let mut truncated = false;
    // Per environment, the newest-seen legacy `Unlocked` still waiting for the
    // `Locked` that opened its bracket.
    let mut pending_unlock: std::collections::HashMap<
        String,
        (chrono::DateTime<chrono::Utc>, usize, usize),
    > = std::collections::HashMap::new();

    for (i, commit) in history.iter().enumerate() {
        if live_entries >= query.limit {
            truncated = true;
            break;
        }
        let Some(new) = reader.get(i).cloned() else {
            continue;
        };
        // Diff against the nearest older *readable* config, so an unreadable
        // commit shows up as a combined change rather than a phantom creation.
        // Only the true root of the history is diffed against nothing: if older
        // commits exist but none is readable, the gap is already in `skipped`.
        let old = if i + 1 == history.len() {
            HitchConfig::default()
        } else {
            let mut found = None;
            for j in i + 1..history.len() {
                if let Some(cfg) = reader.get(j) {
                    found = Some(cfg.clone());
                    break;
                }
            }
            match found {
                Some(cfg) => cfg,
                None => continue,
            }
        };
        let events: Vec<HitchEvent> = derive_events(&old, &new)
            .into_iter()
            .filter(|ev| {
                query
                    .environment
                    .as_deref()
                    .is_none_or(|e| ev.environment() == e)
                    && query.branch.as_deref().is_none_or(|b| {
                        // An unattached rebuild has no branch names yet; the
                        // branch filter runs after attachment gives it some.
                        matches!(
                            ev,
                            HitchEvent::Rebuilt {
                                outcome: RebuildOutcome::Unrecorded,
                                ..
                            }
                        ) || ev.branches().contains(&b)
                    })
            })
            .collect();
        if events.is_empty() {
            continue;
        }

        let draft_idx = drafts.len();
        let mut alive = vec![true; events.len()];
        for (k, ev) in events.iter().enumerate() {
            match ev {
                HitchEvent::Locked { environment, .. } => {
                    match lock_purpose_in(&new, environment) {
                        Some(LockPurpose::Operation) => alive[k] = false,
                        Some(LockPurpose::Manual) => {}
                        None => {
                            if let Some((unlocked_at, d, e)) = pending_unlock.remove(environment) {
                                if unlocked_at - commit.when <= LEGACY_OPERATION_LOCK_WINDOW {
                                    alive[k] = false;
                                    drop_event(&mut drafts, &mut live_entries, d, e);
                                }
                            }
                        }
                    }
                }
                HitchEvent::Unlocked { environment } => match lock_purpose_in(&old, environment) {
                    Some(LockPurpose::Operation) => alive[k] = false,
                    Some(LockPurpose::Manual) => {}
                    None => {
                        pending_unlock.insert(environment.clone(), (commit.when, draft_idx, k));
                    }
                },
                _ => {}
            }
        }
        let alive_count = alive.iter().filter(|a| **a).count();
        if alive_count > 0 {
            live_entries += 1;
        }
        drafts.push(Draft {
            entry: ActivityEntry {
                commit: commit.sha.clone(),
                when: commit.when,
                actor: commit.author.clone(),
                events,
            },
            alive,
            alive_count,
        });
    }

    let mut entries: Vec<ActivityEntry> = drafts
        .into_iter()
        .filter(|d| d.alive_count > 0)
        .map(|d| {
            let mut entry = d.entry;
            let mut keep = d.alive.into_iter();
            entry.events.retain(|_| keep.next().unwrap_or(true));
            entry
        })
        .collect();

    attach_build_records(context, &mut reader, &mut entries);
    if let Some(b) = query.branch.as_deref() {
        for entry in &mut entries {
            entry.events.retain(|ev| ev.branches().contains(&b));
        }
        entries.retain(|e| !e.events.is_empty());
    }

    let mut skipped = reader.skipped;
    let order: std::collections::HashMap<&str, usize> = history
        .iter()
        .enumerate()
        .map(|(i, c)| (c.sha.as_str(), i))
        .collect();
    skipped.sort_by_key(|s| order.get(s.commit.as_str()).copied());
    Ok(ActivityLog {
        entries,
        skipped,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Approval, Environment, RebuildSnapshot, Rejection};
    use chrono::Utc;

    fn config(envs: &[(&str, &str, &[&str])]) -> HitchConfig {
        let mut c = HitchConfig::new();
        for (name, base, branches) in envs {
            let mut e = Environment::new(base.to_string());
            e.branches = branches.iter().map(|b| b.to_string()).collect();
            c.environments.insert(name.to_string(), e);
        }
        c
    }

    fn request(id: &str, status: ApprovalStatus) -> ApprovalRequest {
        let mut r = ApprovalRequest::new(
            "prod".into(),
            "feat".into(),
            Operation::Promote,
            "a@x.com".into(),
            RebuildSnapshot {
                base_branch: "main".into(),
                base_sha: "0".into(),
                branch_shas: Default::default(),
                merge_conflicts: false,
            },
        );
        r.id = id.into();
        r.status = status;
        r
    }

    fn vote() -> Approval {
        Approval {
            approved_by: "b@x.com".into(),
            approved_at: Utc::now(),
            comment: None,
        }
    }

    fn with_request(mut c: HitchConfig, r: ApprovalRequest) -> HitchConfig {
        c.approval_requests.push(r);
        c
    }

    fn ev(kind: &str, id: &str) -> HitchEvent {
        let (request_id, environment, branch) =
            (id.to_string(), "prod".to_string(), "feat".to_string());
        match kind {
            "granted" => HitchEvent::ApprovalGranted {
                request_id,
                environment,
                branch,
                direction: ApprovalDirection::Promote,
            },
            "rejected" => HitchEvent::ApprovalRejected {
                request_id,
                environment,
                branch,
                direction: ApprovalDirection::Promote,
            },
            "applied" => HitchEvent::ApprovalApplied {
                request_id,
                environment,
                branch,
                direction: ApprovalDirection::Promote,
            },
            "cancelled" => HitchEvent::ApprovalCancelled {
                request_id,
                environment,
                branch,
                direction: ApprovalDirection::Promote,
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_new_environment_is_created_with_its_base() {
        let old = config(&[]);
        let new = config(&[("dev", "main", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::EnvironmentCreated {
                environment: "dev".into(),
                base: "main".into()
            }]
        );
    }

    #[test]
    fn a_removed_environment_is_reported() {
        let old = config(&[("dev", "main", &[])]);
        let new = config(&[]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::EnvironmentRemoved {
                environment: "dev".into()
            }]
        );
    }

    #[test]
    fn a_base_change_names_both_bases() {
        let old = config(&[("dev", "main", &[])]);
        let new = config(&[("dev", "develop", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::BaseChanged {
                environment: "dev".into(),
                from: "main".into(),
                to: "develop".into()
            }]
        );
    }

    #[test]
    fn promotions_come_out_in_declaration_order_and_demotions_in_old_order() {
        let old = config(&[("dev", "main", &["a", "b", "c"])]);
        let new = config(&[("dev", "main", &["c", "z", "y"])]);
        let p = |b: &str| HitchEvent::Promoted {
            environment: "dev".into(),
            branch: b.into(),
        };
        let d = |b: &str| HitchEvent::Demoted {
            environment: "dev".into(),
            branch: b.into(),
        };
        assert_eq!(
            derive_events(&old, &new),
            vec![p("z"), p("y"), d("a"), d("b")]
        );
    }

    #[test]
    fn environments_are_diffed_in_name_order_not_map_order() {
        let names: Vec<String> = (0..20).map(|i| format!("env{:02}", i)).collect();
        let build = |branch: Option<&str>| {
            let mut c = HitchConfig::new();
            for n in &names {
                let mut e = Environment::new("main".into());
                if let Some(b) = branch {
                    e.branches.push(b.into());
                }
                c.environments.insert(n.clone(), e);
            }
            c
        };
        let (old, new) = (build(None), build(Some("f")));
        let first = derive_events(&old, &new);
        let got: Vec<&str> = first.iter().map(|e| e.environment()).collect();
        let want: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(got, want);
        for _ in 0..50 {
            assert_eq!(derive_events(&old, &new), first);
        }
    }

    #[test]
    fn locking_carries_the_holder_when_there_is_one() {
        let old = config(&[("dev", "main", &[])]);
        let mut new = old.clone();
        let e = new.environments.get_mut("dev").unwrap();
        e.locked = true;
        e.locked_by = Some("a@x.com".into());
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Locked {
                environment: "dev".into(),
                by: Some("a@x.com".into())
            }]
        );
        new.environments.get_mut("dev").unwrap().locked_by = None;
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Locked {
                environment: "dev".into(),
                by: None
            }]
        );
    }

    #[test]
    fn unlocking_is_reported() {
        let mut old = config(&[("dev", "main", &[])]);
        old.environments.get_mut("dev").unwrap().locked = true;
        let new = config(&[("dev", "main", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Unlocked {
                environment: "dev".into()
            }]
        );
    }

    #[test]
    fn a_rebuild_timestamp_is_an_unrecorded_rebuild_and_a_release_timestamp_a_release() {
        let old = config(&[("dev", "main", &[])]);
        let mut new = old.clone();
        let e = new.environments.get_mut("dev").unwrap();
        e.rebuilt_at = Some(Utc::now());
        e.released_at = Some(Utc::now());
        assert_eq!(
            derive_events(&old, &new),
            vec![
                HitchEvent::Rebuilt {
                    environment: "dev".into(),
                    outcome: RebuildOutcome::Unrecorded
                },
                HitchEvent::Released {
                    environment: "dev".into()
                },
            ]
        );
    }

    #[test]
    fn a_filter_on_dev_does_not_match_a_branch_called_feature_devtools() {
        let e = HitchEvent::Promoted {
            environment: "qa".into(),
            branch: "feature/devtools".into(),
        };
        assert_ne!(e.environment(), "dev");
        assert!(!e.branches().contains(&"dev"));
    }

    #[test]
    fn a_new_request_is_requested_with_its_direction() {
        let old = config(&[("prod", "main", &[])]);
        let new = with_request(old.clone(), request("r1", ApprovalStatus::Pending));
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::ApprovalRequested {
                request_id: "r1".into(),
                environment: "prod".into(),
                branch: "feat".into(),
                direction: ApprovalDirection::Promote,
            }]
        );
    }

    #[test]
    fn a_new_vote_carries_the_count_and_the_new_configs_threshold() {
        let mut old = config(&[("prod", "main", &[])]);
        old.environments.get_mut("prod").unwrap().min_approvals = 5;
        let old = with_request(old, request("r1", ApprovalStatus::Pending));
        let mut new = old.clone();
        new.environments.get_mut("prod").unwrap().min_approvals = 3;
        new.approval_requests[0].approvals.push(vote());
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::ApprovalVoted {
                request_id: "r1".into(),
                environment: "prod".into(),
                branch: "feat".into(),
                approvals: 1,
                required: 3,
                direction: ApprovalDirection::Promote,
            }]
        );
    }

    #[test]
    fn approval_transitions_carry_the_requests_direction() {
        let mut old_req = request("r1", ApprovalStatus::Pending);
        old_req.operation = Operation::Demote;
        let old = with_request(config(&[("prod", "main", &[])]), old_req);
        let mut new = old.clone();
        new.approval_requests[0].status = ApprovalStatus::Approved;
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::ApprovalGranted {
                request_id: "r1".into(),
                environment: "prod".into(),
                branch: "feat".into(),
                direction: ApprovalDirection::Demote,
            }]
        );
    }

    #[test]
    fn a_vote_for_a_removed_environment_falls_back_to_the_vote_count() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let mut new = with_request(config(&[]), request("r1", ApprovalStatus::Pending));
        new.approval_requests[0].approvals.push(vote());
        let events = derive_events(&old, &new);
        assert!(events.contains(&HitchEvent::ApprovalVoted {
            request_id: "r1".into(),
            environment: "prod".into(),
            branch: "feat".into(),
            approvals: 1,
            required: 1,
            direction: ApprovalDirection::Promote,
        }));
    }

    #[test]
    fn each_status_transition_is_its_own_event() {
        for (status, kind) in [
            (ApprovalStatus::Approved, "granted"),
            (ApprovalStatus::Applied, "applied"),
            (ApprovalStatus::Cancelled, "cancelled"),
        ] {
            let old = with_request(
                config(&[("prod", "main", &[])]),
                request("r1", ApprovalStatus::Pending),
            );
            let mut new = old.clone();
            new.approval_requests[0].status = status;
            assert_eq!(derive_events(&old, &new), vec![ev(kind, "r1")]);
        }
    }

    #[test]
    fn one_rejection_is_one_event_however_it_is_recorded() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let rejection = Rejection {
            rejected_by: "b@x.com".into(),
            rejected_at: Utc::now(),
            reason: "no".into(),
        };

        let mut both = old.clone();
        both.approval_requests[0].status = ApprovalStatus::Rejected;
        both.approval_requests[0].rejection = Some(rejection.clone());
        assert_eq!(derive_events(&old, &both), vec![ev("rejected", "r1")]);

        let mut status_only = old.clone();
        status_only.approval_requests[0].status = ApprovalStatus::Rejected;
        assert_eq!(
            derive_events(&old, &status_only),
            vec![ev("rejected", "r1")]
        );

        let mut field_only = old.clone();
        field_only.approval_requests[0].rejection = Some(rejection);
        assert_eq!(derive_events(&old, &field_only), vec![ev("rejected", "r1")]);
    }

    #[test]
    fn a_vote_that_reaches_the_threshold_is_voted_then_granted() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let mut new = old.clone();
        new.approval_requests[0].approvals.push(vote());
        new.approval_requests[0].status = ApprovalStatus::Approved;
        let events = derive_events(&old, &new);
        assert!(matches!(events[0], HitchEvent::ApprovalVoted { .. }));
        assert_eq!(events[1], ev("granted", "r1"));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn approval_requests_are_diffed_in_id_order() {
        let base = config(&[("prod", "main", &[])]);
        let mut new = base.clone();
        new.approval_requests
            .push(request("b", ApprovalStatus::Pending));
        new.approval_requests
            .push(request("a", ApprovalStatus::Pending));
        let ids: Vec<String> = derive_events(&base, &new)
            .into_iter()
            .map(|e| match e {
                HitchEvent::ApprovalRequested { request_id, .. } => request_id,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn events_serialize_with_snake_case_tags_and_direction() {
        let e = HitchEvent::ApprovalRequested {
            request_id: "r".into(),
            environment: "prod".into(),
            branch: "f".into(),
            direction: Operation::Demote.into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "approval_requested");
        assert_eq!(v["direction"], "demote");
        let r = serde_json::to_value(RebuildOutcome::Unrecorded).unwrap();
        assert_eq!(r["status"], "unrecorded");
    }
}
