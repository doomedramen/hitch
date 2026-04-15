use super::types::{Action, App, Focus, Modal, Selection, SelectionKey, Tab};
use crate::core::timeline::TimelineKind;
use crate::core::workspace::BranchRow;
use crate::utils::output::OutputLevel;
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::Instant;

impl App {
    pub(super) fn draw(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)].as_ref())
            .split(f.area());
        let main = chunks[0];
        let footer = chunks[1];
        self.footer_rect = footer;

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(44), Constraint::Min(1)].as_ref())
            .split(main);
        let left = cols[0];
        let right = cols[1];
        self.details_rect = right;

        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)].as_ref())
            .split(left);
        self.filter_rect = left_rows[0];
        self.list_rect = left_rows[1];

        self.draw_filter(f, self.filter_rect);
        self.draw_list(f, self.list_rect);
        self.draw_details(f, right);
        self.draw_footer(f, footer);

        if let Some(modal) = &self.modal {
            self.draw_modal(f, modal);
        }
    }

    fn draw_filter(&self, f: &mut Frame, area: Rect) {
        let style = if self.focus == Focus::Filter {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let dirty = if self.filter != self.filter_applied {
            "*"
        } else {
            ""
        };
        let title = format!("Filter{} (/)", dirty);
        let p = Paragraph::new(self.filter.as_str())
            .style(style)
            .block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(p, area);
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let current = self.list_state.selected().unwrap_or(0);
        let clamped = self.clamp_selection(current);
        self.list_state.select(Some(clamped));

        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let inner_width = inner.width as usize;

        // Reserve fixed space: 2 for selection marker + 6 for icon badge + 1 space.
        let name_width = inner_width.saturating_sub(2 + 6 + 1);
        self.sidebar_name_width = name_width.min(u16::MAX as usize) as u16;

        let selected_idx = self.list_state.selected().unwrap_or(0);
        let items: Vec<ListItem> = self
            .list_entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if !e.selectable {
                    return ListItem::new(Line::from(vec![Span::styled(
                        e.label.clone(),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )]));
                }

                let selected = i == selected_idx;
                let marker = if selected { "▶ " } else { "  " };

                let line = match e.selection.as_ref() {
                    Some(Selection::Branch { row }) => {
                        let badge = branch_badge(row, e.promoted_section);
                        let full = branch_display_name(row);
                        let name = if selected
                            && self
                                .marquee_active_key
                                .as_ref()
                                .is_some_and(|k| matches!(k, super::types::SelectionKey::Branch(b) if b == &row.name))
                            && display_width(&full) > name_width
                            && name_width > 0
                        {
                            marquee_window(&full, self.marquee_offset, name_width)
                        } else {
                            middle_ellipsis(&full, name_width)
                        };

                        Line::from(vec![
                            Span::raw(marker),
                            Span::raw(badge),
                            Span::raw(" "),
                            Span::raw(name),
                        ])
                    }
                    Some(Selection::Environment { name: _ }) => {
                        let name = middle_ellipsis(&e.label, name_width);
                        Line::from(vec![
                            Span::raw(marker),
                            Span::raw("[    ]"),
                            Span::raw(" "),
                            Span::raw(name),
                        ])
                    }
                    None => Line::from(vec![Span::raw(e.label.clone())]),
                };

                ListItem::new(line)
            })
            .collect();

        let title = if let Some(index) = &self.index {
            if let Some(b) = &index.current_branch {
                format!("Branches ({})", b)
            } else {
                "Branches".to_string()
            }
        } else {
            "Branches".to_string()
        };

        let selected_label = self
            .selected_entry()
            .and_then(|e| e.selection.as_ref())
            .map(|s| match s {
                Selection::Environment { name } => format!("env/{}", name),
                Selection::Branch { row } => row.name.clone(),
            })
            .unwrap_or_else(|| "-".to_string());

        // Always show selected row differently, even when the list isn't focused.
        let highlight_style = if self.focus == Focus::List {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{}  •  Selected: {}", title, selected_label)),
            )
            .highlight_style(highlight_style)
            .highlight_symbol("");
        f.render_stateful_widget(list, area, &mut self.list_state);

        if self.focus == Focus::List {
            f.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
                area,
            );
        }
    }

    fn draw_details(&mut self, f: &mut Frame, area: Rect) {
        let title = match self.selected_selection() {
            Some(Selection::Environment { ref name }) => format!("Environment: {}", name),
            Some(Selection::Branch { ref row }) => format!("Branch: {}", row.name),
            None => "Details".to_string(),
        };
        let block = Block::default().borders(Borders::ALL).title(title);
        f.render_widget(block, area);

        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)].as_ref())
            .split(inner);
        let tab_row = rows[0];
        let body = rows[1];

        let mut tabs = match self.tab {
            Tab::Overview => "[Overview]  Timeline(2)",
            Tab::Timeline => "Overview(1)  [Timeline]",
        }
        .to_string();
        if self.should_show_details_progress() {
            if let Some(load) = &self.details_loading {
                tabs.push_str(&format!("  •  {} {}%", load.msg, load.pct));
            }
        }
        f.render_widget(
            Paragraph::new(tabs).style(Style::default().fg(Color::Yellow)),
            tab_row,
        );

        match self.tab {
            Tab::Overview => self.draw_overview(f, body),
            Tab::Timeline => self.draw_timeline(f, body),
        }

        if self.focus == Focus::Details {
            f.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
                area,
            );
        }
    }

    fn draw_overview(&self, f: &mut Frame, area: Rect) {
        let text = match self.selected_selection() {
            Some(Selection::Environment { ref name }) => self
                .env_cache
                .get(name)
                .map(|c| c.model.overview.clone())
                .unwrap_or_else(|| {
                    if self.index_loading || self.index.is_none() {
                        "Loading workspace…".to_string()
                    } else {
                        "Loading environment details…".to_string()
                    }
                }),
            Some(Selection::Branch { ref row }) => self
                .branch_cache
                .get(&row.name)
                .map(|c| c.model.overview.clone())
                .unwrap_or_else(|| {
                    if self.index_loading || self.index.is_none() {
                        "Loading workspace…".to_string()
                    } else {
                        "Loading branch details…".to_string()
                    }
                }),
            None => "Select a branch/environment".to_string(),
        };
        f.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title("Overview")),
            area,
        );
    }

    fn draw_timeline(&mut self, f: &mut Frame, area: Rect) {
        let items = match self.selected_selection() {
            Some(Selection::Environment { ref name }) => self
                .env_cache
                .get(name)
                .map(|c| c.model.timeline.clone())
                .unwrap_or_default(),
            Some(Selection::Branch { ref row }) => self
                .branch_cache
                .get(&row.name)
                .map(|c| c.model.timeline.clone())
                .unwrap_or_default(),
            None => Vec::new(),
        };

        let mut lines = Vec::new();
        for item in items.into_iter().take(200) {
            let ts = item.when.format("%Y-%m-%d %H:%M").to_string();
            let icon = match item.kind {
                TimelineKind::GitCommit => "⬆",
                TimelineKind::HitchEvent => "🚀",
            };
            lines.push(Line::from(vec![
                Span::styled(ts, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::raw(icon),
                Span::raw(" "),
                Span::raw(item.summary),
            ]));
        }
        if lines.is_empty() {
            lines.push(Line::raw("No timeline entries (yet)."));
        }

        // Clamp scroll to content height.
        let inner_height = area.height.saturating_sub(2) as usize;
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(inner_height) as u16;
        if self.timeline_scroll > max_scroll {
            self.timeline_scroll = max_scroll;
        }

        f.render_widget(
            Paragraph::new(Text::from(lines))
                .scroll((self.timeline_scroll, 0))
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title("Timeline")),
            area,
        );
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)].as_ref())
            .split(area);

        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(
            self.status_line.clone(),
            Style::default().fg(Color::Black).bg(Color::Gray),
        ));

        if let Some(index) = &self.index {
            let (envs, locked, needs_rebuild, never_rebuilt) = if let Some(s) = &self.status_summary
            {
                (
                    s.total_envs,
                    s.locked_envs,
                    s.needs_rebuild_envs,
                    s.never_rebuilt_envs,
                )
            } else {
                (
                    index.environments.len(),
                    index.environments.iter().filter(|e| e.locked).count(),
                    usize::MAX,
                    usize::MAX,
                )
            };

            let rebuild_text = if needs_rebuild == usize::MAX {
                "…".to_string()
            } else {
                needs_rebuild.to_string()
            };
            let never_text = if never_rebuilt == usize::MAX {
                "…".to_string()
            } else {
                never_rebuilt.to_string()
            };

            spans.push(Span::raw(format!(
                "  envs:{} locked:{} rebuild:{} never:{}  ",
                envs, locked, rebuild_text, never_text
            )));
        } else {
            spans.push(Span::raw("  envs:… locked:… rebuild:… never:…  "));
        }

        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "q",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(":quit  "));
        spans.push(Span::styled(
            "?",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(":help  "));
        spans.push(Span::styled(
            "H",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(if self.hide_hitch_branches {
            ":show hitch-*  "
        } else {
            ":hide hitch-*  "
        }));
        spans.push(Span::styled(
            "/",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(":filter  "));
        spans.push(Span::styled(
            "1/2",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(":tabs  "));

        let actions = self.available_actions();
        if actions.contains(&Action::Promote) {
            spans.push(Span::styled(
                "P",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(":promote  "));
        }
        if actions.contains(&Action::Rebuild) {
            spans.push(Span::styled(
                "R",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(":rebuild  "));
        }
        if actions.contains(&Action::Release) {
            spans.push(Span::styled(
                "L",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(":release  "));
        }

        f.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

        let line2 = self.activity_line();
        f.render_widget(
            Paragraph::new(line2).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }

    fn activity_line(&self) -> String {
        let Some(started) = self.activity_started_at else {
            return String::new();
        };
        if Instant::now().duration_since(started) < super::types::INDICATOR_SHOW_DELAY {
            return String::new();
        }
        let frames: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let spinner = frames[self.spinner_frame % frames.len()];
        let msg = self.activity_msg.trim();
        if msg.is_empty() {
            spinner.to_string()
        } else {
            format!("{} {}", spinner, msg)
        }
    }

    fn draw_modal(&self, f: &mut Frame, modal: &Modal) {
        match modal {
            Modal::Help => {
                let area = centered_rect(80, 60, f.area());
                f.render_widget(Clear, area);
                let text = [
                    "Legend:",
                    "  [●] local branch",
                    "  [○] origin remote-tracking branch",
                    "  [★] promoted (in PROMOTED section)",
                    "  [◆] base branch for one+ envs",
                    "",
                    "Keys:",
                    "  q: quit",
                    "  /: focus filter",
                    "  H: toggle hitch-* branches",
                    "  Tab: cycle focus",
                    "  1/2: switch tabs",
                    "  Timeline: mouse wheel / ↑↓ scroll (when focused)",
                    "  P: promote (branch)",
                    "  R: rebuild (env)",
                    "  L: release to base (env)",
                    "  Esc: close modals",
                ]
                .join("\n");
                f.render_widget(
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .block(Block::default().borders(Borders::ALL).title("Help (?)")),
                    area,
                );
            }
            Modal::PromotePicker {
                branch,
                env_index,
                envs,
            } => {
                let area = centered_rect(70, 60, f.area());
                f.render_widget(Clear, area);
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(2), Constraint::Min(1)].as_ref())
                    .split(area);
                f.render_widget(
                    Paragraph::new(format!(
                        "Promote '{}' to environment (Enter to select, Esc to cancel)",
                        branch.cli_ref()
                    ))
                    .block(Block::default().borders(Borders::ALL).title("Promote")),
                    chunks[0],
                );
                let items: Vec<ListItem> = envs.iter().map(|e| ListItem::new(e.clone())).collect();
                let mut state = ListState::default();
                state.select(Some(*env_index));
                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::default().borders(Borders::ALL))
                        .highlight_style(
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("▶ "),
                    chunks[1],
                    &mut state,
                );
            }
            Modal::ConfirmRebuild { env_name } => {
                let area = centered_rect(60, 20, f.area());
                f.render_widget(Clear, area);
                let text = format!(
                    "Rebuild env '{}'\n\nEnter/Y: rebuild\nEsc: cancel",
                    env_name
                );
                f.render_widget(
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .block(Block::default().borders(Borders::ALL).title("Confirm")),
                    area,
                );
            }
            Modal::ConfirmRelease {
                env_name,
                target_branch,
            } => {
                let area = centered_rect(70, 25, f.area());
                f.render_widget(Clear, area);
                let text = format!(
                    "Release env '{}' → base '{}'\n\nThis merges permanently.\n\nEnter/Y: release\nEsc: cancel",
                    env_name, target_branch
                );
                f.render_widget(
                    Paragraph::new(text).wrap(Wrap { trim: false }).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Release (dangerous)"),
                    ),
                    area,
                );
            }
            Modal::Operation {
                title,
                sink,
                done,
                ok,
                error,
            } => {
                let area = centered_rect(90, 85, f.area());
                f.render_widget(Clear, area);
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(
                        [
                            Constraint::Length(2),
                            Constraint::Min(1),
                            Constraint::Length(1),
                        ]
                        .as_ref(),
                    )
                    .split(area);

                f.render_widget(
                    Paragraph::new(title.clone())
                        .block(Block::default().borders(Borders::ALL).title("Operation")),
                    chunks[0],
                );

                let lines = sink.snapshot();
                let mut out = Vec::new();
                for l in lines.into_iter().rev().take(400).rev() {
                    let style = match l.level {
                        OutputLevel::Info => Style::default().fg(Color::White),
                        OutputLevel::Success => Style::default().fg(Color::Green),
                        OutputLevel::Warning => Style::default().fg(Color::Yellow),
                        OutputLevel::Error => Style::default().fg(Color::Red),
                    };
                    out.push(Line::from(Span::styled(l.message, style)));
                }
                if out.is_empty() {
                    out.push(Line::raw("No output yet…"));
                }
                f.render_widget(
                    Paragraph::new(Text::from(out))
                        .wrap(Wrap { trim: false })
                        .block(Block::default().borders(Borders::ALL).title("Logs")),
                    chunks[1],
                );

                let footer = if *done {
                    if *ok {
                        "Done (Esc to close)".to_string()
                    } else {
                        format!(
                            "Failed: {} (Esc to close)",
                            error.clone().unwrap_or_else(|| "unknown error".to_string())
                        )
                    }
                } else {
                    "Running… (Esc disabled)".to_string()
                };
                f.render_widget(Paragraph::new(footer), chunks[2]);
            }
        }
    }

    fn should_show_details_progress(&self) -> bool {
        let Some(load) = &self.details_loading else {
            return false;
        };
        match (self.selected_selection(), &load.key) {
            (Some(Selection::Branch { row }), SelectionKey::Branch(b)) => &row.name == b,
            (Some(Selection::Environment { name }), SelectionKey::Environment(e)) => &name == e,
            _ => false,
        }
    }
}

pub(super) fn branch_display_name(row: &BranchRow) -> String {
    if !row.local && row.remote {
        format!("origin/{}", row.name)
    } else {
        row.name.clone()
    }
}

fn branch_badge(row: &BranchRow, promoted_section: bool) -> String {
    let local = if row.local { "●" } else { " " };
    let remote = if row.remote { "○" } else { " " };
    let promoted = if promoted_section { "★" } else { " " };
    let base = if !row.base_for.is_empty() { "◆" } else { " " };
    format!("[{}{}{}{}]", local, remote, promoted, base)
}

pub(super) fn display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

fn middle_ellipsis(s: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    if display_width(s) <= max_cols {
        return s.to_string();
    }
    if max_cols == 1 {
        return "…".to_string();
    }

    let target = max_cols.saturating_sub(1); // reserve for ellipsis
    let left_cols = target / 2;
    let right_cols = target - left_cols;

    let left = take_prefix_cols(s, left_cols);
    let right = take_suffix_cols(s, right_cols);
    format!("{}…{}", left, right)
}

fn marquee_window(s: &str, offset_chars: usize, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    if display_width(s) <= max_cols {
        return s.to_string();
    }
    let gap = "   ";
    let loop_text = format!("{}{}{}", s, gap, s);
    window_from_char_offset(&loop_text, offset_chars, max_cols)
}

fn take_prefix_cols(s: &str, max_cols: usize) -> String {
    let mut out = String::new();
    let mut cols = 0usize;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols + w > max_cols {
            break;
        }
        out.push(ch);
        cols += w;
        if cols >= max_cols {
            break;
        }
    }
    out
}

fn take_suffix_cols(s: &str, max_cols: usize) -> String {
    let mut out_rev: Vec<char> = Vec::new();
    let mut cols = 0usize;
    for ch in s.chars().rev() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols + w > max_cols {
            break;
        }
        out_rev.push(ch);
        cols += w;
        if cols >= max_cols {
            break;
        }
    }
    out_rev.into_iter().rev().collect()
}

fn window_from_char_offset(s: &str, start: usize, max_cols: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut cols = 0usize;
    let mut i = start % chars.len();
    while cols < max_cols {
        let ch = chars[i];
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols + w > max_cols {
            break;
        }
        out.push(ch);
        cols += w;
        i = (i + 1) % chars.len();
        if out.len() > chars.len() + 4 {
            break;
        }
    }
    out
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
