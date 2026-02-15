// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::app::FileBrowserMode;
use crate::app::{App, AppMode, AppState, ConfigItem, SelectedHeader, TorrentControlState};
use crate::config::SortDirection;
use crate::theme::ThemeContext;
use crate::torrent_manager::ManagerCommand;
use crate::tui::events::{handle_navigation, handle_pasted_text};
use crate::tui::layout::calculate_layout;
use crate::tui::layout::compute_visible_peer_columns;
use crate::tui::layout::compute_visible_torrent_columns;
use crate::tui::layout::get_peer_columns;
use crate::tui::layout::get_torrent_columns;
use crate::tui::layout::LayoutContext;

#[cfg(windows)]
use clipboard::{ClipboardContext, ClipboardProvider};
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::prelude::{Alignment, Constraint, Direction, Frame, Line, Span, Style, Stylize};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};
use strum::IntoEnumIterator;
#[cfg(windows)]
use tracing::{event as tracing_event, Level};

pub fn draw_status_error_popup(f: &mut Frame, error_text: &str, ctx: &ThemeContext) {
    let popup_width_percent: u16 = 50;
    let popup_height: u16 = 8;
    let vertical_chunks = ratatui::layout::Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(popup_height),
        Constraint::Min(0),
    ])
    .split(f.area());
    let area = ratatui::layout::Layout::horizontal([
        Constraint::Percentage((100 - popup_width_percent) / 2),
        Constraint::Percentage(popup_width_percent),
        Constraint::Percentage((100 - popup_width_percent) / 2),
    ])
    .split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let text = vec![
        Line::from(Span::styled(
            "Error",
            ctx.apply(Style::default().fg(ctx.state_error()).bold()),
        )),
        Line::from(""),
        Line::from(Span::styled(
            error_text,
            ctx.apply(Style::default().fg(ctx.state_warning())),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "[Press Esc to dismiss]",
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
        )),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(ctx.state_error())));
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

pub fn draw_shutdown_screen(f: &mut Frame, app_state: &AppState, ctx: &ThemeContext) {
    const POPUP_WIDTH: u16 = 40;
    const POPUP_HEIGHT: u16 = 3;
    let area = f.area();
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);
    let vertical_chunks = ratatui::layout::Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(height),
        Constraint::Min(0),
    ])
    .split(area);
    let area = ratatui::layout::Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(width),
        Constraint::Min(0),
    ])
    .split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let container_block = Block::default()
        .title(Span::styled(
            " Exiting ",
            ctx.apply(Style::default().fg(ctx.accent_peach())),
        ))
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
    let inner_area = container_block.inner(area);
    f.render_widget(container_block, area);

    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1)])
        .split(inner_area);
    let progress_label = format!("{:.0}%", (app_state.shutdown_progress * 100.0).min(100.0));
    let progress_bar = Gauge::default()
        .ratio(app_state.shutdown_progress)
        .label(progress_label)
        .gauge_style(
            ctx.apply(
                Style::default()
                    .fg(ctx.state_selected())
                    .bg(ctx.theme.semantic.surface0),
            ),
        );
    f.render_widget(progress_bar, chunks[0]);
}

