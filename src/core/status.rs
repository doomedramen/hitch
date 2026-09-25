use crate::core::state::{EnvironmentHealth, EnvironmentState, RepositoryStateSnapshot};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct StatusSummary {
    pub total_envs: usize,
    pub locked_envs: usize,
    pub needs_rebuild_envs: usize,
    pub never_rebuilt_envs: usize,
}

#[derive(Debug, Clone)]
pub struct EnvironmentStatusModel {
    pub name: String,
    pub base: String,
    pub branches: Vec<String>,
    pub locked: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub rebuilt_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub requires_approval: bool,
    pub min_approvals: usize,
    pub approvers: Vec<String>,
    /// The environment's single verdict, carried whole so a renderer cannot
    /// re-derive one. This replaces a private `RebuildState` enum that
    /// answered the same question a fourth time, by timestamp.
    pub state: EnvironmentState,
}

#[derive(Debug, Clone)]
pub struct StatusModel {
    pub current_branch: Option<String>,
    pub summary: StatusSummary,
    pub environments: Vec<EnvironmentStatusModel>,
}

/// Project a snapshot into the status view.
///
/// Takes the snapshot rather than building one, and is infallible by
/// construction: a view that re-read the repository could disagree with the
/// snapshot it is supposed to be describing, and nothing in the caller would
/// notice.
pub fn build_status_model(snapshot: &RepositoryStateSnapshot) -> StatusModel {
    let environments: Vec<EnvironmentStatusModel> = snapshot
        .environments
        .iter()
        .map(|state| EnvironmentStatusModel {
            name: state.name.clone(),
            base: state.base.clone(),
            branches: state
                .desired
                .branches
                .iter()
                .map(|b| b.name.clone())
                .collect(),
            locked: state.locked,
            locked_by: state.locked_by.clone(),
            locked_at: state.locked_at,
            rebuilt_at: state.rebuilt_at,
            released_at: state.released_at,
            requires_approval: state.approval_policy.required,
            min_approvals: state.approval_policy.min_approvals,
            approvers: state.approval_policy.approvers.clone(),
            state: state.clone(),
        })
        .collect();

    let total_envs = environments.len();
    let locked_envs = environments.iter().filter(|e| e.locked).count();
    // `is_actionable` rather than a local `matches!`: the question "is there
    // pending work here" is answered by the health enum, and re-deciding it
    // here is exactly how a second verdict gets invented.
    let needs_rebuild_envs = environments
        .iter()
        .filter(|e| e.state.health.is_actionable())
        .count();
    // Never-built is a *subset* of actionable, not an alternative to it — an
    // environment that has never been built certainly has pending work — so it
    // is counted separately rather than subtracted out.
    let never_rebuilt_envs = environments
        .iter()
        .filter(|e| matches!(e.state.health, EnvironmentHealth::NeverBuilt))
        .count();

    StatusModel {
        current_branch: snapshot.current_branch.clone(),
        summary: StatusSummary {
            total_envs,
            locked_envs,
            needs_rebuild_envs,
            never_rebuilt_envs,
        },
        environments,
    }
}
