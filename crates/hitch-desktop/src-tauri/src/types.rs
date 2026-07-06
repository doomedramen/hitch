use hitch::commands::global_context::GlobalContext;
use hitch::types::{ApprovalRequest, ApprovalStatus, HitchConfig};
use hitch::utils::output::{BufferedLine, OutputLevel};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "ok")]
pub enum RepoProbeResultDto {
    #[serde(rename = "true")]
    Ok {
        path: String,
        display_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin_url_normalized: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        root_commit_oid: Option<String>,
    },
    #[serde(rename = "false")]
    Err { path: String, error: String },
}

impl RepoProbeResultDto {
    pub fn ok(
        path: String,
        display_name: String,
        origin_url_normalized: Option<String>,
        root_commit_oid: Option<String>,
    ) -> Self {
        Self::Ok {
            path,
            display_name,
            origin_url_normalized,
            root_commit_oid,
        }
    }

    pub fn err(path: String, error: String) -> Self {
        Self::Err { path, error }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceIndexDto {
    pub current_branch: Option<String>,
    pub environments: Vec<EnvironmentMinDto>,
    pub promoted_branches: Vec<BranchRowDto>,
    pub branches: Vec<BranchRowDto>,
}

impl From<hitch::core::workspace_index::WorkspaceIndexModel> for WorkspaceIndexDto {
    fn from(value: hitch::core::workspace_index::WorkspaceIndexModel) -> Self {
        Self {
            current_branch: value.current_branch,
            environments: value.environments.into_iter().map(Into::into).collect(),
            promoted_branches: value
                .promoted_branches
                .into_iter()
                .map(Into::into)
                .collect(),
            branches: value.branches.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EnvironmentMinDto {
    pub name: String,
    pub base: String,
    pub promoted_count: usize,
    pub locked: bool,
    pub requires_approval: bool,
    pub min_approvals: usize,
    pub approvers_count: usize,
}

impl From<hitch::core::workspace_index::EnvironmentMinModel> for EnvironmentMinDto {
    fn from(value: hitch::core::workspace_index::EnvironmentMinModel) -> Self {
        Self {
            name: value.name,
            base: value.base,
            promoted_count: value.promoted_count,
            locked: value.locked,
            requires_approval: value.requires_approval,
            min_approvals: value.min_approvals,
            approvers_count: value.approvers_count,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BranchRowDto {
    pub name: String,
    pub local: bool,
    pub remote: bool,
    pub is_environment: bool,
    pub promoted_to: Vec<String>,
    pub base_for: Vec<String>,
}

impl From<hitch::core::workspace::BranchRow> for BranchRowDto {
    fn from(value: hitch::core::workspace::BranchRow) -> Self {
        Self {
            name: value.name,
            local: value.local,
            remote: value.remote,
            is_environment: value.is_environment,
            promoted_to: value.promoted_to,
            base_for: value.base_for,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TimelineItemDto {
    pub when: String,
    pub kind: TimelineKindDto,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<hitch::core::timeline::TimelineItem> for TimelineItemDto {
    fn from(value: hitch::core::timeline::TimelineItem) -> Self {
        Self {
            when: value.when.to_rfc3339(),
            kind: match value.kind {
                hitch::core::timeline::TimelineKind::GitCommit => TimelineKindDto::GitCommit,
                hitch::core::timeline::TimelineKind::HitchEvent => TimelineKindDto::HitchEvent,
            },
            summary: value.summary,
            detail: value.detail,
        }
    }
}

#[derive(Debug, Serialize)]
pub enum TimelineKindDto {
    GitCommit,
    HitchEvent,
}

#[derive(Debug, Serialize)]
pub struct BranchDetailsDto {
    pub branch: BranchRowDto,
    pub branch_sha: String,
    pub metadata_sha: Option<String>,
    pub overview: String,
    pub timeline: Vec<TimelineItemDto>,
}

impl From<hitch::core::details::BranchDetailsModel> for BranchDetailsDto {
    fn from(value: hitch::core::details::BranchDetailsModel) -> Self {
        Self {
            branch: value.branch.into(),
            branch_sha: value.branch_sha,
            metadata_sha: value.metadata_sha,
            overview: value.overview,
            timeline: value.timeline.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EnvironmentDetailsDto {
    pub name: String,
    pub env_sha: String,
    pub metadata_sha: Option<String>,
    pub overview: String,
    pub timeline: Vec<TimelineItemDto>,
}

impl From<hitch::core::details::EnvironmentDetailsModel> for EnvironmentDetailsDto {
    fn from(value: hitch::core::details::EnvironmentDetailsModel) -> Self {
        Self {
            name: value.name,
            env_sha: value.env_sha,
            metadata_sha: value.metadata_sha,
            overview: value.overview,
            timeline: value.timeline.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BufferedLineDto {
    pub level: OutputLevelDto,
    pub message: String,
}

impl From<BufferedLine> for BufferedLineDto {
    fn from(value: BufferedLine) -> Self {
        Self {
            level: value.level.into(),
            message: value.message,
        }
    }
}

#[derive(Debug, Serialize)]
pub enum OutputLevelDto {
    Info,
    Success,
    Warning,
    Error,
}

impl From<OutputLevel> for OutputLevelDto {
    fn from(value: OutputLevel) -> Self {
        match value {
            OutputLevel::Info => OutputLevelDto::Info,
            OutputLevel::Success => OutputLevelDto::Success,
            OutputLevel::Warning => OutputLevelDto::Warning,
            OutputLevel::Error => OutputLevelDto::Error,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OperationResultDto {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub lines: Vec<BufferedLineDto>,
}

impl OperationResultDto {
    pub fn error(error: String, lines: Vec<BufferedLine>) -> Self {
        Self {
            ok: false,
            error: Some(error),
            lines: lines.into_iter().map(Into::into).collect(),
        }
    }

    pub fn from_result(res: Result<(), String>, lines: Vec<BufferedLine>) -> Self {
        match res {
            Ok(()) => Self {
                ok: true,
                error: None,
                lines: lines.into_iter().map(Into::into).collect(),
            },
            Err(e) => Self {
                ok: false,
                error: Some(e),
                lines: lines.into_iter().map(Into::into).collect(),
            },
        }
    }
}

// =============================================================================
// Approvals
// =============================================================================

#[derive(Debug, Serialize)]
pub struct ApprovalDto {
    pub approved_by: String,
    pub approved_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RejectionDto {
    pub rejected_by: String,
    pub rejected_at: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ApprovalSnapshotDto {
    pub base_branch: String,
    pub base_sha: String,
    /// Branch name -> commit SHA, sorted by branch name for a stable UI order.
    pub branch_shas: Vec<(String, String)>,
    pub merge_conflicts: bool,
}

#[derive(Debug, Serialize)]
pub struct ApprovalRequestDto {
    pub id: String,
    pub environment: String,
    pub branch: String,
    pub operation: String,
    pub requested_by: String,
    pub requested_at: String,
    pub status: String,
    pub approvals: Vec<ApprovalDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection: Option<RejectionDto>,
    pub snapshot: ApprovalSnapshotDto,
    /// The environment's authorized approvers (empty if the environment is gone).
    pub approvers: Vec<String>,
    pub approval_count: usize,
    /// Threshold governing this request (frozen at creation when present).
    pub min_approvals: usize,
    /// Branches whose SHA has moved since the snapshot was taken (only computed
    /// for requests that can still be acted on).
    pub drifted_branches: Vec<String>,
    // Best-effort action hints for the current git user. The backend re-validates
    // every action under the repository lock, so these only drive button state.
    pub viewer_is_requester: bool,
    pub viewer_has_approved: bool,
    pub viewer_can_approve: bool,
    pub viewer_can_reject: bool,
    pub viewer_can_cancel: bool,
    pub viewer_can_refresh: bool,
    /// An Approved request whose operation never applied (e.g. interrupted apply)
    /// can be re-driven to completion. The CLI's `approve` handles this path.
    pub viewer_can_execute: bool,
}

impl ApprovalRequestDto {
    pub fn build(
        context: &GlobalContext,
        config: &HitchConfig,
        req: &ApprovalRequest,
        current_user: Option<&str>,
    ) -> Self {
        let environment = config.get_environment(&req.environment);

        // Prefer the request's frozen threshold; fall back to the environment's
        // current value, then to 1 if the environment has since been removed.
        let min_approvals = environment
            .map(|env| req.required_approvals(env))
            .unwrap_or_else(|| req.snapshot_min_approvals.unwrap_or(1));

        let is_pending = req.status == ApprovalStatus::Pending;
        // Drift only matters while a request can still be approved/executed.
        let can_drift = matches!(
            req.status,
            ApprovalStatus::Pending | ApprovalStatus::Approved
        );
        let drifted_branches = if can_drift {
            hitch::utils::snapshot::drifted_branches(context, &req.rebuild_snapshot)
        } else {
            Vec::new()
        };

        let is_approver = current_user
            .map(|u| environment.map(|env| env.is_approver(u)).unwrap_or(false))
            .unwrap_or(false);
        let viewer_is_requester = current_user.map(|u| u == req.requested_by).unwrap_or(false);
        let viewer_has_approved = current_user.map(|u| req.has_approved(u)).unwrap_or(false);

        // Mirror the CLI's authorization predicates (see utils::authorization):
        //   approve  -> approver, not requester, not already approved
        //   reject   -> approver
        //   cancel   -> requester only
        //   refresh  -> pending and drifted
        let viewer_can_approve =
            is_pending && is_approver && !viewer_is_requester && !viewer_has_approved;
        let viewer_can_reject = is_pending && is_approver;
        let viewer_can_cancel = is_pending && viewer_is_requester;
        let viewer_can_refresh = is_pending && !drifted_branches.is_empty();
        let viewer_can_execute =
            req.status == ApprovalStatus::Approved && (is_approver || viewer_is_requester);

        let mut branch_shas: Vec<(String, String)> = req
            .rebuild_snapshot
            .branch_shas
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        branch_shas.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            id: req.id.clone(),
            environment: req.environment.clone(),
            branch: req.branch.clone(),
            operation: req.operation.to_string(),
            requested_by: req.requested_by.clone(),
            requested_at: req.requested_at.to_rfc3339(),
            status: req.status.to_string(),
            approvals: req
                .approvals
                .iter()
                .map(|a| ApprovalDto {
                    approved_by: a.approved_by.clone(),
                    approved_at: a.approved_at.to_rfc3339(),
                    comment: a.comment.clone(),
                })
                .collect(),
            rejection: req.rejection.as_ref().map(|r| RejectionDto {
                rejected_by: r.rejected_by.clone(),
                rejected_at: r.rejected_at.to_rfc3339(),
                reason: r.reason.clone(),
            }),
            snapshot: ApprovalSnapshotDto {
                base_branch: req.rebuild_snapshot.base_branch.clone(),
                base_sha: req.rebuild_snapshot.base_sha.clone(),
                branch_shas,
                merge_conflicts: req.rebuild_snapshot.merge_conflicts,
            },
            approvers: environment
                .map(|env| env.approvers.clone())
                .unwrap_or_default(),
            approval_count: req.approval_count(),
            min_approvals,
            drifted_branches,
            viewer_is_requester,
            viewer_has_approved,
            viewer_can_approve,
            viewer_can_reject,
            viewer_can_cancel,
            viewer_can_refresh,
            viewer_can_execute,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ApprovalsListDto {
    /// The current git user's email, if configured. Drives the viewer_* hints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_user: Option<String>,
    pub requests: Vec<ApprovalRequestDto>,
}
