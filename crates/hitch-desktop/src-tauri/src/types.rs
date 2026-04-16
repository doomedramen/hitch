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
