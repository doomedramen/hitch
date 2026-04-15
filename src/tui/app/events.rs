use super::types::{
    Action, App, Focus, Modal, Selection, SelectionKey, Tab, FILTER_DEBOUNCE, INDICATOR_SHOW_DELAY,
    SELECTED_POLL_INTERVAL, SELECTION_DEBOUNCE, SPINNER_STEP,
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::time::Instant;

impl App {
    pub(super) fn on_tick(&mut self) {
        let now = Instant::now();

        if let Some(at) = self.pending_filter_apply_at {
            if now >= at {
                self.pending_filter_apply_at = None;
                self.apply_filter();

                // Only kick off details loading if the user isn't actively typing in the filter.
                if self.focus != Focus::Filter && self.modal.is_none() {
                    self.schedule_selected_details_load();
                }
            }
        }

        if let Some(at) = self.pending_selection_load_at {
            if now >= at {
                self.pending_selection_load_at = None;
                if self.modal.is_none() {
                    self.start_load_selected_details();
                }
            }
        }

        // Activity spinner (only after a small delay to avoid flicker).
        if let Some(started) = self.activity_started_at {
            if now.duration_since(started) >= INDICATOR_SHOW_DELAY
                && now.duration_since(self.spinner_last_advance) >= SPINNER_STEP
            {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
                self.spinner_last_advance = now;
            }
        }

        // Selected-row marquee.
        const MARQUEE_STEP: std::time::Duration = std::time::Duration::from_millis(140);
        if self.modal.is_none()
            && self.focus != Focus::Filter
            && self.pending_filter_apply_at.is_none()
            && self.marquee_active_key.is_some()
            && now.duration_since(self.marquee_last_advance) >= MARQUEE_STEP
        {
            if let Some((key, full_name)) = self.selected_display_name() {
                // Only marquee when the selected key matches and it is actually too wide.
                if self.marquee_active_key.as_ref() == Some(&key) {
                    let name_width = self.sidebar_name_width as usize;
                    if super::ui::display_width(&full_name) > name_width && name_width > 0 {
                        self.marquee_offset = self.marquee_offset.saturating_add(1);
                        self.marquee_last_advance = now;
                    }
                }
            }
        }

        // Background refresh for selected item only.
        if self.modal.is_none()
            && self.details_loading.is_none()
            && self.index.is_some()
            && self.focus != Focus::Filter
            && self.pending_filter_apply_at.is_none()
            && now.duration_since(self.last_polled_at) >= SELECTED_POLL_INTERVAL
        {
            self.last_polled_at = now;
            self.start_load_selected_details();
        }
    }

    pub(super) fn on_event(&mut self, ev: Event) -> Result<bool> {
        if self.modal.is_some() {
            return self.on_modal_event(ev);
        }

        match ev {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return Ok(false);
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(true),
                    KeyCode::Char('?') => {
                        self.modal = Some(Modal::Help);
                    }
                    KeyCode::Char('h') | KeyCode::Char('H') => {
                        self.hide_hitch_branches = !self.hide_hitch_branches;
                        self.rebuild_list();
                        let current = self.selected_index();
                        self.list_state.select(Some(self.clamp_selection(current)));
                        let sel = self.current_selection_key();
                        self.reset_marquee(sel.clone());
                        self.timeline_scroll = 0;
                        self.schedule_selected_details_load();
                    }
                    KeyCode::Char('/') => {
                        self.focus = Focus::Filter;
                    }
                    KeyCode::Tab => {
                        self.focus = match self.focus {
                            Focus::Filter => Focus::List,
                            Focus::List => Focus::Details,
                            Focus::Details => Focus::Filter,
                        };
                    }
                    KeyCode::Esc => {
                        self.focus = Focus::List;
                    }
                    KeyCode::Char('1') => {
                        if self.tab != Tab::Overview {
                            self.tab = Tab::Overview;
                            self.timeline_scroll = 0;
                        }
                    }
                    KeyCode::Char('2') => {
                        if self.tab != Tab::Timeline {
                            self.tab = Tab::Timeline;
                            self.timeline_scroll = 0;
                        }
                    }
                    _ => self.dispatch_focus_key(key.code)?,
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.on_mouse_click(m.column, m.row)?;
                }
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    if self.contains(self.list_rect, (m.column, m.row)) {
                        self.focus = Focus::List;
                    } else if self.contains(self.details_rect, (m.column, m.row)) {
                        self.focus = Focus::Details;
                        if self.tab == Tab::Timeline {
                            let delta: i32 = if matches!(m.kind, MouseEventKind::ScrollUp) {
                                -3
                            } else {
                                3
                            };
                            self.timeline_scroll = apply_scroll_delta(self.timeline_scroll, delta);
                        }
                    }
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }

        Ok(false)
    }

    fn dispatch_focus_key(&mut self, code: KeyCode) -> Result<()> {
        match self.focus {
            Focus::Filter => self.on_key_filter(code),
            Focus::List => self.on_key_list(code),
            Focus::Details => self.on_key_details(code),
        }
    }

    fn on_key_filter(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                self.pending_filter_apply_at = None;
                self.apply_filter();
                self.focus = Focus::List;
                self.schedule_selected_details_load();
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.pending_filter_apply_at = Some(Instant::now() + FILTER_DEBOUNCE);
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.pending_filter_apply_at = Some(Instant::now() + FILTER_DEBOUNCE);
            }
            _ => {}
        }
        Ok(())
    }

    fn on_key_list(&mut self, code: KeyCode) -> Result<()> {
        let selected = self.selected_index();
        let before = self.current_selection_key();

        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(n) = self.next_selectable(selected, -1) {
                    self.list_state.select(Some(n));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(n) = self.next_selectable(selected, 1) {
                    self.list_state.select(Some(n));
                }
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                if self.actions_enabled() {
                    self.try_open_promote_picker();
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.actions_enabled() {
                    self.try_open_rebuild();
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                if self.actions_enabled() {
                    self.try_open_release();
                }
            }
            _ => {}
        }

        let after = self.current_selection_key();
        if before != after {
            self.reset_marquee(after.clone());
            self.timeline_scroll = 0;
            self.schedule_selected_details_load();
        }
        Ok(())
    }

    fn on_key_details(&mut self, code: KeyCode) -> Result<()> {
        if self.tab != Tab::Timeline {
            return Ok(());
        }
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.timeline_scroll = self.timeline_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.timeline_scroll = self.timeline_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.timeline_scroll = self.timeline_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.timeline_scroll = self.timeline_scroll.saturating_add(10);
            }
            KeyCode::Home => {
                self.timeline_scroll = 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn on_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        let p = (x, y);
        if self.contains(self.filter_rect, p) {
            self.focus = Focus::Filter;
            return Ok(());
        }
        if self.contains(self.list_rect, p) {
            self.focus = Focus::List;
            let before = self.current_selection_key();

            let inner_top = self.list_rect.y.saturating_add(1);
            if y >= inner_top {
                let idx = (y - inner_top) as usize + self.list_state.offset();
                if idx < self.list_entries.len() && self.list_entries[idx].selectable {
                    self.list_state.select(Some(idx));
                }
            }

            let after = self.current_selection_key();
            if before != after {
                self.reset_marquee(after.clone());
                self.timeline_scroll = 0;
                self.schedule_selected_details_load();
            }
            return Ok(());
        }
        if self.contains(self.footer_rect, p) {
            // Only "safe" actions are clickable: Promote/Rebuild.
            if self.actions_enabled() {
                let actions = self.available_actions();
                if actions.contains(&Action::Promote) {
                    self.try_open_promote_picker();
                } else if actions.contains(&Action::Rebuild) {
                    self.try_open_rebuild();
                }
            }
            return Ok(());
        }
        if self.contains(self.details_rect, p) {
            self.focus = Focus::Details;
        }
        Ok(())
    }

    fn try_open_promote_picker(&mut self) {
        let Some(Selection::Branch { row }) = self.selected_selection() else {
            self.status_line = "Select a branch to promote".to_string();
            return;
        };
        if row.is_environment {
            self.status_line = "Select a normal branch (not env/*)".to_string();
            return;
        }

        let Some(index) = self.index.as_ref() else {
            self.status_line = "Workspace index is still loading".to_string();
            return;
        };

        let envs: Vec<String> = index.environments.iter().map(|e| e.name.clone()).collect();
        if envs.is_empty() {
            self.status_line = "No environments configured".to_string();
            return;
        }

        self.modal = Some(Modal::PromotePicker {
            branch: row,
            env_index: 0,
            envs,
        });
    }

    fn try_open_rebuild(&mut self) {
        let Some(Selection::Environment { name }) = self.selected_selection() else {
            self.status_line = "Select an environment to rebuild".to_string();
            return;
        };
        self.modal = Some(Modal::ConfirmRebuild { env_name: name });
    }

    fn try_open_release(&mut self) {
        let Some(Selection::Environment { name }) = self.selected_selection() else {
            self.status_line = "Select an environment to release".to_string();
            return;
        };
        let Some(index) = self.index.as_ref() else {
            self.status_line = "Workspace index is still loading".to_string();
            return;
        };
        let target = index
            .environments
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.base.clone())
            .unwrap_or_else(|| "(unknown)".to_string());
        self.modal = Some(Modal::ConfirmRelease {
            env_name: name,
            target_branch: target,
        });
    }

    fn on_modal_event(&mut self, ev: Event) -> Result<bool> {
        let Some(mut modal) = self.modal.take() else {
            return Ok(false);
        };

        let mut keep_modal = true;

        match &mut modal {
            Modal::Help => match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc | KeyCode::Char('?') => keep_modal = false,
                    _ => {}
                },
                _ => {}
            },
            Modal::PromotePicker {
                branch,
                env_index,
                envs,
            } => match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc => keep_modal = false,
                    KeyCode::Up | KeyCode::Char('k') => {
                        *env_index = env_index.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *env_index + 1 < envs.len() {
                            *env_index += 1;
                        }
                    }
                    KeyCode::Enter => {
                        let env = envs.get(*env_index).cloned().unwrap_or_default();
                        keep_modal = false;
                        self.start_promote(branch.clone(), env);
                    }
                    _ => {}
                },
                _ => {}
            },
            Modal::ConfirmRebuild { env_name } => match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc => keep_modal = false,
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        let env_name = env_name.clone();
                        keep_modal = false;
                        self.start_rebuild(env_name);
                    }
                    _ => {}
                },
                _ => {}
            },
            Modal::ConfirmRelease {
                env_name,
                target_branch: _,
            } => match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc => keep_modal = false,
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        let env_name = env_name.clone();
                        keep_modal = false;
                        self.start_release(env_name);
                    }
                    _ => {}
                },
                _ => {}
            },
            Modal::Operation { done, .. } => match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc => {
                    if *done {
                        keep_modal = false;
                    }
                }
                _ => {}
            },
        }

        if keep_modal && self.modal.is_none() {
            self.modal = Some(modal);
        }
        Ok(false)
    }

    fn reset_marquee(&mut self, key: Option<SelectionKey>) {
        self.marquee_active_key = key;
        self.marquee_offset = 0;
        self.marquee_last_advance = Instant::now();
    }

    pub(super) fn schedule_selected_details_load(&mut self) {
        self.pending_selection_load_at = Some(Instant::now() + SELECTION_DEBOUNCE);
    }

    pub(super) fn rebuild_list(&mut self) {
        let mut entries = Vec::new();

        if self.index_loading || self.index.is_none() {
            entries.push(super::types::ListEntry {
                selectable: false,
                label: "Loading branches…".to_string(),
                selection: None,
                promoted_section: false,
            });
            self.list_entries = entries;
            self.list_state.select(Some(0));
            return;
        }

        let index = self.index.as_ref().expect("checked above");
        let filter = self.filter_applied.trim().to_lowercase();
        let matches = |name: &str| -> bool {
            if self.hide_hitch_branches && is_hitch_internal_branch(name) {
                return false;
            }
            if filter.is_empty() {
                true
            } else {
                name.to_lowercase().contains(&filter)
            }
        };

        entries.push(super::types::ListEntry {
            selectable: false,
            label: "HITCH".to_string(),
            selection: None,
            promoted_section: false,
        });

        for env in &index.environments {
            if !matches(&env.name) {
                continue;
            }
            let lock_icon = if env.locked { "🔒" } else { "" };
            let approval_icon = if env.requires_approval { "🛂" } else { "" };
            let badge = [lock_icon, approval_icon]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let label = if badge.is_empty() {
                format!("env/{}", env.name)
            } else {
                format!("env/{}  {}", env.name, badge)
            };
            entries.push(super::types::ListEntry {
                selectable: true,
                label,
                selection: Some(Selection::Environment {
                    name: env.name.clone(),
                }),
                promoted_section: false,
            });
        }

        if !index.promoted_branches.is_empty() {
            entries.push(super::types::ListEntry {
                selectable: false,
                label: "PROMOTED".to_string(),
                selection: None,
                promoted_section: false,
            });
            for b in &index.promoted_branches {
                if !matches(&b.name) {
                    continue;
                }
                entries.push(super::types::ListEntry {
                    selectable: true,
                    label: b.name.clone(),
                    selection: Some(Selection::Branch { row: b.clone() }),
                    promoted_section: true,
                });
            }
        }

        entries.push(super::types::ListEntry {
            selectable: false,
            label: "BRANCHES".to_string(),
            selection: None,
            promoted_section: false,
        });
        for b in &index.branches {
            if !matches(&b.name) {
                continue;
            }
            entries.push(super::types::ListEntry {
                selectable: true,
                label: b.name.clone(),
                selection: Some(Selection::Branch { row: b.clone() }),
                promoted_section: false,
            });
        }

        self.list_entries = entries;
    }

    fn apply_filter(&mut self) {
        self.filter_applied = self.filter.clone();
        self.rebuild_list();
        let current = self.selected_index();
        self.list_state.select(Some(self.clamp_selection(current)));
        self.marquee_active_key = self.current_selection_key();
        self.marquee_offset = 0;
        self.marquee_last_advance = Instant::now();
        self.timeline_scroll = 0;
        // Intentionally do not auto-load details here; filter should be a cheap list operation.
    }

    pub(super) fn clamp_selection(&self, idx: usize) -> usize {
        if self.list_entries.is_empty() {
            return 0;
        }
        let mut i = idx.min(self.list_entries.len().saturating_sub(1));
        if self.list_entries.get(i).is_some_and(|e| e.selectable) {
            return i;
        }
        if let Some(n) = self.next_selectable(i, 1) {
            i = n;
        } else if let Some(n) = self.next_selectable(i, -1) {
            i = n;
        }
        i
    }

    pub(super) fn first_selectable_index(&self) -> Option<usize> {
        self.list_entries.iter().position(|e| e.selectable)
    }

    fn next_selectable(&self, from: usize, dir: i32) -> Option<usize> {
        let mut i = from as i32;
        loop {
            i += dir;
            if i < 0 || i >= self.list_entries.len() as i32 {
                return None;
            }
            let ui = i as usize;
            if self.list_entries[ui].selectable {
                return Some(ui);
            }
        }
    }

    fn selected_index(&self) -> usize {
        self.list_state.selected().unwrap_or(0)
    }

    pub(super) fn selected_entry(&self) -> Option<&super::types::ListEntry> {
        self.list_entries.get(self.selected_index())
    }

    pub(super) fn selected_selection(&self) -> Option<Selection> {
        self.selected_entry().and_then(|e| e.selection.clone())
    }

    fn selected_display_name(&self) -> Option<(SelectionKey, String)> {
        let selected = self.selected_selection()?;
        match selected {
            Selection::Environment { name } => {
                Some((SelectionKey::Environment(name.clone()), name))
            }
            Selection::Branch { row } => {
                let name = super::ui::branch_display_name(&row);
                Some((SelectionKey::Branch(row.name.clone()), name))
            }
        }
    }

    pub(super) fn contains(&self, rect: Rect, point: (u16, u16)) -> bool {
        let (x, y) = point;
        x >= rect.x
            && x < rect.x.saturating_add(rect.width)
            && y >= rect.y
            && y < rect.y.saturating_add(rect.height)
    }
}

fn apply_scroll_delta(current: u16, delta: i32) -> u16 {
    if delta < 0 {
        current.saturating_sub(delta.unsigned_abs() as u16)
    } else {
        current.saturating_add(delta as u16)
    }
}

fn is_hitch_internal_branch(name: &str) -> bool {
    name == "hitch-metadata" || name.starts_with("hitch-")
}