pub async fn handle_event(event: CrosstermEvent, app: &mut App) {
    match event {
        CrosstermEvent::Key(key) => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        app.app_state.system_error = None;
                    }
                    KeyCode::Char('/') => {
                        app.app_state.is_searching = true;
                        app.app_state.selected_torrent_index = 0;
                    }
                    KeyCode::Char('x') => {
                        app.app_state.anonymize_torrent_names = !app.app_state.anonymize_torrent_names;
                    }
                    KeyCode::Char('z') => {
                        app.app_state.mode = AppMode::PowerSaving;
                        return;
                    }
                    KeyCode::Char('Q') => {
                        app.app_state.should_quit = true;
                    }
                    KeyCode::Char('c') => {
                        let items = ConfigItem::iter().collect::<Vec<_>>();
                        app.app_state.mode = AppMode::Config {
                            settings_edit: Box::new(app.client_configs.clone()),
                            selected_index: 0,
                            items,
                            editing: None,
                        };
                    }
                    KeyCode::Char('t') => {
                        app.app_state.graph_mode = app.app_state.graph_mode.next();
                    }
                    KeyCode::Char('T') => {
                        app.app_state.graph_mode = app.app_state.graph_mode.prev();
                    }
                    KeyCode::Char('[') | KeyCode::Char('{') => {
                        app.app_state.data_rate = app.app_state.data_rate.next_slower();
                        let new_rate = app.app_state.data_rate.as_ms();

                        for manager_tx in app.torrent_manager_command_txs.values() {
                            let _ = manager_tx.try_send(ManagerCommand::SetDataRate(new_rate));
                        }
                    }
                    KeyCode::Char(']') | KeyCode::Char('}') => {
                        app.app_state.data_rate = app.app_state.data_rate.next_faster();
                        let new_rate = app.app_state.data_rate.as_ms();

                        for manager_tx in app.torrent_manager_command_txs.values() {
                            let _ = manager_tx.try_send(ManagerCommand::SetDataRate(new_rate));
                        }
                    }
                    KeyCode::Char('<') => {
                        let themes = crate::theme::ThemeName::sorted_for_ui();
                        let current_idx = themes
                            .iter()
                            .position(|&t| t == app.client_configs.ui_theme)
                            .unwrap_or(0);
                        let new_idx = if current_idx == 0 {
                            themes.len() - 1
                        } else {
                            current_idx - 1
                        };
                        app.client_configs.ui_theme = themes[new_idx];
                        app.app_state.theme = crate::theme::Theme::builtin(themes[new_idx]);
                        let _ = app
                            .app_command_tx
                            .try_send(AppCommand::UpdateConfig(app.client_configs.clone()));
                    }
                    KeyCode::Char('>') => {
                        let themes = crate::theme::ThemeName::sorted_for_ui();
                        let current_idx = themes
                            .iter()
                            .position(|&t| t == app.client_configs.ui_theme)
                            .unwrap_or(0);
                        let new_idx = (current_idx + 1) % themes.len();
                        app.client_configs.ui_theme = themes[new_idx];
                        app.app_state.theme = crate::theme::Theme::builtin(themes[new_idx]);
                        let _ = app
                            .app_command_tx
                            .try_send(AppCommand::UpdateConfig(app.client_configs.clone()));
                    }
                    KeyCode::Char('p') => {
                        if let Some(info_hash) = app
                            .app_state
                            .torrent_list_order
                            .get(app.app_state.selected_torrent_index)
                        {
                            if let (Some(torrent_display), Some(torrent_manager_command_tx)) = (
                                app.app_state.torrents.get_mut(info_hash),
                                app.torrent_manager_command_txs.get(info_hash),
                            ) {
                                let (new_state, command) =
                                    match torrent_display.latest_state.torrent_control_state {
                                        TorrentControlState::Running => (
                                            TorrentControlState::Paused,
                                            crate::torrent_manager::ManagerCommand::Pause,
                                        ),
                                        TorrentControlState::Paused => (
                                            TorrentControlState::Running,
                                            crate::torrent_manager::ManagerCommand::Resume,
                                        ),
                                        TorrentControlState::Deleting => return,
                                    };
                                torrent_display.latest_state.torrent_control_state = new_state;
                                let torrent_manager_command_tx_clone = torrent_manager_command_tx.clone();
                                tokio::spawn(async move {
                                    let _ = torrent_manager_command_tx_clone.send(command).await;
                                });
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        let initial_path = app.get_initial_source_path();
                        let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                            path: initial_path,
                            browser_mode: FileBrowserMode::File(vec![".torrent".to_string()]),
                            highlight_path: None,
                        });
                    }
                    KeyCode::Char('d') => {
                        if let Some(info_hash) = app
                            .app_state
                            .torrent_list_order
                            .get(app.app_state.selected_torrent_index)
                            .cloned()
                        {
                            app.app_state.mode = AppMode::DeleteConfirm {
                                info_hash,
                                with_files: false,
                            };
                        }
                    }
                    KeyCode::Char('D') => {
                        if let Some(info_hash) = app
                            .app_state
                            .torrent_list_order
                            .get(app.app_state.selected_torrent_index)
                            .cloned()
                        {
                            app.app_state.mode = AppMode::DeleteConfirm {
                                info_hash,
                                with_files: true,
                            };
                        }
                    }
                    KeyCode::Char('s') => {
                        let layout_ctx = LayoutContext::new(app.app_state.screen_area, &app.app_state, 35);
                        let layout_plan = calculate_layout(app.app_state.screen_area, &layout_ctx);
                        let (_, visible_torrent_columns) =
                            compute_visible_torrent_columns(&app.app_state, layout_plan.list.width);
                        let (_, visible_peer_columns) =
                            compute_visible_peer_columns(layout_plan.peers.width);
                        match app.app_state.selected_header {
                            SelectedHeader::Torrent(i) => {
                                let cols = get_torrent_columns();

                                if let Some(def) =
                                    visible_torrent_columns.get(i).and_then(|&real_idx| cols.get(real_idx))
                                {
                                    if let Some(column) = def.sort_enum {
                                        if app.app_state.torrent_sort.0 == column {
                                            app.app_state.torrent_sort.1 =
                                                if app.app_state.torrent_sort.1 == SortDirection::Ascending {
                                                    SortDirection::Descending
                                                } else {
                                                    SortDirection::Ascending
                                                };
                                        } else {
                                            app.app_state.torrent_sort.0 = column;
                                            app.app_state.torrent_sort.1 = SortDirection::Descending;
                                        }
                                        app.sort_and_filter_torrent_list();
                                    }
                                }
                            }
                            SelectedHeader::Peer(i) => {
                                let cols = get_peer_columns();

                                if let Some(def) =
                                    visible_peer_columns.get(i).and_then(|&real_idx| cols.get(real_idx))
                                {
                                    if let Some(column) = def.sort_enum {
                                        if app.app_state.peer_sort.0 == column {
                                            app.app_state.peer_sort.1 =
                                                if app.app_state.peer_sort.1 == SortDirection::Ascending {
                                                    SortDirection::Descending
                                                } else {
                                                    SortDirection::Ascending
                                                };
                                        } else {
                                            app.app_state.peer_sort.0 = column;
                                            app.app_state.peer_sort.1 = SortDirection::Descending;
                                        }
                                    }
                                }
                            }
                        };
                    }
                    KeyCode::Up
                    | KeyCode::Char('k')
                    | KeyCode::Down
                    | KeyCode::Char('j')
                    | KeyCode::Left
                    | KeyCode::Char('h')
                    | KeyCode::Right
                    | KeyCode::Char('l') => {
                        handle_navigation(&mut app.app_state, key.code);
                    }
                    #[cfg(windows)]
                    KeyCode::Char('v') => match ClipboardContext::new() {
                        Ok(mut ctx) => match ctx.get_contents() {
                            Ok(text) => {
                                handle_pasted_text(app, text.trim()).await;
                            }
                            Err(e) => {
                                tracing_event!(Level::ERROR, "Clipboard read error: {}", e);
                                app.app_state.system_error = Some(format!("Clipboard read error: {}", e));
                            }
                        },
                        Err(e) => {
                            tracing_event!(Level::ERROR, "Clipboard context error: {}", e);
                            app.app_state.system_error =
                                Some(format!("Clipboard initialization error: {}", e));
                        }
                    },
                    _ => {}
                }
            }
        }
        #[cfg(not(windows))]
        CrosstermEvent::Paste(pasted_text) => {
            handle_pasted_text(app, pasted_text.trim()).await;
        }
        _ => {}
    }
}
