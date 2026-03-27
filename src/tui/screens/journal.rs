// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::{AppMode, AppState, JournalFilter};
use crate::persistence::event_journal::{EventCategory, EventJournalEntry, EventType};
use crate::tui::screen_context::ScreenContext;
use chrono::{DateTime, Local};
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::prelude::{
    Alignment, Constraint, Direction, Frame, Line, Modifier, Span, Style, Stylize,
};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalAction {
    ToNormal,
    FilterNext,
    FilterPrev,
    MoveUp,
    MoveDown,
}

fn map_key_to_journal_action(key_code: KeyCode, key_kind: KeyEventKind) -> Option<JournalAction> {
    if !matches!(key_kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    match key_code {
        KeyCode::Esc | KeyCode::Char('q') => Some(JournalAction::ToNormal),
        KeyCode::Tab => Some(JournalAction::FilterNext),
        KeyCode::BackTab => Some(JournalAction::FilterPrev),
        KeyCode::Up | KeyCode::Char('k') => Some(JournalAction::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Some(JournalAction::MoveDown),
        _ => None,
    }
}

pub fn handle_event(event: CrosstermEvent, app_state: &mut AppState) {
    if !matches!(app_state.mode, AppMode::Journal) {
        return;
    }

    let CrosstermEvent::Key(key) = event else {
        return;
    };

    let Some(action) = map_key_to_journal_action(key.code, key.kind) else {
        return;
    };

    match action {
        JournalAction::ToNormal => app_state.mode = AppMode::Normal,
        JournalAction::FilterNext => {
            app_state.ui.journal.filter = app_state.ui.journal.filter.next();
            app_state.ui.journal.selected_index = 0;
        }
        JournalAction::FilterPrev => {
            app_state.ui.journal.filter = app_state.ui.journal.filter.prev();
            app_state.ui.journal.selected_index = 0;
        }
        JournalAction::MoveUp => {
            app_state.ui.journal.selected_index =
                app_state.ui.journal.selected_index.saturating_sub(1);
        }
        JournalAction::MoveDown => {
            let len = filtered_entries(app_state).len();
            if len > 0 {
                app_state.ui.journal.selected_index =
                    (app_state.ui.journal.selected_index + 1).min(len - 1);
            }
        }
    }
}

fn entry_matches_filter(entry: &EventJournalEntry, filter: JournalFilter) -> bool {
    match filter {
        JournalFilter::All => true,
        JournalFilter::Queue => matches!(entry.category, EventCategory::Ingest),
        JournalFilter::Commands => matches!(entry.category, EventCategory::Control),
        JournalFilter::Health => matches!(entry.category, EventCategory::DataHealth),
    }
}

fn filtered_entries(app_state: &AppState) -> Vec<&EventJournalEntry> {
    app_state
        .event_journal_state
        .entries
        .iter()
        .rev()
        .filter(|entry| entry_matches_filter(entry, app_state.ui.journal.filter))
        .collect()
}

fn event_type_label(entry: &EventJournalEntry) -> &'static str {
    match entry.event_type {
        EventType::IngestQueued => "Queued",
        EventType::IngestAdded => "Added",
        EventType::IngestDuplicate => "Duplicate",
        EventType::IngestInvalid => "Invalid",
        EventType::IngestFailed => "Failed",
        EventType::TorrentCompleted => "Complete",
        EventType::DataUnavailable => "Missing",
        EventType::DataRecovered => "Found",
        EventType::ControlQueued => "Queued",
        EventType::ControlApplied => "Applied",
        EventType::ControlFailed => "Error",
    }
}

fn source_label(entry: &EventJournalEntry) -> String {
    entry
        .source_watch_folder
        .as_ref()
        .map(|path| compact_path_label(path, 2))
        .or_else(|| {
            entry
                .source_path
                .as_ref()
                .map(|path| compact_path_label(path, 2))
        })
        .unwrap_or_else(|| "-".to_string())
}

fn pretty_timestamp(ts_iso: &str) -> String {
    DateTime::parse_from_rfc3339(ts_iso)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%b %d %I:%M %p")
                .to_string()
        })
        .unwrap_or_else(|_| ts_iso.to_string())
}

fn compact_path_label(path: &Path, depth: usize) -> String {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    if components.is_empty() {
        return path.display().to_string();
    }

    if components.len() <= depth {
        return components.join("/");
    }

    format!(".../{}", components[components.len() - depth..].join("/"))
}

