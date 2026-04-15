use super::types::{
    Action, ActivityKind, App, CachedBranchDetails, CachedEnvDetails, DetailsLoadState, Modal,
    Selection, SelectionKey, WorkerMsg,
};
use crate::commands::global_context::GlobalContext;
use crate::core::details::{build_branch_details_model, build_environment_details_model};
use crate::core::status::build_status_model;
use crate::core::workspace::BranchRow;
use crate::core::workspace_index::build_workspace_index_model;
use crate::utils::confirm::AlwaysYesConfirm;
use crate::utils::logging::Logger;
use crate::utils::output::{BufferedOutputSink, OutputLevel, OutputSink};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

impl App {
    pub(super) fn on_worker_msg(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::WorkspaceIndexProgress { token, pct, msg } => {
                if token != self.index_load_token {
                    return;
                }
                self.index_progress_pct = pct;
                self.index_progress_msg = msg;
                if self.activity_kind == Some(ActivityKind::WorkspaceIndex) {
                    self.activity_msg = self.index_progress_msg.clone();
                }
            }
            WorkerMsg::WorkspaceIndexReady { token, model } => {
                if token != self.index_load_token {
                    return;
                }
                self.index_loading = false;
                self.index_progress_pct = 100;
                self.index_progress_msg = "Workspace index ready".to_string();
                self.index = Some(model);
                self.rebuild_list();
                self.list_state
                    .select(Some(self.first_selectable_index().unwrap_or(0)));
                self.marquee_active_key = self.current_selection_key();
                self.marquee_offset = 0;
                self.marquee_last_advance = Instant::now();
                self.schedule_selected_details_load();
                self.start_load_status_summary();
                self.clear_activity();
            }
            WorkerMsg::WorkspaceIndexError { token, error } => {
                if token != self.index_load_token {
                    return;
                }
                self.index_loading = false;
                self.status_line = format!("Error: {}", error);
                self.index_progress_msg = "Workspace index failed".to_string();
                self.clear_activity();
            }
            WorkerMsg::StatusSummaryReady { token, summary } => {
                if token != self.status_summary_token {
                    return;
                }
                self.status_summary = Some(summary);
            }
            WorkerMsg::BranchDetailsProgress {
                token,
                branch,
                pct,
                msg,
            } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                if let Some(DetailsLoadState {
                    key: SelectionKey::Branch(b),
                    pct: pct2,
                    msg: msg2,
                    ..
                }) = &mut self.details_loading
                {
                    if *b == branch {
                        *pct2 = pct;
                        *msg2 = msg.clone();
                    }
                }
                if self.activity_kind == Some(ActivityKind::Details) {
                    self.activity_msg = msg;
                }
            }
            WorkerMsg::BranchDetailsNoop { token, branch } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                if let Some(cached) = self.branch_cache.get_mut(&branch) {
                    cached.last_validated_at = Instant::now();
                }
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::BranchDetailsReady { token, model } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                self.branch_cache.insert(
                    model.branch.name.clone(),
                    CachedBranchDetails {
                        model: *model,
                        last_validated_at: Instant::now(),
                    },
                );
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::BranchDetailsError {
                token,
                branch,
                error,
            } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                self.status_line = format!("Error loading {}: {}", branch, error);
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::EnvDetailsProgress {
                token,
                env,
                pct,
                msg,
            } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                if let Some(DetailsLoadState {
                    key: SelectionKey::Environment(e),
                    pct: pct2,
                    msg: msg2,
                    ..
                }) = &mut self.details_loading
                {
                    if *e == env {
                        *pct2 = pct;
                        *msg2 = msg.clone();
                    }
                }
                if self.activity_kind == Some(ActivityKind::Details) {
                    self.activity_msg = msg;
                }
            }
            WorkerMsg::EnvDetailsNoop { token, env } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                if let Some(cached) = self.env_cache.get_mut(&env) {
                    cached.last_validated_at = Instant::now();
                }
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::EnvDetailsReady { token, model } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                self.env_cache.insert(
                    model.name.clone(),
                    CachedEnvDetails {
                        model: *model,
                        last_validated_at: Instant::now(),
                    },
                );
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::EnvDetailsError { token, env, error } => {
                if !self.matches_details_loading_token(token) {
                    return;
                }
                self.status_line = format!("Error loading env/{}: {}", env, error);
                self.details_loading = None;
                self.clear_activity();
            }
            WorkerMsg::Done { ok, error } => {
                if let Some(Modal::Operation {
                    done,
                    ok: ok2,
                    error: err2,
                    ..
                }) = &mut self.modal
                {
                    *done = true;
                    *ok2 = ok;
                    *err2 = error;
                    self.status_line = if ok {
                        "Done".to_string()
                    } else {
                        "Failed".to_string()
                    };
                }

                self.reload_after_operation();
            }
        }
    }

    pub(super) fn start_load_workspace_index(&mut self) {
        self.start_activity(ActivityKind::WorkspaceIndex, "Loading workspace index…");
        self.index_loading = true;
        self.index_progress_pct = 0;
        self.index_progress_msg = "Loading workspace index…".to_string();
        self.index_load_token = self.bump_token();
        let token = self.index_load_token;
        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;

        thread::spawn(move || {
            let _ = tx.send(WorkerMsg::WorkspaceIndexProgress {
                token,
                pct: 5,
                msg: "Reading hitch config…".to_string(),
            });

            let logger = Arc::new(Logger::for_command("tui-index", verbose));
            let context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::WorkspaceIndexError {
                        token,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            let _ = tx.send(WorkerMsg::WorkspaceIndexProgress {
                token,
                pct: 35,
                msg: "Listing branches…".to_string(),
            });

            match build_workspace_index_model(&context) {
                Ok(model) => {
                    let _ = tx.send(WorkerMsg::WorkspaceIndexProgress {
                        token,
                        pct: 100,
                        msg: "Workspace index ready".to_string(),
                    });
                    let _ = tx.send(WorkerMsg::WorkspaceIndexReady { token, model });
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::WorkspaceIndexError {
                        token,
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    pub(super) fn start_load_status_summary(&mut self) {
        self.status_summary_token = self.bump_token();
        let token = self.status_summary_token;
        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;

        thread::spawn(move || {
            let logger = Arc::new(Logger::for_command("tui-summary", verbose));
            let context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Ok(status) = build_status_model(&context) {
                let _ = tx.send(WorkerMsg::StatusSummaryReady {
                    token,
                    summary: status.summary,
                });
            }
        });
    }

    pub(super) fn start_load_selected_details(&mut self) {
        let Some(sel) = self.selected_selection() else {
            return;
        };
        if self.index_loading || self.index.is_none() {
            return;
        }
        if self.modal.is_some() {
            return;
        }

        match sel {
            Selection::Branch { row } => self.start_load_branch_details(row),
            Selection::Environment { name } => self.start_load_env_details(name),
        }
    }

    fn start_load_branch_details(&mut self, row: BranchRow) {
        self.start_activity(ActivityKind::Details, "Loading branch details…");
        let token = self.bump_token();
        self.details_loading = Some(DetailsLoadState {
            key: SelectionKey::Branch(row.name.clone()),
            token,
            pct: 0,
            msg: "Loading branch details…".to_string(),
        });

        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;
        let branch = row.name.clone();
        let cached = self.branch_cache.get(&branch).cloned();

        thread::spawn(move || {
            let _ = tx.send(WorkerMsg::BranchDetailsProgress {
                token,
                branch: branch.clone(),
                pct: 10,
                msg: "Checking SHAs…".to_string(),
            });

            let logger = Arc::new(Logger::for_command("tui-branch-details", verbose));
            let context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::BranchDetailsError {
                        token,
                        branch,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            let metadata_sha = context.git().get_branch_commit_sha("hitch-metadata").ok();
            let branch_sha = match context.git().get_branch_commit_sha(&row.name) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::BranchDetailsError {
                        token,
                        branch,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            if let Some(c) = cached {
                if c.model.branch_sha == branch_sha && c.model.metadata_sha == metadata_sha {
                    let _ = tx.send(WorkerMsg::BranchDetailsNoop { token, branch });
                    return;
                }
            }

            let _ = tx.send(WorkerMsg::BranchDetailsProgress {
                token,
                branch: branch.clone(),
                pct: 45,
                msg: "Building overview…".to_string(),
            });

            match build_branch_details_model(&context, &row, branch_sha, metadata_sha) {
                Ok(model) => {
                    let _ = tx.send(WorkerMsg::BranchDetailsProgress {
                        token,
                        branch: branch.clone(),
                        pct: 100,
                        msg: "Ready".to_string(),
                    });
                    let _ = tx.send(WorkerMsg::BranchDetailsReady {
                        token,
                        model: Box::new(model),
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::BranchDetailsError {
                        token,
                        branch,
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn start_load_env_details(&mut self, env_name: String) {
        self.start_activity(ActivityKind::Details, "Loading environment details…");
        let token = self.bump_token();
        self.details_loading = Some(DetailsLoadState {
            key: SelectionKey::Environment(env_name.clone()),
            token,
            pct: 0,
            msg: "Loading environment details…".to_string(),
        });

        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;
        let cached = self.env_cache.get(&env_name).cloned();

        thread::spawn(move || {
            let _ = tx.send(WorkerMsg::EnvDetailsProgress {
                token,
                env: env_name.clone(),
                pct: 10,
                msg: "Checking SHAs…".to_string(),
            });

            let logger = Arc::new(Logger::for_command("tui-env-details", verbose));
            let context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::EnvDetailsError {
                        token,
                        env: env_name,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            let metadata_sha = context.git().get_branch_commit_sha("hitch-metadata").ok();
            let env_sha = match context.git().get_branch_commit_sha(&env_name) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::EnvDetailsError {
                        token,
                        env: env_name,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            if let Some(c) = cached {
                if c.model.env_sha == env_sha && c.model.metadata_sha == metadata_sha {
                    let _ = tx.send(WorkerMsg::EnvDetailsNoop {
                        token,
                        env: env_name,
                    });
                    return;
                }
            }

            let _ = tx.send(WorkerMsg::EnvDetailsProgress {
                token,
                env: env_name.clone(),
                pct: 55,
                msg: "Building overview…".to_string(),
            });

            match build_environment_details_model(&context, &env_name, env_sha, metadata_sha) {
                Ok(model) => {
                    let _ = tx.send(WorkerMsg::EnvDetailsProgress {
                        token,
                        env: env_name.clone(),
                        pct: 100,
                        msg: "Ready".to_string(),
                    });
                    let _ = tx.send(WorkerMsg::EnvDetailsReady {
                        token,
                        model: Box::new(model),
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerMsg::EnvDetailsError {
                        token,
                        env: env_name,
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    pub(super) fn start_promote(&mut self, branch: BranchRow, env: String) {
        let sink = BufferedOutputSink::new();
        sink.log(
            OutputLevel::Info,
            &format!("Promoting '{}' -> {}", branch.cli_ref(), env),
        );

        let title = format!("Promote {} → {}", branch.cli_ref(), env);
        self.modal = Some(Modal::Operation {
            title,
            sink: sink.clone(),
            done: false,
            ok: false,
            error: None,
        });

        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;

        thread::spawn(move || {
            let logger = Arc::new(Logger::for_command("tui-promote", verbose));
            let mut context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Done {
                        ok: false,
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };

            context = context
                .with_output(sink.clone() as Arc<dyn OutputSink>)
                .with_confirm(Arc::new(AlwaysYesConfirm));

            let args = crate::commands::promote::PromoteCommand {
                branch: branch.cli_ref().to_string(),
                env_name: env,
                no_rebuild: false,
            };
            let res = crate::commands::promote::run(args, &context);
            let _ = tx.send(WorkerMsg::Done {
                ok: res.is_ok(),
                error: res.err().map(|e| e.to_string()),
            });
        });
    }

    pub(super) fn start_rebuild(&mut self, env_name: String) {
        let sink = BufferedOutputSink::new();
        sink.log(OutputLevel::Info, &format!("Rebuilding '{}'", env_name));

        let title = format!("Rebuild {}", env_name);
        self.modal = Some(Modal::Operation {
            title,
            sink: sink.clone(),
            done: false,
            ok: false,
            error: None,
        });

        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;

        thread::spawn(move || {
            let logger = Arc::new(Logger::for_command("tui-rebuild", verbose));
            let mut context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Done {
                        ok: false,
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };

            context = context
                .with_output(sink.clone() as Arc<dyn OutputSink>)
                .with_confirm(Arc::new(AlwaysYesConfirm));

            let args = crate::commands::rebuild::RebuildCommand {
                env_name,
                reuse_resolutions: true,
                force: false,
            };
            let res = crate::commands::rebuild::run(args, &context);
            let _ = tx.send(WorkerMsg::Done {
                ok: res.is_ok(),
                error: res.err().map(|e| e.to_string()),
            });
        });
    }

    pub(super) fn start_release(&mut self, env_name: String) {
        let sink = BufferedOutputSink::new();
        sink.log(OutputLevel::Warning, &format!("Releasing '{}'", env_name));

        let title = format!("Release {}", env_name);
        self.modal = Some(Modal::Operation {
            title,
            sink: sink.clone(),
            done: false,
            ok: false,
            error: None,
        });

        let tx = self.worker_tx.clone();
        let verbose = self.context.verbose;
        let no_push = self.context.no_push;

        thread::spawn(move || {
            let logger = Arc::new(Logger::for_command("tui-release", verbose));
            let mut context = match GlobalContext::new(verbose, no_push, logger) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::Done {
                        ok: false,
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };

            context = context
                .with_output(sink.clone() as Arc<dyn OutputSink>)
                .with_confirm(Arc::new(AlwaysYesConfirm));

            // NOTE: ReleaseCommand prompts via stdin unless force=true, so the TUI always uses force
            // and provides confirmation via a modal.
            let args = crate::commands::release::ReleaseCommand {
                env_name,
                target_branch: None,
                force: true,
            };
            let res = crate::commands::release::run(args, &context);
            let _ = tx.send(WorkerMsg::Done {
                ok: res.is_ok(),
                error: res.err().map(|e| e.to_string()),
            });
        });
    }

    pub(super) fn available_actions(&self) -> Vec<Action> {
        if self.details_loading.is_some() || self.index_loading {
            return Vec::new();
        }
        match self.selected_selection() {
            Some(Selection::Environment { .. }) => vec![Action::Rebuild, Action::Release],
            Some(Selection::Branch { row }) => {
                if row.is_environment {
                    vec![Action::Rebuild]
                } else {
                    vec![Action::Promote]
                }
            }
            None => Vec::new(),
        }
    }

    pub(super) fn actions_enabled(&self) -> bool {
        !self.index_loading && self.details_loading.is_none() && self.modal.is_none()
    }

    fn start_activity(&mut self, kind: ActivityKind, msg: &str) {
        let now = Instant::now();
        self.activity_started_at = Some(now);
        self.activity_kind = Some(kind);
        self.activity_msg = msg.to_string();
        self.spinner_frame = 0;
        self.spinner_last_advance = now;
    }

    fn clear_activity(&mut self) {
        self.activity_started_at = None;
        self.activity_kind = None;
        self.activity_msg.clear();
        self.spinner_frame = 0;
        self.spinner_last_advance = Instant::now();
    }

    fn reload_after_operation(&mut self) {
        self.index = None;
        self.index_loading = true;
        self.status_summary = None;
        self.branch_cache.clear();
        self.env_cache.clear();
        self.rebuild_list();
        self.start_load_workspace_index();
    }

    fn matches_details_loading_token(&self, token: u64) -> bool {
        self.details_loading
            .as_ref()
            .is_some_and(|d| d.token == token)
    }

    pub(super) fn current_selection_key(&self) -> Option<SelectionKey> {
        self.selected_selection().map(|s| match s {
            Selection::Environment { name } => SelectionKey::Environment(name),
            Selection::Branch { row } => SelectionKey::Branch(row.name),
        })
    }

    pub(super) fn bump_token(&mut self) -> u64 {
        let t = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        t
    }
}
