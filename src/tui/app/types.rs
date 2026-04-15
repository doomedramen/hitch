use crate::commands::global_context::GlobalContext;
use crate::core::details::{BranchDetailsModel, EnvironmentDetailsModel};
use crate::core::status::StatusSummary;
use crate::core::workspace::BranchRow;
use crate::core::workspace_index::WorkspaceIndexModel;
use crate::utils::output::BufferedOutputSink;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub(super) const FILTER_DEBOUNCE: Duration = Duration::from_millis(250);
pub(super) const SELECTION_DEBOUNCE: Duration = Duration::from_millis(200);
pub(super) const SELECTED_POLL_INTERVAL: Duration = Duration::from_secs(10);
pub(super) const INDICATOR_SHOW_DELAY: Duration = Duration::from_millis(300);
pub(super) const SPINNER_STEP: Duration = Duration::from_millis(140);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityKind {
    WorkspaceIndex,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Filter,
    List,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Overview,
    Timeline,
}

#[derive(Debug, Clone)]
pub(super) enum Selection {
    Environment { name: String },
    Branch { row: BranchRow },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum SelectionKey {
    Environment(String),
    Branch(String),
}

#[derive(Debug)]
pub(super) struct DetailsLoadState {
    pub(super) key: SelectionKey,
    pub(super) token: u64,
    pub(super) pct: u16,
    pub(super) msg: String,
}

#[derive(Debug)]
pub(super) enum WorkerMsg {
    WorkspaceIndexProgress {
        token: u64,
        pct: u16,
        msg: String,
    },
    WorkspaceIndexReady {
        token: u64,
        model: WorkspaceIndexModel,
    },
    WorkspaceIndexError {
        token: u64,
        error: String,
    },

    StatusSummaryReady {
        token: u64,
        summary: StatusSummary,
    },

    BranchDetailsProgress {
        token: u64,
        branch: String,
        pct: u16,
        msg: String,
    },
    BranchDetailsReady {
        token: u64,
        model: Box<BranchDetailsModel>,
    },
    BranchDetailsNoop {
        token: u64,
        branch: String,
    },
    BranchDetailsError {
        token: u64,
        branch: String,
        error: String,
    },

    EnvDetailsProgress {
        token: u64,
        env: String,
        pct: u16,
        msg: String,
    },
    EnvDetailsReady {
        token: u64,
        model: Box<EnvironmentDetailsModel>,
    },
    EnvDetailsNoop {
        token: u64,
        env: String,
    },
    EnvDetailsError {
        token: u64,
        env: String,
        error: String,
    },

    Done {
        ok: bool,
        error: Option<String>,
    },
}

#[derive(Debug)]
pub(super) enum Modal {
    PromotePicker {
        branch: BranchRow,
        env_index: usize,
        envs: Vec<String>,
    },
    ConfirmRebuild {
        env_name: String,
    },
    ConfirmRelease {
        env_name: String,
        target_branch: String,
    },
    Operation {
        title: String,
        sink: Arc<BufferedOutputSink>,
        done: bool,
        ok: bool,
        error: Option<String>,
    },
    Help,
}

#[derive(Debug, Clone)]
pub(super) struct ListEntry {
    pub(super) selectable: bool,
    pub(super) label: String,
    pub(super) selection: Option<Selection>,
    pub(super) promoted_section: bool,
}

#[derive(Debug, Clone)]
pub(super) struct CachedBranchDetails {
    pub(super) model: BranchDetailsModel,
    pub(super) last_validated_at: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct CachedEnvDetails {
    pub(super) model: EnvironmentDetailsModel,
    pub(super) last_validated_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Promote,
    Rebuild,
    Release,
}

pub(super) struct App {
    pub(super) context: GlobalContext,
    pub(super) focus: Focus,
    pub(super) tab: Tab,

    pub(super) filter: String,
    pub(super) filter_applied: String,
    pub(super) pending_filter_apply_at: Option<Instant>,

    pub(super) index: Option<WorkspaceIndexModel>,
    pub(super) index_loading: bool,
    pub(super) index_load_token: u64,
    pub(super) index_progress_pct: u16,
    pub(super) index_progress_msg: String,

    pub(super) status_summary: Option<StatusSummary>,
    pub(super) status_summary_token: u64,

    pub(super) list_entries: Vec<ListEntry>,
    pub(super) list_state: ListState,

    pub(super) status_line: String,

    pub(super) details_loading: Option<DetailsLoadState>,
    pub(super) next_token: u64,
    pub(super) pending_selection_load_at: Option<Instant>,
    pub(super) last_polled_at: Instant,

    pub(super) branch_cache: HashMap<String, CachedBranchDetails>,
    pub(super) env_cache: HashMap<String, CachedEnvDetails>,

    pub(super) hide_hitch_branches: bool,

    pub(super) modal: Option<Modal>,
    pub(super) worker_tx: mpsc::Sender<WorkerMsg>,

    // Hit-test rects
    pub(super) filter_rect: Rect,
    pub(super) list_rect: Rect,
    pub(super) details_rect: Rect,
    pub(super) footer_rect: Rect,

    // Sidebar rendering state
    pub(super) sidebar_name_width: u16,
    pub(super) marquee_active_key: Option<SelectionKey>,
    pub(super) marquee_offset: usize,
    pub(super) marquee_last_advance: Instant,

    // Activity indicator state
    pub(super) activity_started_at: Option<Instant>,
    pub(super) activity_kind: Option<ActivityKind>,
    pub(super) activity_msg: String,
    pub(super) spinner_frame: usize,
    pub(super) spinner_last_advance: Instant,

    // Details pane state
    pub(super) timeline_scroll: u16,
}