pub fn draw(f: &mut Frame, screen: &ScreenContext<'_>) {
    let app_state = screen.app.state;
    let ctx = screen.theme;
    let area = f.area();
    let layout = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    let popup = crate::tui::formatters::centered_rect(92, 84, layout[0]);
    f.render_widget(Clear, popup);

    let outer = Block::default()
        .title(Span::styled(
            " Event Journal ",
            ctx.apply(Style::default().fg(ctx.accent_sapphire()).bold()),
        ))
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
    let inner = outer.inner(popup);
    f.render_widget(outer, popup);

    let rows = ratatui::layout::Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(3),
    ])
    .split(inner);

    let filter_spans = [
        JournalFilter::All,
        JournalFilter::Queue,
        JournalFilter::Commands,
        JournalFilter::Health,
    ]
    .iter()
    .enumerate()
    .flat_map(|(idx, filter)| {
        let style = if *filter == app_state.ui.journal.filter {
            ctx.apply(
                Style::default()
                    .fg(ctx.state_selected())
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1))
        };
        let mut spans = vec![Span::styled(filter.label().to_string(), style)];
        if idx < 3 {
            spans.push(Span::styled(
                "  ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
            ));
        }
        spans
    })
    .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(Line::from(filter_spans)), rows[0]);

    let entries = filtered_entries(app_state);
    let status_line = format!("{} entries", entries.len());
    f.render_widget(
        Paragraph::new(status_line)
            .style(ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1))),
        rows[1],
    );

    let body_rows = entries
        .iter()
        .map(|entry| {
            Row::new(vec![
                Cell::from(pretty_timestamp(&entry.ts_iso)),
                Cell::from(event_type_label(entry)),
                Cell::from(
                    entry
                        .torrent_name
                        .clone()
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(source_label(entry)),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        body_rows,
        [
            Constraint::Length(17),
            Constraint::Length(10),
            Constraint::Percentage(47),
            Constraint::Percentage(26),
        ],
    )
    .header(
        Row::new(vec!["Time", "Event", "Torrent", "Source"]).style(
            ctx.apply(
                Style::default()
                    .fg(ctx.theme.semantic.subtext0)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
    )
    .row_highlight_style(
        ctx.apply(
            Style::default()
                .fg(ctx.theme.semantic.text)
                .bg(ctx.theme.semantic.surface0),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.surface2))),
    );

    let mut table_state = TableState::default();
    if !entries.is_empty() {
        table_state.select(Some(
            app_state.ui.journal.selected_index.min(entries.len() - 1),
        ));
    }
    f.render_stateful_widget(table, rows[2], &mut table_state);

    let details_text = entries
        .get(app_state.ui.journal.selected_index)
        .and_then(|entry| entry.message.clone())
        .unwrap_or_else(|| "No journal entries yet.".to_string());
    f.render_widget(
        Paragraph::new(details_text)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left)
            .style(ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1))),
        rows[3],
    );

    let footer_hint = Paragraph::new("[Tab] Filter  [Shift+Tab] Back  [j/k] Move  [q] Close")
        .alignment(Alignment::Center)
        .style(ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)));
    f.render_widget(footer_hint, layout[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::event_journal::{EventCategory, EventJournalState};
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    use std::path::Path;

    fn base_state() -> AppState {
        let mut state = AppState {
            mode: AppMode::Journal,
            ..Default::default()
        };
        state.event_journal_state = EventJournalState {
            next_id: 4,
            entries: vec![
                EventJournalEntry {
                    id: 1,
                    category: EventCategory::Ingest,
                    event_type: EventType::IngestAdded,
                    torrent_name: Some("Sample Alpha".to_string()),
                    ..Default::default()
                },
                EventJournalEntry {
                    id: 2,
                    category: EventCategory::Control,
                    event_type: EventType::ControlApplied,
                    torrent_name: Some("Sample Beta".to_string()),
                    ..Default::default()
                },
                EventJournalEntry {
                    id: 3,
                    category: EventCategory::DataHealth,
                    event_type: EventType::DataUnavailable,
                    torrent_name: Some("Sample Gamma".to_string()),
                    ..Default::default()
                },
            ],
        };
        state
    }

    #[test]
    fn tab_cycles_filters() {
        let mut app_state = base_state();

        handle_event(
            CrosstermEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            &mut app_state,
        );
        assert_eq!(app_state.ui.journal.filter, JournalFilter::Queue);

        handle_event(
            CrosstermEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            &mut app_state,
        );
        assert_eq!(app_state.ui.journal.filter, JournalFilter::Commands);
    }

    #[test]
    fn filter_selection_matches_requested_groups() {
        let mut app_state = base_state();

        app_state.ui.journal.filter = JournalFilter::Queue;
        let added = filtered_entries(&app_state);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].event_type, EventType::IngestAdded);

        app_state.ui.journal.filter = JournalFilter::Commands;
        let commands = filtered_entries(&app_state);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].event_type, EventType::ControlApplied);

        app_state.ui.journal.filter = JournalFilter::Health;
        let health = filtered_entries(&app_state);
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].event_type, EventType::DataUnavailable);
    }

    #[test]
    fn compact_path_label_keeps_tail_components() {
        let label = compact_path_label(Path::new("/alpha/beta/watch_files"), 2);
        assert_eq!(label, ".../beta/watch_files");
    }

    #[test]
    fn pretty_timestamp_formats_rfc3339_values() {
        let label = pretty_timestamp("2026-03-15T14:26:28Z");
        assert!(label.contains("Mar"));
    }
}
