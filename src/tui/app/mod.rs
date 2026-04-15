//! Hitch TUI application.

mod events;
mod types;
mod ui;
mod workers;

use crate::commands::global_context::GlobalContext;
use crate::tui::terminal::TerminalGuard;
use crate::utils::logging::Logger;
use anyhow::Result;
use crossterm::event;
use ratatui::Terminal;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use types::{App, Focus, Tab, WorkerMsg};

pub fn run_tui(verbose: bool, no_push: bool) -> Result<()> {
    let mut guard = TerminalGuard::enter()?;

    // Ensure we always restore the terminal even on panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        default_hook(info);
    }));

    let logger = Arc::new(Logger::for_command("tui", verbose));
    let context =
        GlobalContext::new(verbose, no_push, logger).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    {
        let backend = ratatui::backend::CrosstermBackend::new(guard.stdout_mut());
        let mut terminal = Terminal::new(backend)?;

        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMsg>();
        let mut app = App::new(context, worker_tx);

        let tick_rate = Duration::from_millis(33);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|f| app.draw(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                let ev = event::read()?;
                if app.on_event(ev)? {
                    break;
                }
            }

            while let Ok(msg) = worker_rx.try_recv() {
                app.on_worker_msg(msg);
            }

            if last_tick.elapsed() >= tick_rate {
                app.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    guard.restore();
    Ok(())
}

impl App {
    fn new(context: GlobalContext, worker_tx: mpsc::Sender<WorkerMsg>) -> Self {
        let mut app = Self {
            context,
            focus: Focus::List,
            tab: Tab::Overview,

            filter: String::new(),
            filter_applied: String::new(),
            pending_filter_apply_at: None,

            index: None,
            index_loading: true,
            index_load_token: 0,
            index_progress_pct: 0,
            index_progress_msg: "Loading workspace index…".to_string(),

            status_summary: None,
            status_summary_token: 0,

            list_entries: Vec::new(),
            list_state: Default::default(),

            status_line: "Ready".to_string(),

            details_loading: None,
            next_token: 1,
            pending_selection_load_at: None,
            last_polled_at: Instant::now(),

            branch_cache: Default::default(),
            env_cache: Default::default(),

            hide_hitch_branches: true,

            modal: None,
            worker_tx,

            filter_rect: Default::default(),
            list_rect: Default::default(),
            details_rect: Default::default(),
            footer_rect: Default::default(),

            sidebar_name_width: 0,
            marquee_active_key: None,
            marquee_offset: 0,
            marquee_last_advance: Instant::now(),

            activity_started_at: None,
            activity_kind: None,
            activity_msg: String::new(),
            spinner_frame: 0,
            spinner_last_advance: Instant::now(),

            timeline_scroll: 0,
        };

        app.rebuild_list();
        app.list_state.select(Some(0));
        app.start_load_workspace_index();
        app
    }
}
