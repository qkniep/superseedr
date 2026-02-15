// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::sort_and_filter_torrent_list_state;
use crate::app::torrent_completion_percent;
use crate::app::AppCommand;
use crate::app::BrowserPane;
use crate::app::FileBrowserMode;
use crate::app::GraphDisplayMode;
use crate::app::PeerInfo;
use crate::app::{App, AppMode, AppState, ConfigItem, SelectedHeader, TorrentControlState};
use crate::config::Settings;
use crate::config::SortDirection;
use crate::theme::ThemeContext;
use crate::torrent_manager::ManagerCommand;
use crate::tui::formatters::{
    calculate_nice_upper_bound, format_bytes, format_countdown, format_duration, format_iops,
    format_latency, format_limit_bps, format_limit_delta, format_memory, format_permits_spans,
    format_speed, format_time, generate_x_axis_labels, ip_to_color, parse_peer_id, sanitize_text,
    speed_to_style, truncate_with_ellipsis,
};
use crate::tui::layout::common::compute_smart_table_layout;
use crate::tui::layout::common::compute_visible_peer_columns;
use crate::tui::layout::common::compute_visible_torrent_columns;
use crate::tui::layout::common::get_peer_columns;
use crate::tui::layout::common::get_torrent_columns;
use crate::tui::layout::common::ColumnId;
use crate::tui::layout::common::{PeerColumnId, SmartCol};
use crate::tui::layout::normal::calculate_layout;
use crate::tui::layout::normal::LayoutContext;
use crate::tui::layout::normal::LayoutPlan;
use crate::tui::layout::normal::DEFAULT_SIDEBAR_PERCENT;
use crate::tui::screen_context::ScreenContext;
use crate::tui::tree::TreeViewState;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use clipboard::{ClipboardContext, ClipboardProvider};
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::layout::Layout;
use ratatui::prelude::{
    symbols, Alignment, Color, Constraint, Direction, Frame, Line, Modifier, Rect, Span, Style,
    Stylize,
};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Gauge, LineGauge, Padding, Paragraph, Row, Table, TableState, Wrap,
};
use strum::IntoEnumIterator;
use throbber_widgets_tui::Throbber;
use tracing::{event as tracing_event, Level};

static APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SECONDS_HISTORY_MAX: usize = 3600;
const MINUTES_HISTORY_MAX: usize = 48 * 60;

#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    ClearSystemError,
    StartSearch,
    Navigate(KeyCode),
    ToggleAnonymizeNames,
    EnterPowerSaving,
    RequestQuit,
    GraphNext,
    GraphPrev,
    OpenAddTorrentBrowser,
    OpenDeleteConfirm {
        with_files: bool,
    },
    OpenConfig,
    DataRateSlower,
    DataRateFaster,
    ThemePrev,
    ThemeNext,
    TogglePauseSelected,
    SortBySelectedColumn,
    OpenHelp,
    PasteText(String),
    #[cfg(windows)]
    PasteFromClipboard,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEffect {
    OpenAddTorrentFileBrowser,
    OpenConfigScreen,
    BroadcastManagerDataRate(u64),
    ApplyThemePrev,
    ApplyThemeNext,
    SendPause(Vec<u8>),
    SendResume(Vec<u8>),
    OpenHelpScreen,
    HandlePastedText(String),
    #[cfg(windows)]
    ReadClipboardAndPaste,
}

#[derive(Default)]
pub struct ReduceResult {
    pub redraw: bool,
    pub effects: Vec<UiEffect>,
}

pub fn reduce_ui_action(app_state: &mut AppState, action: UiAction) -> ReduceResult {
    match action {
        UiAction::ClearSystemError => {
            app_state.system_error = None;
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::StartSearch => {
            app_state.ui.is_searching = true;
            app_state.ui.selected_torrent_index = 0;
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::Navigate(key_code) => {
            handle_navigation(app_state, key_code);
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::ToggleAnonymizeNames => {
            app_state.anonymize_torrent_names = !app_state.anonymize_torrent_names;
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::EnterPowerSaving => {
            app_state.mode = AppMode::PowerSaving;
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::RequestQuit => {
            app_state.should_quit = true;
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::GraphNext => {
            app_state.graph_mode = app_state.graph_mode.next();
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::GraphPrev => {
            app_state.graph_mode = app_state.graph_mode.prev();
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::OpenAddTorrentBrowser => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::OpenAddTorrentFileBrowser],
        },
        UiAction::OpenDeleteConfirm { with_files } => {
            if let Some(info_hash) = app_state
                .torrent_list_order
                .get(app_state.ui.selected_torrent_index)
                .cloned()
            {
                app_state.ui.delete_confirm.info_hash = info_hash;
                app_state.ui.delete_confirm.with_files = with_files;
                app_state.mode = AppMode::DeleteConfirm;
            }
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::OpenConfig => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::OpenConfigScreen],
        },
        UiAction::DataRateSlower => {
            app_state.data_rate = app_state.data_rate.next_slower();
            ReduceResult {
                redraw: true,
                effects: vec![UiEffect::BroadcastManagerDataRate(
                    app_state.data_rate.as_ms(),
                )],
            }
        }
        UiAction::DataRateFaster => {
            app_state.data_rate = app_state.data_rate.next_faster();
            ReduceResult {
                redraw: true,
                effects: vec![UiEffect::BroadcastManagerDataRate(
                    app_state.data_rate.as_ms(),
                )],
            }
        }
        UiAction::ThemePrev => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::ApplyThemePrev],
        },
        UiAction::ThemeNext => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::ApplyThemeNext],
        },
        UiAction::TogglePauseSelected => {
            let selected_hash = app_state
                .torrent_list_order
                .get(app_state.ui.selected_torrent_index)
                .cloned();
            if let Some(info_hash) = selected_hash {
                if let Some(torrent_display) = app_state.torrents.get_mut(&info_hash) {
                    match torrent_display.latest_state.torrent_control_state {
                        TorrentControlState::Running => {
                            torrent_display.latest_state.torrent_control_state =
                                TorrentControlState::Paused;
                            return ReduceResult {
                                redraw: true,
                                effects: vec![UiEffect::SendPause(info_hash)],
                            };
                        }
                        TorrentControlState::Paused => {
                            torrent_display.latest_state.torrent_control_state =
                                TorrentControlState::Running;
                            return ReduceResult {
                                redraw: true,
                                effects: vec![UiEffect::SendResume(info_hash)],
                            };
                        }
                        TorrentControlState::Deleting => {}
                    }
                }
            }
            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::SortBySelectedColumn => {
            let layout_ctx =
                LayoutContext::new(app_state.screen_area, app_state, DEFAULT_SIDEBAR_PERCENT);
            let layout_plan = calculate_layout(app_state.screen_area, &layout_ctx);
            let (_, visible_torrent_columns) =
                compute_visible_torrent_columns(app_state, layout_plan.list.width);
            let (_, visible_peer_columns) = compute_visible_peer_columns(layout_plan.peers.width);

            match app_state.ui.selected_header {
                SelectedHeader::Torrent(i) => {
                    let cols = get_torrent_columns();
                    if let Some(def) = visible_torrent_columns
                        .get(i)
                        .and_then(|&real_idx| cols.get(real_idx))
                    {
                        if let Some(column) = def.sort_enum {
                            if app_state.torrent_sort.0 == column {
                                app_state.torrent_sort.1 =
                                    if app_state.torrent_sort.1 == SortDirection::Ascending {
                                        SortDirection::Descending
                                    } else {
                                        SortDirection::Ascending
                                    };
                            } else {
                                app_state.torrent_sort.0 = column;
                                app_state.torrent_sort.1 = SortDirection::Descending;
                            }
                            sort_and_filter_torrent_list_state(app_state);
                        }
                    }
                }
                SelectedHeader::Peer(i) => {
                    let cols = get_peer_columns();
                    if let Some(def) = visible_peer_columns
                        .get(i)
                        .and_then(|&real_idx| cols.get(real_idx))
                    {
                        if let Some(column) = def.sort_enum {
                            if app_state.peer_sort.0 == column {
                                app_state.peer_sort.1 =
                                    if app_state.peer_sort.1 == SortDirection::Ascending {
                                        SortDirection::Descending
                                    } else {
                                        SortDirection::Ascending
                                    };
                            } else {
                                app_state.peer_sort.0 = column;
                                app_state.peer_sort.1 = SortDirection::Descending;
                            }
                        }
                    }
                }
            };

            ReduceResult {
                redraw: true,
                effects: Vec::new(),
            }
        }
        UiAction::OpenHelp => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::OpenHelpScreen],
        },
        UiAction::PasteText(text) => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::HandlePastedText(text)],
        },
        #[cfg(windows)]
        UiAction::PasteFromClipboard => ReduceResult {
            redraw: true,
            effects: vec![UiEffect::ReadClipboardAndPaste],
        },
    }
}

fn map_key_to_ui_action(key_code: KeyCode) -> Option<UiAction> {
    match key_code {
        KeyCode::Esc => Some(UiAction::ClearSystemError),
        KeyCode::Char('/') => Some(UiAction::StartSearch),
        KeyCode::Char('x') => Some(UiAction::ToggleAnonymizeNames),
        KeyCode::Char('z') => Some(UiAction::EnterPowerSaving),
        KeyCode::Char('Q') => Some(UiAction::RequestQuit),
        KeyCode::Char('t') => Some(UiAction::GraphNext),
        KeyCode::Char('T') => Some(UiAction::GraphPrev),
        KeyCode::Char('a') => Some(UiAction::OpenAddTorrentBrowser),
        KeyCode::Char('d') => Some(UiAction::OpenDeleteConfirm { with_files: false }),
        KeyCode::Char('D') => Some(UiAction::OpenDeleteConfirm { with_files: true }),
        KeyCode::Char('c') => Some(UiAction::OpenConfig),
        KeyCode::Char('m') => Some(UiAction::OpenHelp),
        #[cfg(windows)]
        KeyCode::Char('v') => Some(UiAction::PasteFromClipboard),
        KeyCode::Char('[') | KeyCode::Char('{') => Some(UiAction::DataRateSlower),
        KeyCode::Char(']') | KeyCode::Char('}') => Some(UiAction::DataRateFaster),
        KeyCode::Char('<') => Some(UiAction::ThemePrev),
        KeyCode::Char('>') => Some(UiAction::ThemeNext),
        KeyCode::Char('p') => Some(UiAction::TogglePauseSelected),
        KeyCode::Char('s') => Some(UiAction::SortBySelectedColumn),
        KeyCode::Up
        | KeyCode::Char('k')
        | KeyCode::Down
        | KeyCode::Char('j')
        | KeyCode::Left
        | KeyCode::Char('h')
        | KeyCode::Right
        | KeyCode::Char('l') => Some(UiAction::Navigate(key_code)),
        _ => None,
    }
}

pub fn draw(f: &mut Frame, screen: &ScreenContext<'_>, plan: &LayoutPlan) {
    let app_state = screen.app.state;
    let settings = screen.settings;
    let ctx = screen.theme;

    draw_torrent_list(f, app_state, plan.list, ctx);
    draw_footer(f, app_state, settings, plan.footer, ctx);
    draw_details_panel(f, app_state, plan.details, ctx);
    draw_peers_table(f, app_state, plan.peers, ctx);

    if let Some(r) = plan.chart {
        draw_network_chart(f, app_state, r, ctx);
    }
    if let Some(r) = plan.sparklines {
        draw_torrent_sparklines(f, app_state, r, ctx);
    }
    if let Some(r) = plan.peer_stream {
        draw_peer_stream(f, app_state, r, ctx);
    }
    if let Some(r) = plan.block_stream {
        draw_vertical_block_stream(f, app_state, r, ctx);
    }
    if let Some(r) = plan.stats {
        draw_stats_panel(f, app_state, settings, r, ctx);
    }
}

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

pub(crate) fn truncate_theme_label_preserving_fx(
    theme_name: &str,
    fx_enabled: bool,
    max_len: usize,
) -> String {
    if max_len == 0 {
        return String::new();
    }

    if !fx_enabled {
        return truncate_with_ellipsis(theme_name, max_len);
    }

    let suffix = "[FX]";
    let suffix_len = suffix.chars().count();
    let full = format!("{theme_name} {suffix}");
    if full.chars().count() <= max_len {
        return full;
    }

    if max_len <= 3 {
        return ".".repeat(max_len);
    }

    if max_len <= suffix_len + 3 {
        return truncate_with_ellipsis(&full, max_len);
    }

    let name_len = max_len.saturating_sub(3 + suffix_len);
    let name_prefix: String = theme_name.chars().take(name_len).collect();
    format!("{name_prefix}...{suffix}")
}

pub(crate) fn compute_footer_left_width(footer_width: u16, is_update: bool) -> u16 {
    let min_left = if is_update { 68u16 } else { 48u16 };
    let max_left = if is_update { 110u16 } else { 90u16 };
    let right_status = 21u16;
    let min_commands = 18u16;
    let reserved = right_status + min_commands;

    let available_for_left = footer_width.saturating_sub(reserved);
    available_for_left.clamp(min_left, max_left)
}

fn estimate_footer_left_content_width(app_state: &AppState, ctx: &ThemeContext) -> u16 {
    let fx_enabled = ctx.theme.effects.enabled();
    let theme_label = if fx_enabled {
        format!("{} [FX]", ctx.theme.name)
    } else {
        ctx.theme.name.to_string()
    };

    let content = if let Some(new_version) = &app_state.update_available {
        format!(
            "UPDATE AVAILABLE: v{} -> v{} | {} | {}",
            APP_VERSION,
            new_version,
            app_state.data_rate.to_string(),
            theme_label
        )
    } else {
        #[cfg(all(feature = "dht", feature = "pex"))]
        {
            format!(
                "superseedr v{} | {} | {}",
                APP_VERSION,
                app_state.data_rate.to_string(),
                theme_label
            )
        }
        #[cfg(not(all(feature = "dht", feature = "pex")))]
        {
            format!(
                "superseedr [PRIVATE] v{} | {} | {}",
                APP_VERSION,
                app_state.data_rate.to_string(),
                theme_label
            )
        }
    };

    (content.chars().count() as u16).saturating_add(2)
}

fn footer_command_len(key: &str, suffix: &str) -> usize {
    key.chars().count() + suffix.chars().count()
}

fn try_push_footer_command(
    spans: &mut Vec<Span<'static>>,
    used_width: &mut usize,
    max_width: usize,
    key: &'static str,
    suffix: &'static str,
    key_style: Style,
) -> bool {
    let item_width = footer_command_len(key, suffix);
    let separator_width = if *used_width == 0 { 0 } else { 3 };
    if *used_width + separator_width + item_width > max_width {
        return false;
    }

    if separator_width > 0 {
        spans.push(Span::raw(" | "));
    }
    spans.push(Span::styled(key, key_style));
    spans.push(Span::raw(suffix));
    *used_width += separator_width + item_width;
    true
}

pub fn draw_footer(
    f: &mut Frame,
    app_state: &AppState,
    settings: &Settings,
    footer_chunk: ratatui::layout::Rect,
    ctx: &ThemeContext,
) {
    let show_branding = footer_chunk.width >= 80;

    let is_update = app_state.update_available.is_some();
    let (left_constraint, right_constraint) = if show_branding {
        let min_left = if is_update { 52u16 } else { 40u16 };
        let min_commands = 18u16;
        let desired_left = compute_footer_left_width(footer_chunk.width, is_update);
        let content_left = estimate_footer_left_content_width(app_state, ctx);
        let left_target = desired_left.min(content_left.max(min_left));
        let symmetric_left_cap = footer_chunk.width.saturating_sub(min_commands) / 2;

        if symmetric_left_cap >= min_left {
            let symmetric_left = left_target.min(symmetric_left_cap);
            (
                Constraint::Length(symmetric_left),
                Constraint::Length(symmetric_left),
            )
        } else {
            (Constraint::Length(left_target), Constraint::Length(21))
        }
    } else {
        (Constraint::Length(0), Constraint::Length(21))
    };

    let footer_layout = ratatui::layout::Layout::default()
        .direction(Direction::Horizontal)
        .constraints([left_constraint, Constraint::Min(0), right_constraint])
        .split(footer_chunk);

    let client_id_chunk = footer_layout[0];
    let commands_chunk = footer_layout[1];
    let status_chunk = footer_layout[2];

    if show_branding {
        let current_dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
        let current_ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);
        let fx_enabled = ctx.theme.effects.enabled();
        let theme_name = ctx.theme.name.to_string();
        let fit_theme_label = |prefix: &str| -> String {
            let max_theme_width =
                (client_id_chunk.width as usize).saturating_sub(prefix.chars().count());
            if max_theme_width == 0 {
                String::new()
            } else if max_theme_width <= 3 {
                ".".repeat(max_theme_width)
            } else {
                truncate_theme_label_preserving_fx(&theme_name, fx_enabled, max_theme_width)
            }
        };

        let client_display_line = if let Some(new_version) = &app_state.update_available {
            let theme_display = fit_theme_label(&format!(
                "UPDATE AVAILABLE: v{} -> v{} | {} | ",
                APP_VERSION,
                new_version,
                app_state.data_rate.to_string()
            ));
            Line::from(vec![
                Span::styled(
                    "UPDATE AVAILABLE: ",
                    ctx.apply(Style::default().fg(ctx.state_warning()).bold()),
                ),
                Span::styled(
                    format!("v{}", APP_VERSION),
                    Style::default()
                        .fg(ctx.theme.semantic.surface2)
                        .add_modifier(ratatui::prelude::Modifier::CROSSED_OUT),
                ),
                Span::styled(
                    " \u{2192} ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                ),
                Span::styled(
                    format!("v{}", new_version),
                    ctx.apply(Style::default().fg(ctx.state_success()).bold()),
                ),
                Span::styled(
                    " | ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                ),
                Span::styled(
                    app_state.data_rate.to_string(),
                    ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
                ),
                Span::styled(
                    " | ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                ),
                Span::styled(
                    theme_display,
                    ctx.apply(Style::default().fg(ctx.state_selected())),
                ),
            ])
        } else {
            #[cfg(all(feature = "dht", feature = "pex"))]
            {
                let theme_display = fit_theme_label(&format!(
                    "superseedr v{} | {} | ",
                    APP_VERSION,
                    app_state.data_rate.to_string()
                ));
                Line::from(vec![
                    Span::styled(
                        "super",
                        ctx.apply(
                            speed_to_style(ctx, current_dl_speed)
                                .add_modifier(ratatui::prelude::Modifier::BOLD),
                        ),
                    ),
                    Span::styled(
                        "seedr",
                        ctx.apply(
                            speed_to_style(ctx, current_ul_speed)
                                .add_modifier(ratatui::prelude::Modifier::BOLD),
                        ),
                    ),
                    Span::styled(
                        format!(" v{}", APP_VERSION),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
                    ),
                    Span::styled(
                        " | ",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                    ),
                    Span::styled(
                        app_state.data_rate.to_string(),
                        ctx.apply(Style::default().fg(ctx.state_warning()).bold()),
                    ),
                    Span::styled(
                        " | ",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                    ),
                    Span::styled(
                        theme_display,
                        ctx.apply(Style::default().fg(ctx.state_selected())),
                    ),
                ])
            }
            #[cfg(not(all(feature = "dht", feature = "pex")))]
            {
                let theme_display = fit_theme_label(&format!(
                    "superseedr [PRIVATE] v{} | {} | ",
                    APP_VERSION,
                    app_state.data_rate.to_string()
                ));
                Line::from(vec![
                    Span::styled(
                        "superseedr",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                    )
                    .add_modifier(ratatui::prelude::Modifier::CROSSED_OUT),
                    Span::styled(
                        " [PRIVATE]",
                        Style::default()
                            .fg(ctx.state_error())
                            .add_modifier(ratatui::prelude::Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" v{}", APP_VERSION),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
                    ),
                    Span::styled(
                        " | ",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                    ),
                    Span::styled(
                        app_state.data_rate.to_string(),
                        ctx.apply(Style::default().fg(ctx.state_warning()).bold()),
                    ),
                    Span::styled(
                        " | ",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
                    ),
                    Span::styled(
                        theme_display,
                        ctx.apply(Style::default().fg(ctx.state_selected())),
                    ),
                ])
            }
        };

        let client_id_paragraph = Paragraph::new(client_display_line).alignment(Alignment::Left);
        f.render_widget(client_id_paragraph, client_id_chunk);
    }

    let max_width = commands_chunk.width as usize;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used_width = 0usize;

    let manual_key = "[m]";
    let manual_suffix = if app_state.system_warning.is_some() {
        "anual (warning)"
    } else {
        "anual"
    };
    let manual_min_width = footer_command_len(manual_key, "");

    let mut push_if_fits = |key: &'static str, suffix: &'static str, key_style: Style| {
        let separator_width = if used_width == 0 { 0 } else { 3 };
        let candidate_width = footer_command_len(key, suffix);
        let required_for_manual = if used_width + separator_width + candidate_width == 0 {
            manual_min_width
        } else {
            3 + manual_min_width
        };
        if used_width + separator_width + candidate_width + required_for_manual <= max_width {
            let _ = try_push_footer_command(
                &mut spans,
                &mut used_width,
                max_width,
                key,
                suffix,
                key_style,
            );
        }
    };

    push_if_fits(
        "Arrows",
        " nav",
        ctx.apply(Style::default().fg(ctx.state_info())),
    );
    push_if_fits(
        "[Q]",
        "uit",
        ctx.apply(Style::default().fg(ctx.state_error())),
    );
    push_if_fits(
        "[v]",
        "paste",
        ctx.apply(Style::default().fg(ctx.accent_teal())),
    );
    push_if_fits(
        "[p]",
        "ause",
        ctx.apply(Style::default().fg(ctx.state_success())),
    );
    push_if_fits(
        "[a]",
        "dd",
        ctx.apply(Style::default().fg(ctx.state_success())),
    );
    push_if_fits(
        "[d]",
        "elete",
        ctx.apply(Style::default().fg(ctx.state_warning())),
    );
    push_if_fits(
        "[s]",
        "ort",
        ctx.apply(Style::default().fg(ctx.state_selected())),
    );
    push_if_fits(
        "[t]",
        "ime",
        ctx.apply(Style::default().fg(ctx.accent_sapphire())),
    );
    push_if_fits(
        "[<]theme[>]",
        "",
        ctx.apply(Style::default().fg(ctx.state_selected())),
    );
    push_if_fits(
        "[/]",
        "search",
        ctx.apply(Style::default().fg(ctx.state_warning())),
    );
    push_if_fits(
        "[c]",
        "onfig",
        ctx.apply(Style::default().fg(ctx.state_complete())),
    );
    push_if_fits(
        "[d]",
        "elete",
        ctx.apply(Style::default().fg(ctx.state_error())),
    );
    push_if_fits(
        "[x]",
        "anon",
        ctx.apply(Style::default().fg(ctx.accent_sapphire())),
    );
    push_if_fits(
        "[z]",
        "power",
        ctx.apply(Style::default().fg(ctx.state_warning())),
    );
    push_if_fits(
        "[T]",
        "time++",
        ctx.apply(Style::default().fg(ctx.accent_sapphire())),
    );
    push_if_fits(
        "[[]",
        "slower",
        ctx.apply(Style::default().fg(ctx.state_info())),
    );
    push_if_fits(
        "[]]",
        "faster",
        ctx.apply(Style::default().fg(ctx.state_success())),
    );

    if !try_push_footer_command(
        &mut spans,
        &mut used_width,
        max_width,
        manual_key,
        manual_suffix,
        ctx.apply(Style::default().fg(ctx.accent_teal())),
    ) {
        let _ = try_push_footer_command(
            &mut spans,
            &mut used_width,
            max_width,
            manual_key,
            "anual",
            ctx.apply(Style::default().fg(ctx.accent_teal())),
        );
    }
    if !spans.iter().any(|s| matches!(s.content.as_ref(), "[m]")) {
        let _ = try_push_footer_command(
            &mut spans,
            &mut used_width,
            max_width,
            manual_key,
            "",
            ctx.apply(Style::default().fg(ctx.accent_teal())),
        );
    }

    let footer_paragraph = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .style(ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)));
    f.render_widget(footer_paragraph, commands_chunk);

    let port_style = if app_state.externally_accessable_port {
        ctx.apply(Style::default().fg(ctx.state_success()))
    } else {
        ctx.apply(Style::default().fg(ctx.state_error()))
    };
    let port_text = if app_state.externally_accessable_port {
        "Open"
    } else {
        "Closed"
    };

    let footer_status = Line::from(vec![
        Span::raw("Port: "),
        Span::styled(settings.client_port.to_string(), port_style),
        Span::raw(" ["),
        Span::styled(port_text, port_style),
        Span::raw("]"),
    ])
    .alignment(Alignment::Right);

    let status_paragraph = Paragraph::new(footer_status)
        .style(ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)));
    f.render_widget(status_paragraph, status_chunk);
}

pub fn draw_torrent_list(f: &mut Frame, app_state: &AppState, area: Rect, ctx: &ThemeContext) {
    let mut table_state = TableState::default();
    if matches!(app_state.ui.selected_header, SelectedHeader::Torrent(_)) {
        table_state.select(Some(app_state.ui.selected_torrent_index));
    }

    let all_cols = get_torrent_columns();
    let (constraints, visible_indices) = compute_visible_torrent_columns(app_state, area.width);

    let (sort_col, sort_dir) = app_state.torrent_sort;
    let header_cells: Vec<Cell> = visible_indices
        .iter()
        .enumerate()
        .map(|(visual_idx, &real_idx)| {
            let def = &all_cols[real_idx];
            let is_selected = app_state.ui.selected_header == SelectedHeader::Torrent(visual_idx);
            let is_sorting = def.sort_enum == Some(sort_col);

            let mut style = ctx.apply(Style::default().fg(ctx.state_warning()));
            if is_sorting {
                style = style.fg(ctx.state_selected());
            }
            style = ctx.apply(style);

            let mut spans = vec![];
            let mut text_span = Span::styled(def.header, style);
            if is_selected {
                text_span = text_span.underlined().bold();
            }
            spans.push(text_span);

            if is_sorting {
                let arrow = if sort_dir == SortDirection::Ascending {
                    " ▲"
                } else {
                    " ▼"
                };
                spans.push(Span::styled(arrow, style));
            }
            Cell::from(Line::from(spans))
        })
        .collect();
    let header = Row::new(header_cells).height(1);

    let rows =
        app_state
            .torrent_list_order
            .iter()
            .enumerate()
            .map(|(i, info_hash)| match app_state.torrents.get(info_hash) {
                Some(torrent) => {
                    let state = &torrent.latest_state;
                    let is_selected = i == app_state.ui.selected_torrent_index;

                    let mut row_style = match state.torrent_control_state {
                        TorrentControlState::Running => {
                            ctx.apply(Style::default().fg(ctx.theme.semantic.text))
                        }
                        TorrentControlState::Paused => {
                            ctx.apply(Style::default().fg(ctx.theme.semantic.surface1))
                        }
                        TorrentControlState::Deleting => {
                            ctx.apply(Style::default().fg(ctx.state_error()))
                        }
                    };
                    row_style = ctx.apply(row_style);

                    if is_selected {
                        let is_safe_ascii = state.torrent_name.is_ascii();
                        if is_safe_ascii {
                            row_style = row_style.add_modifier(Modifier::BOLD);
                        }
                    }

                    let cells: Vec<Cell> = visible_indices
                        .iter()
                        .map(|&real_idx| {
                            let def = &all_cols[real_idx];
                            match def.id {
                                ColumnId::Status => {
                                    let display_pct = torrent_completion_percent(state);
                                    Cell::from(format!("{:.1}%", display_pct))
                                }
                                ColumnId::Name => {
                                    let name = if app_state.anonymize_torrent_names {
                                        format!("Torrent {}", i + 1)
                                    } else {
                                        sanitize_text(&state.torrent_name)
                                    };
                                    let mut c = Cell::from(name);
                                    if is_selected {
                                        let s = ctx.apply(
                                            ctx.apply(Style::default().fg(ctx.state_warning())),
                                        );
                                        c = c.style(s);
                                    }
                                    c
                                }
                                ColumnId::DownSpeed => {
                                    Cell::from(format_speed(torrent.smoothed_download_speed_bps))
                                        .style(ctx.apply(speed_to_style(
                                            ctx,
                                            torrent.smoothed_download_speed_bps,
                                        )))
                                }
                                ColumnId::UpSpeed => {
                                    Cell::from(format_speed(torrent.smoothed_upload_speed_bps))
                                        .style(ctx.apply(speed_to_style(
                                            ctx,
                                            torrent.smoothed_upload_speed_bps,
                                        )))
                                }
                            }
                        })
                        .collect();

                    Row::new(cells).style(row_style)
                }
                None => Row::new(vec![Cell::from("Error retrieving data")]),
            });

    let border_style = if matches!(app_state.ui.selected_header, SelectedHeader::Torrent(_)) {
        ctx.apply(Style::default().fg(ctx.state_selected()))
    } else {
        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2))
    };

    let mut title_spans = Vec::new();
    if app_state.ui.is_searching {
        title_spans.push(Span::raw("Search: /"));
        title_spans.push(Span::styled(
            &app_state.ui.search_query,
            ctx.apply(Style::default().fg(ctx.state_warning())),
        ));
    } else if !app_state.ui.search_query.is_empty() {
        title_spans.push(Span::styled(
            format!("[{}] ", app_state.ui.search_query),
            ctx.apply(
                Style::default()
                    .fg(ctx.theme.semantic.subtext1)
                    .add_modifier(Modifier::ITALIC),
            ),
        ));
    }

    if !app_state.ui.is_searching {
        if let Some(info_hash) = app_state
            .torrent_list_order
            .get(app_state.ui.selected_torrent_index)
        {
            if let Some(torrent) = app_state.torrents.get(info_hash) {
                let path_cow;
                let text_to_show = if app_state.anonymize_torrent_names {
                    "/path/to/torrent/file"
                } else {
                    path_cow = torrent
                        .latest_state
                        .download_path
                        .as_ref()
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_else(|| std::borrow::Cow::Borrowed("Unknown path"));
                    &sanitize_text(&path_cow)
                };

                let avail_width = area.width.saturating_sub(10) as usize;
                let display_name = truncate_with_ellipsis(text_to_show, avail_width);

                title_spans.push(Span::styled(
                    display_name,
                    ctx.apply(Style::default().fg(ctx.state_warning())),
                ));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Line::from(title_spans));

    let inner_area = block.inner(area);
    let table = Table::new(rows, constraints).header(header).block(block);
    f.render_stateful_widget(table, area, &mut table_state);

    if app_state.torrent_list_order.is_empty() {
        let empty_msg = vec![
            Line::from(Span::styled(
                "No Torrents",
                ctx.apply(
                    Style::default()
                        .fg(ctx.theme.semantic.surface2)
                        .add_modifier(Modifier::BOLD),
                ),
            )),
            Line::from(Span::styled(
                "Press [a] to add a file or [v] to paste a magnet link",
                ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
            )),
        ];

        let center_y = inner_area.y + (inner_area.height / 2).saturating_sub(1);
        let text_area = Rect::new(inner_area.x, center_y, inner_area.width, 2);

        f.render_widget(
            Paragraph::new(empty_msg).alignment(Alignment::Center),
            text_area,
        );
    }
}

pub fn draw_details_panel(
    f: &mut Frame,
    app_state: &AppState,
    details_text_chunk: Rect,
    ctx: &ThemeContext,
) {
    let details_block = Block::default()
        .title(Span::styled(
            "Details",
            ctx.apply(Style::default().fg(ctx.state_selected())),
        ))
        .borders(Borders::ALL)
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
    let details_inner_chunk = details_block.inner(details_text_chunk);
    f.render_widget(details_block, details_text_chunk);

    let detail_rows = ratatui::layout::Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(details_inner_chunk);

    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
        .and_then(|h| app_state.torrents.get(h));

    if let Some(torrent) = selected_torrent {
        let state = &torrent.latest_state;

        let progress_chunks =
            ratatui::layout::Layout::horizontal([Constraint::Length(11), Constraint::Min(0)])
                .split(detail_rows[0]);

        f.render_widget(Paragraph::new("Progress: "), progress_chunks[0]);

        let (progress_ratio, progress_label_text) = if state.number_of_pieces_total > 0 {
            if state.torrent_control_state != TorrentControlState::Running
                || state.activity_message.contains("Seeding")
                || state.activity_message.contains("Finished")
            {
                (1.0, "100.0%".to_string())
            } else {
                let ratio =
                    state.number_of_pieces_completed as f64 / state.number_of_pieces_total as f64;
                (ratio, format!("{:.1}%", ratio * 100.0))
            }
        } else {
            (0.0, "0.0%".to_string())
        };
        let custom_line_set = symbols::line::Set {
            horizontal: "⣿",
            ..symbols::line::THICK
        };
        let line_gauge = LineGauge::default()
            .ratio(progress_ratio)
            .label(progress_label_text)
            .line_set(custom_line_set)
            .filled_style(ctx.apply(Style::default().fg(ctx.state_success())));
        f.render_widget(line_gauge, progress_chunks[1]);

        let status_text = if state.activity_message.is_empty() {
            "Waiting..."
        } else {
            state.activity_message.as_str()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Status:   ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(status_text),
            ])),
            detail_rows[1],
        );

        let total_pieces = state.number_of_pieces_total as usize;
        let (seeds, leeches) = state
            .peers
            .iter()
            .filter(|p| p.last_action != "Connecting...")
            .fold((0, 0), |(s, l), peer| {
                if total_pieces > 0 {
                    let pieces_have = peer
                        .bitfield
                        .iter()
                        .take(total_pieces)
                        .filter(|&&b| b)
                        .count();
                    if pieces_have == total_pieces {
                        (s + 1, l)
                    } else {
                        (s, l + 1)
                    }
                } else {
                    (s, l + 1)
                }
            });
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Peers:    ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format!(
                    "{} (",
                    state.number_of_successfully_connected_peers
                )),
                Span::styled(
                    format!("{}", seeds),
                    ctx.apply(Style::default().fg(ctx.state_success())),
                ),
                Span::raw(" / "),
                Span::styled(
                    format!("{}", leeches),
                    ctx.apply(Style::default().fg(ctx.state_error())),
                ),
                Span::raw(")"),
            ])),
            detail_rows[2],
        );

        let written_size_spans = if state.number_of_pieces_completed < state.number_of_pieces_total
        {
            vec![
                Span::styled(
                    "Written:  ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format_bytes(state.bytes_written)),
                Span::raw(format!(" / {}", format_bytes(state.total_size))),
            ]
        } else {
            vec![
                Span::styled(
                    "Size:     ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format_bytes(state.total_size)),
            ]
        };
        f.render_widget(
            Paragraph::new(Line::from(written_size_spans)),
            detail_rows[3],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Pieces:   ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format!(
                    "{}/{}",
                    state.number_of_pieces_completed, state.number_of_pieces_total
                )),
            ])),
            detail_rows[4],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "ETA:      ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format_duration(state.eta)),
            ])),
            detail_rows[5],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Announce: ",
                    ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
                ),
                Span::raw(format_countdown(state.next_announce_in)),
            ])),
            detail_rows[6],
        );
    } else {
        let placeholder_style = ctx.apply(Style::default().fg(ctx.theme.semantic.overlay0));
        let label_style = ctx.apply(Style::default().fg(ctx.theme.semantic.surface2));

        let progress_chunks =
            ratatui::layout::Layout::horizontal([Constraint::Length(11), Constraint::Min(0)])
                .split(detail_rows[0]);
        f.render_widget(
            Paragraph::new("Progress: ").style(label_style),
            progress_chunks[0],
        );
        let line_gauge = LineGauge::default()
            .ratio(0.0)
            .label(" --.--%")
            .style(placeholder_style);
        f.render_widget(line_gauge, progress_chunks[1]);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Status:   ", label_style),
                Span::styled("No Selection", placeholder_style),
            ])),
            detail_rows[1],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Peers:    ", label_style),
                Span::styled("- (- / -)", placeholder_style),
            ])),
            detail_rows[2],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Size:     ", label_style),
                Span::styled("- / -", placeholder_style),
            ])),
            detail_rows[3],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Pieces:   ", label_style),
                Span::styled("- / -", placeholder_style),
            ])),
            detail_rows[4],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("ETA:      ", label_style),
                Span::styled("--:--:--", placeholder_style),
            ])),
            detail_rows[5],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Announce: ", label_style),
                Span::styled("--s", placeholder_style),
            ])),
            detail_rows[6],
        );
    }
}

pub fn draw_network_chart(
    f: &mut Frame,
    app_state: &AppState,
    chart_chunk: Rect,
    ctx: &ThemeContext,
) {
    if chart_chunk.width < 5 || chart_chunk.height < 5 {
        return;
    }

    let smooth_data = |data: &[u64], alpha: f64| -> Vec<u64> {
        if data.is_empty() {
            return Vec::new();
        }
        let mut smoothed_data = Vec::with_capacity(data.len());
        let mut last_ema = data[0] as f64;
        smoothed_data.push(last_ema as u64);
        for &value in data.iter().skip(1) {
            let current_ema = (value as f64 * alpha) + (last_ema * (1.0 - alpha));
            smoothed_data.push(current_ema as u64);
            last_ema = current_ema;
        }
        smoothed_data
    };

    let (
        dl_history_source,
        ul_history_source,
        backoff_history_source_ms,
        time_window_points,
        _time_unit_secs,
    ) = match app_state.graph_mode {
        GraphDisplayMode::ThreeHours
        | GraphDisplayMode::TwelveHours
        | GraphDisplayMode::TwentyFourHours => (
            &app_state.minute_avg_dl_history,
            &app_state.minute_avg_ul_history,
            &app_state.minute_disk_backoff_history_ms,
            MINUTES_HISTORY_MAX,
            60,
        ),
        _ => {
            let points = app_state.graph_mode.as_seconds().min(SECONDS_HISTORY_MAX);
            (
                &app_state.avg_download_history,
                &app_state.avg_upload_history,
                &app_state.disk_backoff_history_ms,
                points,
                1,
            )
        }
    };

    let dl_len = dl_history_source.len();
    let ul_len = ul_history_source.len();
    let backoff_len = backoff_history_source_ms.len();

    let available_points = dl_len.min(ul_len).min(backoff_len);
    let points_to_show = time_window_points.min(available_points);

    let dl_history_slice = &dl_history_source[dl_len.saturating_sub(points_to_show)..];
    let ul_history_slice = &ul_history_source[ul_len.saturating_sub(points_to_show)..];

    let skip_count = backoff_len.saturating_sub(points_to_show);
    let backoff_history_relevant_ms: Vec<u64> = backoff_history_source_ms
        .iter()
        .skip(skip_count)
        .copied()
        .collect();

    let stable_max_speed = dl_history_slice
        .iter()
        .chain(ul_history_slice.iter())
        .max()
        .copied()
        .unwrap_or(10_000);
    let nice_max_speed = calculate_nice_upper_bound(stable_max_speed);

    let smoothing_period = 5.0;
    let alpha = 2.0 / (smoothing_period + 1.0);
    let smoothed_dl_data = smooth_data(dl_history_slice, alpha);
    let smoothed_ul_data = smooth_data(ul_history_slice, alpha);

    let dl_data: Vec<(f64, f64)> = smoothed_dl_data
        .iter()
        .enumerate()
        .map(|(i, &s)| (i as f64, s as f64))
        .collect();
    let ul_data: Vec<(f64, f64)> = smoothed_ul_data
        .iter()
        .enumerate()
        .map(|(i, &s)| (i as f64, s as f64))
        .collect();

    let backoff_marker_data: Vec<(f64, f64)> = backoff_history_relevant_ms
        .iter()
        .enumerate()
        .filter_map(|(i, &ms)| {
            if ms > 0 {
                let y_val = smoothed_dl_data.get(i).copied().unwrap_or(0) as f64;
                Some((i as f64, y_val))
            } else {
                None
            }
        })
        .collect();

    let backoff_dataset = ratatui::widgets::Dataset::default()
        .name("File Limits")
        .marker(ratatui::symbols::Marker::Braille)
        .graph_type(ratatui::widgets::GraphType::Scatter)
        .style(
            ctx.apply(
                Style::default()
                    .fg(ctx.state_error())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .data(&backoff_marker_data);

    let datasets = vec![
        ratatui::widgets::Dataset::default()
            .name("Download")
            .marker(ratatui::symbols::Marker::Braille)
            .style(
                ctx.apply(
                    Style::default()
                        .fg(ctx.state_info())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .data(&dl_data),
        ratatui::widgets::Dataset::default()
            .name("Upload")
            .marker(ratatui::symbols::Marker::Braille)
            .style(
                ctx.apply(
                    Style::default()
                        .fg(ctx.state_success())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .data(&ul_data),
        backoff_dataset,
    ];

    let y_speed_axis_labels = vec![
        Span::raw("0"),
        Span::styled(
            format_speed(nice_max_speed / 2),
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
        ),
        Span::styled(
            format_speed(nice_max_speed),
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
        ),
    ];
    let x_labels = generate_x_axis_labels(ctx, app_state.graph_mode);

    let all_modes = [
        GraphDisplayMode::OneMinute,
        GraphDisplayMode::FiveMinutes,
        GraphDisplayMode::TenMinutes,
        GraphDisplayMode::ThirtyMinutes,
        GraphDisplayMode::OneHour,
        GraphDisplayMode::ThreeHours,
        GraphDisplayMode::TwelveHours,
        GraphDisplayMode::TwentyFourHours,
    ];
    let mut title_spans: Vec<Span> = vec![Span::styled(
        "Network Activity ",
        ctx.apply(Style::default().fg(ctx.accent_peach())),
    )];
    for (i, &mode) in all_modes.iter().enumerate() {
        let is_active = mode == app_state.graph_mode;
        let mode_str = mode.to_string();

        let style = if is_active {
            ctx.apply(
                Style::default()
                    .fg(ctx.state_warning())
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ctx.apply(Style::default().fg(ctx.theme.semantic.surface0))
        };

        title_spans.push(Span::styled(mode_str, style));

        if i < all_modes.len().saturating_sub(1) {
            title_spans.push(Span::styled(
                " ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.surface2)),
            ));
        }
    }
    let chart_title = Line::from(title_spans);

    let chart = ratatui::widgets::Chart::new(datasets)
        .block(
            Block::default()
                .title(chart_title)
                .borders(Borders::ALL)
                .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
        )
        .x_axis(
            ratatui::widgets::Axis::default()
                .style(ctx.apply(Style::default().fg(ctx.theme.semantic.overlay0)))
                .bounds([0.0, points_to_show.saturating_sub(1) as f64])
                .labels(x_labels),
        )
        .y_axis(
            ratatui::widgets::Axis::default()
                .style(ctx.apply(Style::default().fg(ctx.theme.semantic.overlay0)))
                .bounds([0.0, nice_max_speed as f64])
                .labels(y_speed_axis_labels),
        )
        .legend_position(Some(ratatui::widgets::LegendPosition::TopRight));

    f.render_widget(chart, chart_chunk);
}

pub fn draw_stats_panel(
    f: &mut Frame,
    app_state: &AppState,
    settings: &Settings,
    stats_chunk: Rect,
    ctx: &ThemeContext,
) {
    let total_peers = app_state
        .torrents
        .values()
        .map(|t| t.latest_state.number_of_successfully_connected_peers)
        .sum::<usize>();

    let total_library_size: u64 = app_state
        .torrents
        .values()
        .map(|t| t.latest_state.total_size)
        .sum();

    let dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
    let dl_limit = settings.global_download_limit_bps;

    let mut dl_spans = vec![
        Span::styled(
            "DL Speed: ",
            ctx.apply(Style::default().fg(ctx.metric_download()).bold()),
        ),
        Span::styled(
            format_speed(dl_speed),
            ctx.apply(Style::default().fg(ctx.metric_download()).bold()),
        ),
        Span::raw(" / "),
    ];
    if dl_limit > 0 && dl_speed >= dl_limit {
        dl_spans.push(Span::styled(
            format_limit_bps(dl_limit),
            ctx.apply(Style::default().fg(ctx.state_error())),
        ));
    } else {
        dl_spans.push(Span::styled(
            format_limit_bps(dl_limit),
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
        ));
    }

    let ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);
    let ul_limit = settings.global_upload_limit_bps;

    let mut ul_spans = vec![
        Span::styled(
            "UL Speed: ",
            ctx.apply(Style::default().fg(ctx.metric_upload()).bold()),
        ),
        Span::styled(
            format_speed(ul_speed),
            ctx.apply(Style::default().fg(ctx.metric_upload()).bold()),
        ),
        Span::raw(" / "),
    ];

    if ul_limit > 0 && ul_speed >= ul_limit {
        ul_spans.push(Span::styled(
            format_limit_bps(ul_limit),
            ctx.apply(Style::default().fg(ctx.state_error())),
        ));
    } else {
        ul_spans.push(Span::styled(
            format_limit_bps(ul_limit),
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
        ));
    }

    let thrash_text: String;
    let thrash_style: Style;
    let baseline_val = app_state.adaptive_max_scpb;
    let thrash_score_val = app_state.global_disk_thrash_score;
    let thrash_score_str = format!("{:.0}", thrash_score_val);

    if thrash_score_val < 0.01 {
        thrash_text = format!("- ({})", thrash_score_str);
        thrash_style = ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0));
    } else if baseline_val == 0.0 {
        thrash_text = format!("∞ ({})", thrash_score_str);
        thrash_style = ctx.apply(Style::default().fg(ctx.state_error())).bold();
    } else {
        let diff = thrash_score_val - baseline_val;
        let thrash_percentage = (diff / baseline_val) * 100.0;

        if thrash_percentage > -0.01 && thrash_percentage < 0.01 {
            thrash_text = format!("0.0% ({})", thrash_score_str);
            thrash_style = ctx.apply(Style::default().fg(ctx.theme.semantic.text));
        } else {
            thrash_text = format!("{:+.1}% ({})", thrash_percentage, thrash_score_str);
            if thrash_percentage > 15.0 {
                thrash_style = ctx.apply(Style::default().fg(ctx.state_error())).bold();
            } else if thrash_percentage > 0.0 {
                thrash_style = ctx.apply(Style::default().fg(ctx.state_warning()));
            } else {
                thrash_style = ctx.apply(Style::default().fg(ctx.state_success()));
            }
        }
    }

    let stats_text = vec![
        Line::from(vec![
            Span::styled(
                "Run Time: ",
                ctx.apply(Style::default().fg(ctx.accent_teal())),
            ),
            Span::styled(
                format_time(app_state.run_time),
                ctx.apply(Style::default().fg(ctx.accent_teal())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Torrents: ",
                ctx.apply(Style::default().fg(ctx.accent_peach())),
            ),
            Span::styled(
                format!(
                    "{} ({})",
                    app_state.torrents.len(),
                    format_bytes(total_library_size)
                ),
                ctx.apply(Style::default().fg(ctx.accent_peach())),
            ),
        ]),
        Line::from(""),
        Line::from(dl_spans),
        Line::from(vec![
            Span::styled(
                "Session DL: ",
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
            Span::styled(
                format_bytes(app_state.session_total_downloaded),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Lifetime DL: ",
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
            Span::styled(
                format_bytes(
                    app_state.lifetime_downloaded_from_config + app_state.session_total_downloaded,
                ),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(""),
        Line::from(ul_spans),
        Line::from(vec![
            Span::styled(
                "Session UL: ",
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled(
                format_bytes(app_state.session_total_uploaded),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Lifetime UL: ",
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled(
                format_bytes(
                    app_state.lifetime_uploaded_from_config + app_state.session_total_uploaded,
                ),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("CPU: ", ctx.apply(Style::default().fg(ctx.state_error()))),
            Span::styled(
                format!("{:.1}%", app_state.cpu_usage),
                ctx.apply(Style::default().fg(ctx.state_error())),
            ),
        ]),
        Line::from(vec![
            Span::styled("RAM: ", ctx.apply(Style::default().fg(ctx.state_warning()))),
            Span::styled(
                format!("{:.1}%", app_state.ram_usage_percent),
                ctx.apply(Style::default().fg(ctx.state_warning())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "App RAM: ",
                ctx.apply(Style::default().fg(ctx.accent_flamingo())),
            ),
            Span::styled(
                format_memory(app_state.app_ram_usage),
                ctx.apply(Style::default().fg(ctx.accent_flamingo())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Disk    ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
            ),
            Span::styled("↑ ", ctx.apply(Style::default().fg(ctx.state_success()))),
            Span::styled(
                format!("{:<12}", format_speed(app_state.avg_disk_read_bps)),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled("↓ ", ctx.apply(Style::default().fg(ctx.accent_sky()))),
            Span::styled(
                format_speed(app_state.avg_disk_write_bps),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Seek    ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
            ),
            Span::styled("↑ ", ctx.apply(Style::default().fg(ctx.state_success()))),
            Span::styled(
                format!(
                    "{:<12}",
                    format_bytes(app_state.global_disk_read_thrash_score)
                ),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled("↓ ", ctx.apply(Style::default().fg(ctx.accent_sky()))),
            Span::styled(
                format_bytes(app_state.global_disk_write_thrash_score),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Latency ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
            ),
            Span::styled("↑ ", ctx.apply(Style::default().fg(ctx.state_success()))),
            Span::styled(
                format!("{:<12}", format_latency(app_state.avg_disk_read_latency)),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled("↓ ", ctx.apply(Style::default().fg(ctx.accent_sky()))),
            Span::styled(
                format_latency(app_state.avg_disk_write_latency),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "IOPS    ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
            ),
            Span::styled("↑ ", ctx.apply(Style::default().fg(ctx.state_success()))),
            Span::styled(
                format!("{:<12}", format_iops(app_state.read_iops)),
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::styled("↓ ", ctx.apply(Style::default().fg(ctx.accent_sky()))),
            Span::styled(
                format_iops(app_state.write_iops),
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Next Tuning in: ",
                ctx.apply(Style::default().fg(ctx.theme.semantic.text)),
            ),
            Span::raw(format!("{}s", app_state.tuning_countdown)),
        ]),
        Line::from(vec![
            Span::styled(
                "Disk Thrash: ",
                ctx.apply(Style::default().fg(ctx.accent_teal())),
            ),
            Span::styled(thrash_text, ctx.apply(thrash_style)),
        ]),
        Line::from(vec![
            Span::styled(
                "Reserve Pool:  ",
                ctx.apply(Style::default().fg(ctx.accent_teal())),
            ),
            Span::raw(app_state.limits.reserve_permits.to_string()),
            format_limit_delta(
                ctx,
                app_state.limits.reserve_permits,
                app_state.last_tuning_limits.reserve_permits,
            ),
        ]),
        {
            let mut spans = format_permits_spans(
                ctx,
                "Peer Slots: ",
                total_peers,
                app_state.limits.max_connected_peers,
                ctx.state_selected(),
            );
            spans.push(format_limit_delta(
                ctx,
                app_state.limits.max_connected_peers,
                app_state.last_tuning_limits.max_connected_peers,
            ));
            Line::from(spans)
        },
        Line::from(vec![
            Span::styled(
                "Disk Reads:    ",
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
            Span::raw(app_state.limits.disk_read_permits.to_string()),
            format_limit_delta(
                ctx,
                app_state.limits.disk_read_permits,
                app_state.last_tuning_limits.disk_read_permits,
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Disk Writes:   ",
                ctx.apply(Style::default().fg(ctx.accent_sky())),
            ),
            Span::raw(app_state.limits.disk_write_permits.to_string()),
            format_limit_delta(
                ctx,
                app_state.limits.disk_write_permits,
                app_state.last_tuning_limits.disk_write_permits,
            ),
        ]),
    ];

    let (lvl, progress) = crate::tui::view::calculate_player_stats(app_state);
    let available_width = stats_chunk.width.saturating_sub(18) as usize;

    let (gauge_width, show_pct) = if available_width > 25 {
        (20, true)
    } else if available_width > 15 {
        (10, true)
    } else {
        (10, false)
    };

    let filled_len = (progress * gauge_width as f64).round() as usize;
    let empty_len = gauge_width - filled_len;
    let gauge_str = format!("[{}{}]", "=".repeat(filled_len), "-".repeat(empty_len));

    let mut title_spans = vec![
        Span::styled(
            "Stats",
            ctx.apply(Style::default().fg(ctx.theme.semantic.white)),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("Lvl {}", lvl),
            ctx.apply(Style::default().fg(ctx.state_warning()).bold()),
        ),
        Span::raw(" "),
        Span::styled(
            gauge_str,
            ctx.apply(Style::default().fg(ctx.state_success())),
        ),
    ];

    if show_pct {
        title_spans.push(Span::styled(
            format!(" {:.0}%", progress * 100.0),
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
        ));
    }

    let stats_paragraph = Paragraph::new(stats_text)
        .block(
            Block::default()
                .title(Line::from(title_spans))
                .borders(Borders::ALL)
                .borders(Borders::ALL)
                .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
        )
        .style(ctx.apply(Style::default().fg(ctx.theme.semantic.text)));

    f.render_widget(stats_paragraph, stats_chunk);
}

pub fn draw_peer_stream(f: &mut Frame, app_state: &AppState, area: Rect, ctx: &ThemeContext) {
    if area.height < 3 || area.width < 10 {
        return;
    }

    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let color_discovered = ctx.peer_discovered();
    let color_connected = ctx.peer_connected();
    let color_disconnected = ctx.peer_disconnected();
    let color_border = ctx.theme.semantic.border;

    let default_slice: Vec<u64> = Vec::new();

    let (disc_slice, conn_slice, disconn_slice) = if let Some(torrent) = selected_torrent {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let dh = &torrent.peer_discovery_history;
        let ch = &torrent.peer_connection_history;
        let dch = &torrent.peer_disconnect_history;

        (
            &dh[dh.len().saturating_sub(width)..],
            &ch[ch.len().saturating_sub(width)..],
            &dch[dch.len().saturating_sub(width)..],
        )
    } else {
        (&default_slice[..], &default_slice[..], &default_slice[..])
    };

    let discovered_count: u64 = disc_slice.iter().sum();
    let connected_count: u64 = conn_slice.iter().sum();
    let disconnected_count: u64 = disconn_slice.iter().sum();

    let legend_style_fn = |count: u64, color: Color| {
        if selected_torrent.is_some() && count > 0 {
            ctx.apply(Style::default().fg(color))
        } else {
            ctx.apply(Style::default().fg(ctx.theme.semantic.surface1))
        }
    };

    let legend_line = Line::from(vec![
        Span::styled(
            "Connected:",
            legend_style_fn(connected_count, color_connected),
        ),
        Span::styled(
            format!(" {} ", connected_count),
            legend_style_fn(connected_count, color_connected).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "Discovered:",
            legend_style_fn(discovered_count, color_discovered),
        ),
        Span::styled(
            format!(" {} ", discovered_count),
            legend_style_fn(discovered_count, color_discovered).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "Disconnected:",
            legend_style_fn(disconnected_count, color_disconnected),
        ),
        Span::styled(
            format!(" {} ", disconnected_count),
            legend_style_fn(disconnected_count, color_disconnected).add_modifier(Modifier::BOLD),
        ),
    ]);

    let time_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut conn_points_small = Vec::new();
    let mut disc_points_small = Vec::new();
    let mut disconn_points_small = Vec::new();

    let mut conn_points_large = Vec::new();
    let mut disc_points_large = Vec::new();
    let mut disconn_points_large = Vec::new();

    let mut rng = StdRng::seed_from_u64(time_seed);

    let mut generate_points = |data_slice: &[u64],
                               small_points: &mut Vec<(f64, f64)>,
                               large_points: &mut Vec<(f64, f64)>,
                               base_y: f64| {
        for (i, &val) in data_slice.iter().enumerate() {
            if val == 0 {
                continue;
            }
            let val_f = val as f64;
            let is_heavy = val > 3;

            let small_dot_count = (val_f.sqrt().ceil() as usize).clamp(1, 6);
            let activity_spread = (val_f * 0.08).min(0.6);
            let base_jitter = 0.05;
            let intensity = base_jitter + activity_spread;

            for _ in 0..small_dot_count {
                let x_jitter = rng.random_range(-intensity..intensity);
                let y_jitter = rng.random_range(-intensity..intensity);
                small_points.push((i as f64 + x_jitter, base_y + y_jitter));
            }

            if is_heavy {
                let heavy_jitter = rng.random_range(-0.1..0.1);
                large_points.push((i as f64 + heavy_jitter, base_y + heavy_jitter));
            }
        }
    };

    generate_points(
        conn_slice,
        &mut conn_points_small,
        &mut conn_points_large,
        3.0,
    );
    generate_points(
        disc_slice,
        &mut disc_points_small,
        &mut disc_points_large,
        2.0,
    );
    generate_points(
        disconn_slice,
        &mut disconn_points_small,
        &mut disconn_points_large,
        1.0,
    );

    let datasets = vec![
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::DIM),
            )
            .data(&conn_points_small),
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::DIM),
            )
            .data(&disc_points_small),
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::DIM),
            )
            .data(&disconn_points_small),
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&conn_points_large),
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&disc_points_large),
        ratatui::widgets::Dataset::default()
            .marker(ratatui::symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&disconn_points_large),
    ];

    let x_bound = disc_slice.len().max(1).saturating_sub(1) as f64;

    let chart = ratatui::widgets::Chart::new(datasets)
        .block(
            Block::default()
                .title_top(
                    Line::from(Span::styled(
                        " Peer Activity Stream ",
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
                    ))
                    .alignment(Alignment::Left),
                )
                .title_top(legend_line.alignment(Alignment::Right))
                .borders(Borders::ALL)
                .border_style(ctx.apply(Style::default().fg(color_border))),
        )
        .x_axis(ratatui::widgets::Axis::default().bounds([0.0, x_bound]))
        .y_axis(ratatui::widgets::Axis::default().bounds([0.5, 3.5]));

    f.render_widget(chart, area);
}

pub fn draw_vertical_block_stream(
    f: &mut Frame,
    app_state: &AppState,
    area: Rect,
    ctx: &ThemeContext,
) {
    if area.width < 2 {
        return;
    }
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    const UP_TRIANGLE: &str = "▲";
    const DOWN_TRIANGLE: &str = "▼";
    const SEPARATOR: &str = "·";

    let color_inflow = ctx.theme.scale.stream.inflow;
    let color_outflow = ctx.theme.scale.stream.outflow;
    let color_border = ctx.theme.semantic.border;
    let color_empty = ctx.theme.semantic.surface0;

    let (total_in, total_out) = if let Some(t) = selected_torrent {
        let in_sum: u64 = t.latest_state.blocks_in_history.iter().sum();
        let out_sum: u64 = t.latest_state.blocks_out_history.iter().sum();
        (in_sum, out_sum)
    } else {
        (0, 0)
    };

    let title_str = "Blocks";
    let title_len = title_str.len();
    let total_ops = total_in + total_out;

    let title_spans: Vec<Span> = if total_ops == 0 {
        vec![Span::styled(
            title_str,
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
        )]
    } else {
        let blue_ratio = total_in as f64 / total_ops as f64;
        let blue_chars = (blue_ratio * title_len as f64).round() as usize;
        let (blue_part, green_part) = title_str.split_at(blue_chars.min(title_len));
        let mut spans = Vec::new();
        if !blue_part.is_empty() {
            spans.push(Span::styled(
                blue_part,
                ctx.apply(Style::default().fg(color_inflow)),
            ));
        }
        if !green_part.is_empty() {
            spans.push(Span::styled(
                green_part,
                ctx.apply(Style::default().fg(color_outflow)),
            ));
        }
        spans
    };
    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(color_border)));

    let Some(torrent) = selected_torrent else {
        f.render_widget(block, area);
        return;
    };

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let history_len = inner_area.height as usize;
    let content_width = inner_area.width as usize;

    if history_len == 0 || content_width == 0 {
        return;
    }

    let in_history = &torrent.latest_state.blocks_in_history;
    let out_history = &torrent.latest_state.blocks_out_history;

    let in_slice = &in_history[in_history.len().saturating_sub(history_len)..];
    let out_slice = &out_history[out_history.len().saturating_sub(history_len)..];

    let slice_len = in_slice.len();
    let mut lines: Vec<Line> = Vec::with_capacity(history_len);
    let frame_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    for i in 0..history_len {
        let mut spans = Vec::new();
        let dl_slice_index = slice_len.saturating_sub(1).saturating_sub(i);
        let raw_blocks_in = if i < slice_len {
            *in_slice.get(dl_slice_index).unwrap_or(&0)
        } else {
            0
        };
        let upload_padding = history_len.saturating_sub(slice_len);
        let ul_slice_index = i.saturating_sub(upload_padding);
        let raw_blocks_out = if i >= upload_padding {
            *out_slice.get(ul_slice_index).unwrap_or(&0)
        } else {
            0
        };

        let total_raw = raw_blocks_in + raw_blocks_out;
        let mut blocks_in: u64;
        let mut blocks_out: u64;

        if total_raw > content_width as u64 {
            blocks_in =
                (raw_blocks_in as f64 / total_raw as f64 * content_width as f64).round() as u64;
            blocks_out =
                (raw_blocks_out as f64 / total_raw as f64 * content_width as f64).round() as u64;
            if raw_blocks_in > 0 && blocks_in == 0 {
                blocks_in = 1;
            }
            if raw_blocks_out > 0 && blocks_out == 0 {
                blocks_out = 1;
            }

            let total_drawn = blocks_in + blocks_out;
            if total_drawn > content_width as u64 {
                let overfill = total_drawn - content_width as u64;
                if raw_blocks_in > raw_blocks_out {
                    blocks_in = blocks_in.saturating_sub(overfill);
                } else {
                    blocks_out = blocks_out.saturating_sub(overfill);
                }
            } else if total_drawn < content_width as u64 {
                let remainder = (content_width as u64) - total_drawn;
                if raw_blocks_in > raw_blocks_out {
                    blocks_in += remainder;
                } else {
                    blocks_out += remainder;
                }
            }
        } else {
            blocks_in = raw_blocks_in;
            blocks_out = raw_blocks_out;
        }

        let total_blocks = (blocks_in + blocks_out) as usize;
        if total_blocks == 0 {
            let padding = " ".repeat(content_width.saturating_sub(1) / 2);
            let trailing_padding = content_width
                .saturating_sub(1)
                .saturating_sub(padding.len());
            spans.push(Span::raw(padding));
            spans.push(Span::styled(
                SEPARATOR,
                ctx.apply(Style::default().fg(color_empty)),
            ));
            spans.push(Span::raw(" ".repeat(trailing_padding)));
        } else {
            let padding = (content_width.saturating_sub(total_blocks)) / 2;
            let trailing_padding = content_width
                .saturating_sub(total_blocks)
                .saturating_sub(padding);

            let (
                larger_stream_count,
                smaller_stream_count,
                larger_symbol,
                smaller_symbol,
                larger_color,
                smaller_color,
                larger_seed_salt,
                smaller_seed_salt,
            ) = if blocks_in >= blocks_out {
                (
                    blocks_in,
                    blocks_out,
                    DOWN_TRIANGLE,
                    UP_TRIANGLE,
                    color_inflow,
                    color_outflow,
                    dl_slice_index as u64,
                    (ul_slice_index as u64) ^ 0xABCDEF,
                )
            } else {
                (
                    blocks_out,
                    blocks_in,
                    UP_TRIANGLE,
                    DOWN_TRIANGLE,
                    color_outflow,
                    color_inflow,
                    (ul_slice_index as u64) ^ 0xABCDEF,
                    dl_slice_index as u64,
                )
            };

            let mut order_rng = StdRng::seed_from_u64(
                (dl_slice_index as u64) ^ (ul_slice_index as u64) ^ 0xDEADBEEF,
            );
            let total_scaled_blocks_f64 = (larger_stream_count + smaller_stream_count) as f64;
            let ratio_smaller = smaller_stream_count as f64 / total_scaled_blocks_f64;
            let smaller_first: bool = order_rng.random_bool(1.0 - ratio_smaller);

            spans.push(Span::raw(" ".repeat(padding)));
            if smaller_first {
                render_sparkles(
                    &mut spans,
                    smaller_symbol,
                    smaller_stream_count,
                    smaller_color,
                    frame_seed ^ smaller_seed_salt,
                );
                render_sparkles(
                    &mut spans,
                    larger_symbol,
                    larger_stream_count,
                    larger_color,
                    frame_seed ^ larger_seed_salt,
                );
            } else {
                render_sparkles(
                    &mut spans,
                    larger_symbol,
                    larger_stream_count,
                    larger_color,
                    frame_seed ^ larger_seed_salt,
                );
                render_sparkles(
                    &mut spans,
                    smaller_symbol,
                    smaller_stream_count,
                    smaller_color,
                    frame_seed ^ smaller_seed_salt,
                );
            }
            spans.push(Span::raw(" ".repeat(trailing_padding)));
        }
        lines.push(Line::from(spans));
    }
    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner_area);
}

fn render_sparkles<'a>(
    spans: &mut Vec<Span<'a>>,
    symbol: &'a str,
    count: u64,
    color: Color,
    seed: u64,
) {
    let mut rng = StdRng::seed_from_u64(seed);
    for _ in 0..count {
        let is_bold: bool = rng.random();
        let mut style = Style::default().fg(color);
        style = if is_bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style.add_modifier(Modifier::DIM)
        };
        spans.push(Span::styled(symbol, style));
    }
}

pub fn draw_torrent_sparklines(
    f: &mut Frame,
    app_state: &AppState,
    area: Rect,
    ctx: &ThemeContext,
) {
    let torrent = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let Some(torrent) = torrent else {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
        f.render_widget(block, area);
        return;
    };

    let dl_history = &torrent.download_history;
    let ul_history = &torrent.upload_history;
    const ACTIVITY_WINDOW: usize = 60;
    let check_dl_slice = &dl_history[dl_history.len().saturating_sub(ACTIVITY_WINDOW)..];
    let check_ul_slice = &ul_history[ul_history.len().saturating_sub(ACTIVITY_WINDOW)..];
    let has_dl_activity = check_dl_slice.iter().any(|&s| s > 0);
    let has_ul_activity = check_ul_slice.iter().any(|&s| s > 0);

    if has_dl_activity && !has_ul_activity {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let dl_slice = &dl_history[dl_history.len().saturating_sub(width)..];
        let max_speed = dl_slice.iter().max().copied().unwrap_or(1);
        let nice_max_speed = calculate_nice_upper_bound(max_speed).max(1);

        let dl_sparkline = ratatui::widgets::Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("DL Activity (Peak: {})", format_speed(nice_max_speed)),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
                    ))
                    .borders(Borders::ALL)
                    .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
            )
            .data(dl_slice)
            .max(nice_max_speed)
            .style(ctx.apply(Style::default().fg(ctx.state_info())));
        f.render_widget(dl_sparkline, area);
    } else if !has_dl_activity && has_ul_activity {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let ul_slice = &ul_history[ul_history.len().saturating_sub(width)..];
        let max_speed = ul_slice.iter().max().copied().unwrap_or(1);
        let nice_max_speed = calculate_nice_upper_bound(max_speed).max(1);
        let ul_sparkline = ratatui::widgets::Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("UL Activity (Peak: {})", format_speed(nice_max_speed)),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
                    ))
                    .borders(Borders::ALL)
                    .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
            )
            .data(ul_slice)
            .max(nice_max_speed)
            .style(ctx.apply(Style::default().fg(ctx.state_success())));
        f.render_widget(ul_sparkline, area);
    } else if !has_dl_activity && !has_ul_activity {
        let style = ctx.apply(Style::default().fg(ctx.state_selected()));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
        let inner_area = block.inner(area);
        f.render_widget(block, area);

        let vertical_chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner_area);
        let throbber_width = 23;
        let horizontal_chunks = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(throbber_width),
            Constraint::Min(0),
        ])
        .split(vertical_chunks[1]);
        let inner_chunks = Layout::horizontal([
            Constraint::Length(1),
            Constraint::Length(21),
            Constraint::Length(1),
        ])
        .split(horizontal_chunks[1]);

        let throbber_left_area = inner_chunks[0];
        let label_area = inner_chunks[1];
        let throbber_right_area = inner_chunks[2];

        let label_text = Paragraph::new(" Searching for Peers ")
            .style(style)
            .alignment(Alignment::Center);
        let throbber_style = ctx.apply(
            Style::default()
                .fg(ctx.state_complete())
                .add_modifier(Modifier::BOLD),
        );
        let throbber_widget = Throbber::default().style(throbber_style);

        f.render_widget(label_text, label_area);
        f.render_stateful_widget(
            throbber_widget.clone(),
            throbber_left_area,
            &mut app_state.throbber_holder.borrow_mut().torrent_sparkline,
        );
        f.render_stateful_widget(
            throbber_widget,
            throbber_right_area,
            &mut app_state.throbber_holder.borrow_mut().torrent_sparkline,
        );
    } else {
        let sparkline_chunks =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
        let dl_sparkline_chunk = sparkline_chunks[0];
        let ul_sparkline_chunk = sparkline_chunks[1];

        let dl_width = dl_sparkline_chunk.width.saturating_sub(2).max(1) as usize;
        let ul_width = ul_sparkline_chunk.width.saturating_sub(2).max(1) as usize;
        let dl_slice = &dl_history[dl_history.len().saturating_sub(dl_width)..];
        let ul_slice = &ul_history[ul_history.len().saturating_sub(ul_width)..];
        let max_dl = dl_slice.iter().max().copied().unwrap_or(0);
        let max_ul = ul_slice.iter().max().copied().unwrap_or(0);
        let dl_nice_max = calculate_nice_upper_bound(max_dl).max(1);
        let ul_nice_max = calculate_nice_upper_bound(max_ul).max(1);

        let dl_sparkline = ratatui::widgets::Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("DL (Peak: {})", format_speed(dl_nice_max)),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
                    ))
                    .borders(Borders::ALL)
                    .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
            )
            .data(dl_slice)
            .max(dl_nice_max)
            .style(ctx.apply(Style::default().fg(ctx.state_info())));
        f.render_widget(dl_sparkline, dl_sparkline_chunk);

        let ul_sparkline = ratatui::widgets::Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("UL (Peak: {})", format_speed(ul_nice_max)),
                        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
                    ))
                    .borders(Borders::ALL)
                    .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
            )
            .data(ul_slice)
            .max(ul_nice_max)
            .style(ctx.apply(Style::default().fg(ctx.state_success())));
        f.render_widget(ul_sparkline, ul_sparkline_chunk);
    }
}

pub fn draw_peers_table(
    f: &mut Frame,
    app_state: &AppState,
    peers_chunk: Rect,
    ctx: &ThemeContext,
) {
    if peers_chunk.height < 2 || peers_chunk.width < 2 {
        return;
    }

    if let Some(info_hash) = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
    {
        if let Some(torrent) = app_state.torrents.get(info_hash) {
            let state = &torrent.latest_state;

            if peers_chunk.height > 0 {
                let has_established_peers =
                    state.peers.iter().any(|p| p.last_action != "Connecting...");

                let mut peers_to_display: Vec<PeerInfo> = if has_established_peers {
                    state
                        .peers
                        .iter()
                        .filter(|p| p.last_action != "Connecting...")
                        .cloned()
                        .collect()
                } else {
                    state.peers.clone()
                };

                let (sort_by, sort_direction) = app_state.peer_sort;
                peers_to_display.sort_by(|a, b| {
                    use crate::config::PeerSortColumn::*;
                    let ordering = match sort_by {
                        Flags => a.peer_choking.cmp(&b.peer_choking),
                        Completed => {
                            let total = state.number_of_pieces_total as usize;
                            if total == 0 {
                                std::cmp::Ordering::Equal
                            } else {
                                let a_c = a.bitfield.iter().take(total).filter(|&&h| h).count();
                                let b_c = b.bitfield.iter().take(total).filter(|&&h| h).count();
                                a_c.cmp(&b_c)
                            }
                        }
                        Address => a.address.cmp(&b.address),
                        Client => a.peer_id.cmp(&b.peer_id),
                        Action => a.last_action.cmp(&b.last_action),
                        DL => a.download_speed_bps.cmp(&b.download_speed_bps),
                        UL => a.upload_speed_bps.cmp(&b.upload_speed_bps),
                    };
                    if sort_direction == SortDirection::Ascending {
                        ordering
                    } else {
                        ordering.reverse()
                    }
                });

                let all_peer_cols = get_peer_columns();
                let smart_cols: Vec<SmartCol> = all_peer_cols
                    .iter()
                    .map(|c| SmartCol {
                        min_width: c.min_width,
                        priority: c.priority,
                        constraint: c.default_constraint,
                    })
                    .collect();

                let (constraints, visible_indices) =
                    compute_smart_table_layout(&smart_cols, peers_chunk.width, 1);

                let peer_border_style =
                    if matches!(app_state.ui.selected_header, SelectedHeader::Peer(_)) {
                        ctx.apply(Style::default().fg(ctx.state_selected()))
                    } else {
                        ctx.apply(Style::default().fg(ctx.theme.semantic.surface2))
                    };

                if peers_to_display.is_empty() {
                    draw_swarm_heatmap(
                        f,
                        ctx,
                        &state.peers,
                        state.number_of_pieces_total,
                        peers_chunk,
                    );
                } else {
                    let header_cells: Vec<Cell> = visible_indices
                        .iter()
                        .enumerate()
                        .map(|(visual_idx, &real_idx)| {
                            let def = &all_peer_cols[real_idx];

                            let is_selected =
                                app_state.ui.selected_header == SelectedHeader::Peer(visual_idx);
                            let is_sorting = def.sort_enum == Some(sort_by);

                            let mut style = ctx.apply(Style::default().fg(ctx.state_warning()));
                            if is_sorting {
                                style = style.fg(ctx.state_selected());
                            }
                            style = ctx.apply(style);

                            let mut text = def.header.to_string();
                            if is_sorting {
                                text.push_str(if sort_direction == SortDirection::Ascending {
                                    " ▲"
                                } else {
                                    " ▼"
                                });
                            }

                            let mut span = Span::styled(text, style);
                            if is_selected {
                                span = span.underlined().bold();
                            }
                            Cell::from(Line::from(vec![span]))
                        })
                        .collect();

                    let peer_header = Row::new(header_cells).height(1);

                    let peer_rows = peers_to_display.iter().map(|peer| {
                        let row_color =
                            if peer.download_speed_bps == 0 && peer.upload_speed_bps == 0 {
                                ctx.theme.semantic.surface1
                            } else {
                                ip_to_color(ctx, &peer.address)
                            };

                        let cells: Vec<Cell> = visible_indices
                            .iter()
                            .map(|&real_idx| {
                                let def = &all_peer_cols[real_idx];
                                match def.id {
                                    PeerColumnId::Flags => Line::from(vec![
                                        Span::styled(
                                            "■",
                                            ctx.apply(Style::default().fg(if peer.am_interested {
                                                ctx.accent_sapphire()
                                            } else {
                                                ctx.theme.semantic.surface1
                                            })),
                                        ),
                                        Span::styled(
                                            "■",
                                            ctx.apply(Style::default().fg(if peer.peer_choking {
                                                ctx.accent_maroon()
                                            } else {
                                                ctx.theme.semantic.surface1
                                            })),
                                        ),
                                        Span::styled(
                                            "■",
                                            ctx.apply(Style::default().fg(
                                                if peer.peer_interested {
                                                    ctx.accent_teal()
                                                } else {
                                                    ctx.theme.semantic.surface1
                                                },
                                            )),
                                        ),
                                        Span::styled(
                                            "■",
                                            ctx.apply(Style::default().fg(if peer.am_choking {
                                                ctx.accent_peach()
                                            } else {
                                                ctx.theme.semantic.surface1
                                            })),
                                        ),
                                    ])
                                    .into(),
                                    PeerColumnId::Address => {
                                        let display = if app_state.anonymize_torrent_names {
                                            "xxx.xxx.xxx"
                                        } else {
                                            &peer.address
                                        };
                                        Cell::from(display.to_string())
                                    }
                                    PeerColumnId::Client => {
                                        let raw_client = parse_peer_id(&peer.peer_id);
                                        Cell::from(sanitize_text(&raw_client))
                                    }
                                    PeerColumnId::Action => Cell::from(peer.last_action.clone()),
                                    PeerColumnId::Progress => {
                                        let total = state.number_of_pieces_total as usize;
                                        let pct = if total > 0 {
                                            let c = peer
                                                .bitfield
                                                .iter()
                                                .take(total)
                                                .filter(|&&b| b)
                                                .count();
                                            (c as f64 / total as f64) * 100.0
                                        } else {
                                            0.0
                                        };
                                        Cell::from(format!("{:.0}%", pct))
                                    }
                                    PeerColumnId::DownSpeed => {
                                        if peers_chunk.width > 120 {
                                            Cell::from(format!(
                                                "{} ({})",
                                                format_speed(peer.download_speed_bps),
                                                format_bytes(peer.total_downloaded)
                                            ))
                                        } else {
                                            Cell::from(format_speed(peer.download_speed_bps))
                                        }
                                    }
                                    PeerColumnId::UpSpeed => {
                                        if peers_chunk.width > 120 {
                                            Cell::from(format!(
                                                "{} ({})",
                                                format_speed(peer.upload_speed_bps),
                                                format_bytes(peer.total_uploaded)
                                            ))
                                        } else {
                                            Cell::from(format_speed(peer.upload_speed_bps))
                                        }
                                    }
                                }
                            })
                            .collect();
                        Row::new(cells).style(ctx.apply(Style::default().fg(row_color)))
                    });

                    let peers_table = Table::new(peer_rows, constraints)
                        .header(peer_header)
                        .block(Block::default());

                    let table_rows_needed: u16 = 1 + peers_to_display.len() as u16;
                    let peer_block_height_needed: u16 = table_rows_needed + 1;
                    let remaining_height =
                        peers_chunk.height.saturating_sub(peer_block_height_needed);
                    const MIN_HEATMAP_HEIGHT: u16 = 4;

                    let peers_block = Block::default()
                        .padding(Padding::new(1, 1, 0, 0))
                        .border_style(peer_border_style);

                    if remaining_height >= MIN_HEATMAP_HEIGHT {
                        let layout_chunks = Layout::vertical([
                            Constraint::Length(peer_block_height_needed),
                            Constraint::Min(0),
                        ])
                        .split(peers_chunk);
                        let inner_peers_area = peers_block.inner(layout_chunks[0]);
                        f.render_widget(peers_block, layout_chunks[0]);
                        f.render_widget(peers_table, inner_peers_area);
                        draw_swarm_heatmap(
                            f,
                            ctx,
                            &state.peers,
                            state.number_of_pieces_total,
                            layout_chunks[1],
                        );
                    } else {
                        let inner_peers_area = peers_block.inner(peers_chunk);
                        f.render_widget(peers_block, peers_chunk);
                        f.render_widget(peers_table, inner_peers_area);
                    }
                }
            }
        }
    } else {
        draw_swarm_heatmap(f, ctx, &[], 0, peers_chunk);
    }
}

fn draw_swarm_heatmap(
    f: &mut Frame,
    ctx: &ThemeContext,
    peers: &[PeerInfo],
    total_pieces: u32,
    area: Rect,
) {
    let color_status_low = ctx.apply(
        Style::default()
            .fg(ctx.state_error())
            .add_modifier(Modifier::DIM),
    );
    let color_status_medium = ctx.apply(
        Style::default()
            .fg(ctx.state_warning())
            .add_modifier(Modifier::DIM),
    );
    let color_status_high = ctx.apply(
        Style::default()
            .fg(ctx.state_info())
            .add_modifier(Modifier::DIM),
    );
    let color_status_complete = ctx.apply(
        Style::default()
            .fg(ctx.state_complete())
            .add_modifier(Modifier::BOLD),
    );
    let color_status_empty = ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1));
    let color_status_waiting = ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1));

    let color_heatmap_low = ctx.theme.scale.heatmap.low;
    let color_heatmap_medium = ctx.theme.scale.heatmap.medium;
    let color_heatmap_high = ctx.theme.scale.heatmap.high;
    let color_heatmap_empty = ctx.theme.scale.heatmap.empty;

    let shade_light = symbols::shade::LIGHT;
    let shade_medium = symbols::shade::MEDIUM;
    let shade_dark = symbols::shade::DARK;

    let total_pieces_usize = total_pieces as usize;
    let mut availability: Vec<u32> = vec![0; total_pieces_usize];
    if total_pieces_usize > 0 {
        for peer in peers {
            for (i, has_piece) in peer.bitfield.iter().enumerate().take(total_pieces_usize) {
                if *has_piece {
                    availability[i] += 1;
                }
            }
        }
    }

    let max_avail = availability.iter().max().copied().unwrap_or(0);
    let pieces_available_in_swarm = availability.iter().filter(|&&count| count > 0).count();
    let is_swarm_complete =
        total_pieces_usize > 0 && pieces_available_in_swarm == total_pieces_usize;
    let total_peers = peers.len();

    let (status_text, status_style) = if total_pieces_usize == 0 {
        ("Waiting...".to_string(), color_status_waiting)
    } else if is_swarm_complete {
        ("Complete".to_string(), color_status_complete)
    } else if max_avail == 0 {
        ("Empty".to_string(), color_status_empty)
    } else if total_peers == 0 {
        ("Low (0%)".to_string(), color_status_low)
    } else {
        let availability_percentage =
            (pieces_available_in_swarm as f64 / total_pieces_usize as f64) * 100.0;
        if availability_percentage < 33.3 {
            (
                format!("Low ({:.0}%)", availability_percentage),
                color_status_low,
            )
        } else if availability_percentage < 66.6 {
            (
                format!("Medium ({:.0}%)", availability_percentage),
                color_status_medium,
            )
        } else {
            (
                format!("High ({:.0}%)", availability_percentage),
                color_status_high,
            )
        }
    };

    let title = Line::from(vec![
        Span::styled(
            " Swarm Availability: ",
            ctx.apply(Style::default().fg(ctx.state_complete())),
        ),
        Span::styled(status_text, status_style),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::NONE)
        .padding(Padding::new(1, 1, 0, 1))
        .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if total_pieces_usize == 0 {
        let available_width = inner_area.width as usize;
        let available_height = inner_area.height as usize;
        let mut lines = Vec::with_capacity(available_height);

        for _ in 0..available_height {
            let row_str = shade_light.repeat(available_width);
            lines.push(Line::from(Span::styled(
                row_str,
                ctx.apply(Style::default().fg(ctx.theme.semantic.surface1)),
            )));
        }

        let heatmap = Paragraph::new(lines);
        f.render_widget(heatmap, inner_area);
        return;
    }

    let max_avail_f64 = max_avail.max(5) as f64;
    let available_width = inner_area.width as usize;
    let available_height = inner_area.height as usize;
    let total_cells = (available_width * available_height) as u64;

    if total_cells == 0 {
        return;
    }

    let mut lines = Vec::with_capacity(available_height);
    let total_pieces_u64 = total_pieces_usize as u64;

    for y in 0..available_height {
        let mut spans = Vec::with_capacity(available_width);
        for x in 0..available_width {
            let cell_index = (y * available_width + x) as u64;
            let piece_index = ((cell_index * total_pieces_u64) / total_cells) as usize;
            if piece_index >= total_pieces_usize {
                spans.push(Span::raw(" "));
                continue;
            }
            let count = availability[piece_index];
            let (piece_char, color) = if count == 0 {
                (shade_light, color_heatmap_empty)
            } else {
                let norm_val = count as f64 / max_avail_f64;
                if norm_val < 0.20 {
                    (shade_light, color_heatmap_low)
                } else if norm_val < 0.80 {
                    (shade_medium, color_heatmap_medium)
                } else {
                    (shade_dark, color_heatmap_high)
                }
            };
            spans.push(Span::styled(
                piece_char.to_string(),
                ctx.apply(Style::default().fg(color)),
            ));
        }
        lines.push(Line::from(spans));
    }
    let heatmap = Paragraph::new(lines);
    f.render_widget(heatmap, inner_area);
}

pub(crate) fn handle_navigation(app_state: &mut AppState, key_code: KeyCode) {
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.ui.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let selected_torrent_has_peers =
        selected_torrent.is_some_and(|torrent| !torrent.latest_state.peers.is_empty());

    let selected_torrent_peer_count =
        selected_torrent.map_or(0, |torrent| torrent.latest_state.peers.len());

    let layout_ctx = LayoutContext::new(app_state.screen_area, app_state, DEFAULT_SIDEBAR_PERCENT);
    let layout_plan = calculate_layout(app_state.screen_area, &layout_ctx);
    let (_, visible_torrent_columns) =
        compute_visible_torrent_columns(app_state, layout_plan.list.width);
    let (_, visible_peer_columns) = compute_visible_peer_columns(layout_plan.peers.width);
    let torrent_col_count = visible_torrent_columns.len();
    let peer_col_count = visible_peer_columns.len();

    app_state.ui.selected_header = match app_state.ui.selected_header {
        SelectedHeader::Torrent(i) => {
            if torrent_col_count == 0 {
                SelectedHeader::Torrent(0)
            } else {
                SelectedHeader::Torrent(i.min(torrent_col_count - 1))
            }
        }
        SelectedHeader::Peer(i) => {
            if !selected_torrent_has_peers || peer_col_count == 0 {
                SelectedHeader::Torrent(torrent_col_count.saturating_sub(1))
            } else {
                SelectedHeader::Peer(i.min(peer_col_count - 1))
            }
        }
    };

    match key_code {
        KeyCode::Up | KeyCode::Char('k') => match app_state.ui.selected_header {
            SelectedHeader::Torrent(_) => {
                app_state.ui.selected_torrent_index =
                    app_state.ui.selected_torrent_index.saturating_sub(1);
                app_state.ui.selected_peer_index = 0;
            }
            SelectedHeader::Peer(_) => {
                app_state.ui.selected_peer_index =
                    app_state.ui.selected_peer_index.saturating_sub(1);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match app_state.ui.selected_header {
            SelectedHeader::Torrent(_) => {
                if !app_state.torrent_list_order.is_empty() {
                    let new_index = app_state.ui.selected_torrent_index.saturating_add(1);
                    if new_index < app_state.torrent_list_order.len() {
                        app_state.ui.selected_torrent_index = new_index;
                    }
                }
                app_state.ui.selected_peer_index = 0;
            }
            SelectedHeader::Peer(_) => {
                if selected_torrent_peer_count > 0 {
                    let new_index = app_state.ui.selected_peer_index.saturating_add(1);
                    if new_index < selected_torrent_peer_count {
                        app_state.ui.selected_peer_index = new_index;
                    }
                }
            }
        },
        KeyCode::Left | KeyCode::Char('h') => {
            app_state.ui.selected_header = match app_state.ui.selected_header {
                SelectedHeader::Torrent(0) => {
                    if selected_torrent_has_peers && peer_col_count > 0 {
                        SelectedHeader::Peer(peer_col_count - 1)
                    } else {
                        SelectedHeader::Torrent(0)
                    }
                }
                SelectedHeader::Torrent(i) => SelectedHeader::Torrent(i - 1),
                SelectedHeader::Peer(0) => {
                    SelectedHeader::Torrent(torrent_col_count.saturating_sub(1))
                }
                SelectedHeader::Peer(i) => SelectedHeader::Peer(i - 1),
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app_state.ui.selected_header = match app_state.ui.selected_header {
                SelectedHeader::Torrent(i) => {
                    if i < torrent_col_count.saturating_sub(1) {
                        SelectedHeader::Torrent(i + 1)
                    } else if selected_torrent_has_peers && peer_col_count > 0 {
                        SelectedHeader::Peer(0)
                    } else {
                        SelectedHeader::Torrent(i)
                    }
                }
                SelectedHeader::Peer(i) => {
                    if i < peer_col_count.saturating_sub(1) {
                        SelectedHeader::Peer(i + 1)
                    } else {
                        SelectedHeader::Torrent(0)
                    }
                }
            };
        }
        _ => {}
    }
}

fn handle_search_key(key_code: KeyCode, app: &mut App) -> bool {
    if !matches!(app.app_state.mode, AppMode::Normal) || !app.app_state.ui.is_searching {
        return false;
    }

    match key_code {
        KeyCode::Esc => {
            app.app_state.ui.is_searching = false;
            app.app_state.ui.search_query.clear();
            app.sort_and_filter_torrent_list();
            app.app_state.ui.selected_torrent_index = 0;
        }
        KeyCode::Enter => {
            app.app_state.ui.is_searching = false;
        }
        KeyCode::Backspace => {
            app.app_state.ui.search_query.pop();
            app.sort_and_filter_torrent_list();
            app.app_state.ui.selected_torrent_index = 0;
        }
        KeyCode::Char(c) => {
            app.app_state.ui.search_query.push(c);
            app.sort_and_filter_torrent_list();
            app.app_state.ui.selected_torrent_index = 0;
        }
        _ => {}
    }

    true
}

async fn handle_pasted_text(app: &mut App, pasted_text: &str) {
    let pasted_text = pasted_text.trim();

    if pasted_text.starts_with("magnet:") {
        let download_path = app.client_configs.default_download_folder.clone();

        app.add_magnet_torrent(
            "Fetching name...".to_string(),
            pasted_text.to_string(),
            download_path.clone(),
            false,
            TorrentControlState::Running,
            HashMap::new(),
            None,
        )
        .await;

        if download_path.is_none() {
            app.app_state.pending_torrent_link = pasted_text.to_string();
            let initial_path = app.get_initial_destination_path();
            let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                path: initial_path,
                browser_mode: FileBrowserMode::DownloadLocSelection {
                    torrent_files: vec![],
                    container_name: String::new(),
                    use_container: false,
                    is_editing_name: false,
                    focused_pane: BrowserPane::FileSystem,
                    preview_tree: Vec::new(),
                    preview_state: TreeViewState::default(),
                    cursor_pos: 0,
                    original_name_backup: "Magnet Download".to_string(),
                },
                highlight_path: None,
            });
        }
    } else {
        let path = Path::new(pasted_text);
        if path.is_file() && path.extension().is_some_and(|ext| ext == "torrent") {
            if let Some(download_path) = app.client_configs.default_download_folder.clone() {
                app.add_torrent_from_file(
                    path.to_path_buf(),
                    Some(download_path),
                    false,
                    TorrentControlState::Running,
                    HashMap::new(),
                    None,
                )
                .await;
            } else {
                let _ = app
                    .app_command_tx
                    .try_send(AppCommand::AddTorrentFromFile(path.to_path_buf()));
            }
        } else {
            tracing_event!(
                Level::WARN,
                "Clipboard content not recognized as magnet link or torrent file: {}",
                pasted_text
            );
            app.app_state.system_error = Some(
                "Clipboard content not recognized as magnet link or torrent file.".to_string(),
            );
        }
    }
}

pub async fn handle_event(event: CrosstermEvent, app: &mut App) {
    match event {
        CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
            let _ = handle_key_press(key.code, app).await;
        }
        #[cfg(not(windows))]
        CrosstermEvent::Paste(pasted_text) => {
            let _ = handle_paste_text(pasted_text.trim().to_string(), app).await;
        }
        _ => {}
    };
}

async fn handle_key_press(key_code: KeyCode, app: &mut App) -> bool {
    if handle_search_key(key_code, app) {
        app.app_state.ui.needs_redraw = true;
        return true;
    }

    if handle_reducer_key(key_code, app).await {
        return true;
    }

    false
}

async fn handle_reducer_key(key_code: KeyCode, app: &mut App) -> bool {
    let Some(action) = map_key_to_ui_action(key_code) else {
        return false;
    };

    let result = reduce_ui_action(&mut app.app_state, action);
    if result.redraw {
        app.app_state.ui.needs_redraw = true;
    }
    execute_ui_effects(app, result.effects).await;
    true
}

async fn handle_paste_text(text: String, app: &mut App) -> bool {
    let result = reduce_ui_action(&mut app.app_state, UiAction::PasteText(text));
    if result.redraw {
        app.app_state.ui.needs_redraw = true;
    }
    execute_ui_effects(app, result.effects).await;
    true
}

async fn execute_ui_effects(app: &mut App, effects: Vec<UiEffect>) {
    for effect in effects {
        execute_ui_effect(app, effect).await;
    }
}

async fn execute_ui_effect(app: &mut App, effect: UiEffect) {
    match effect {
        UiEffect::OpenAddTorrentFileBrowser => {
            let initial_path = app.get_initial_source_path();
            let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                path: initial_path,
                browser_mode: FileBrowserMode::File(vec![".torrent".to_string()]),
                highlight_path: None,
            });
        }
        UiEffect::OpenConfigScreen => {
            app.app_state.ui.config.settings_edit = Box::new(app.client_configs.clone());
            app.app_state.ui.config.selected_index = 0;
            app.app_state.ui.config.items = ConfigItem::iter().collect::<Vec<_>>();
            app.app_state.ui.config.editing = None;
            app.app_state.mode = AppMode::Config;
        }
        UiEffect::BroadcastManagerDataRate(new_rate) => {
            for manager_tx in app.torrent_manager_command_txs.values() {
                let _ = manager_tx.try_send(ManagerCommand::SetDataRate(new_rate));
            }
        }
        UiEffect::ApplyThemePrev => {
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
        UiEffect::ApplyThemeNext => {
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
        UiEffect::SendPause(info_hash) => {
            if let Some(torrent_manager_command_tx) =
                app.torrent_manager_command_txs.get(&info_hash)
            {
                let torrent_manager_command_tx_clone = torrent_manager_command_tx.clone();
                tokio::spawn(async move {
                    let _ = torrent_manager_command_tx_clone
                        .send(crate::torrent_manager::ManagerCommand::Pause)
                        .await;
                });
            }
        }
        UiEffect::SendResume(info_hash) => {
            if let Some(torrent_manager_command_tx) =
                app.torrent_manager_command_txs.get(&info_hash)
            {
                let torrent_manager_command_tx_clone = torrent_manager_command_tx.clone();
                tokio::spawn(async move {
                    let _ = torrent_manager_command_tx_clone
                        .send(crate::torrent_manager::ManagerCommand::Resume)
                        .await;
                });
            }
        }
        UiEffect::OpenHelpScreen => {
            app.app_state.mode = AppMode::Help;
        }
        UiEffect::HandlePastedText(text) => {
            handle_pasted_text(app, &text).await;
        }
        #[cfg(windows)]
        UiEffect::ReadClipboardAndPaste => match ClipboardContext::new() {
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
                app.app_state.system_error = Some(format!("Clipboard initialization error: {}", e));
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        AppState, DataRate, PeerInfo, SelectedHeader, TorrentControlState, TorrentDisplayState,
        TorrentMetrics,
    };
    use crate::config::{PeerSortColumn, SortDirection, TorrentSortColumn};

    fn create_mock_metrics(peer_count: usize) -> TorrentMetrics {
        let mut metrics = TorrentMetrics::default();
        let mut peers = Vec::new();
        for i in 0..peer_count {
            peers.push(PeerInfo {
                address: format!("127.0.0.1:{}", 6881 + i),
                ..Default::default()
            });
        }
        metrics.peers = peers;
        metrics
    }

    fn create_mock_display_state(peer_count: usize) -> TorrentDisplayState {
        TorrentDisplayState {
            latest_state: create_mock_metrics(peer_count),
            ..Default::default()
        }
    }

    fn create_test_app_state() -> AppState {
        let mut app_state = AppState {
            screen_area: ratatui::layout::Rect::new(0, 0, 200, 100),
            ..Default::default()
        };

        let torrent_a = create_mock_display_state(2);
        let torrent_b = create_mock_display_state(0);

        app_state
            .torrents
            .insert("hash_a".as_bytes().to_vec(), torrent_a);
        app_state
            .torrents
            .insert("hash_b".as_bytes().to_vec(), torrent_b);
        app_state.torrent_list_order =
            vec!["hash_a".as_bytes().to_vec(), "hash_b".as_bytes().to_vec()];

        app_state
    }

    #[test]
    fn reducer_start_search_sets_search_and_resets_selection() {
        let mut app_state = AppState::default();
        app_state.ui.is_searching = false;
        app_state.ui.selected_torrent_index = 7;

        let result = reduce_ui_action(&mut app_state, UiAction::StartSearch);

        assert!(result.redraw);
        assert!(app_state.ui.is_searching);
        assert_eq!(app_state.ui.selected_torrent_index, 0);
    }

    #[test]
    fn reducer_start_search_keeps_browser_search_state_intact() {
        let mut app_state = AppState::default();
        app_state.ui.file_browser.is_searching = true;
        app_state.ui.file_browser.search_query = "downloads".to_string();

        let result = reduce_ui_action(&mut app_state, UiAction::StartSearch);

        assert!(result.redraw);
        assert!(app_state.ui.is_searching);
        assert!(app_state.ui.file_browser.is_searching);
        assert_eq!(app_state.ui.file_browser.search_query, "downloads");
    }

    #[test]
    fn reducer_clear_system_error_clears_error() {
        let mut app_state = AppState {
            system_error: Some("boom".to_string()),
            ..Default::default()
        };

        let result = reduce_ui_action(&mut app_state, UiAction::ClearSystemError);

        assert!(result.redraw);
        assert!(app_state.system_error.is_none());
    }

    #[test]
    fn reducer_navigate_updates_selection() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0;
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        let result = reduce_ui_action(&mut app_state, UiAction::Navigate(KeyCode::Down));

        assert!(result.redraw);
        assert_eq!(app_state.ui.selected_torrent_index, 1);
        assert_eq!(app_state.ui.selected_peer_index, 0);
    }

    #[test]
    fn reducer_toggle_anonymize_names_flips_flag() {
        let mut app_state = AppState::default();
        assert!(!app_state.anonymize_torrent_names);

        reduce_ui_action(&mut app_state, UiAction::ToggleAnonymizeNames);
        assert!(app_state.anonymize_torrent_names);

        reduce_ui_action(&mut app_state, UiAction::ToggleAnonymizeNames);
        assert!(!app_state.anonymize_torrent_names);
    }

    #[test]
    fn reducer_enter_power_saving_sets_mode() {
        let mut app_state = AppState {
            mode: AppMode::Normal,
            ..Default::default()
        };

        reduce_ui_action(&mut app_state, UiAction::EnterPowerSaving);

        assert!(matches!(app_state.mode, AppMode::PowerSaving));
    }

    #[test]
    fn reducer_request_quit_sets_flag() {
        let mut app_state = AppState::default();
        assert!(!app_state.should_quit);

        reduce_ui_action(&mut app_state, UiAction::RequestQuit);

        assert!(app_state.should_quit);
    }

    #[test]
    fn reducer_graph_actions_cycle_mode() {
        let mut app_state = AppState::default();
        let initial = app_state.graph_mode;

        reduce_ui_action(&mut app_state, UiAction::GraphNext);
        assert_eq!(app_state.graph_mode, initial.next());

        reduce_ui_action(&mut app_state, UiAction::GraphPrev);
        assert_eq!(app_state.graph_mode, initial);
    }

    #[test]
    fn reducer_open_add_torrent_browser_emits_effect() {
        let mut app_state = AppState::default();

        let result = reduce_ui_action(&mut app_state, UiAction::OpenAddTorrentBrowser);

        assert!(result.redraw);
        assert_eq!(result.effects, vec![UiEffect::OpenAddTorrentFileBrowser]);
    }

    #[test]
    fn reducer_open_delete_confirm_sets_mode_and_payload() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 1;

        let result = reduce_ui_action(
            &mut app_state,
            UiAction::OpenDeleteConfirm { with_files: true },
        );

        assert!(result.redraw);
        assert!(matches!(app_state.mode, AppMode::DeleteConfirm));
        assert_eq!(app_state.ui.delete_confirm.info_hash, b"hash_b".to_vec());
        assert!(app_state.ui.delete_confirm.with_files);
    }

    #[test]
    fn reducer_open_delete_confirm_is_noop_when_no_selection() {
        let mut app_state = AppState::default();
        app_state.ui.selected_torrent_index = 0;

        let result = reduce_ui_action(
            &mut app_state,
            UiAction::OpenDeleteConfirm { with_files: false },
        );

        assert!(result.redraw);
        assert!(matches!(app_state.mode, AppMode::Normal));
    }

    #[test]
    fn reducer_open_config_emits_effect() {
        let mut app_state = AppState::default();

        let result = reduce_ui_action(&mut app_state, UiAction::OpenConfig);

        assert!(result.redraw);
        assert_eq!(result.effects, vec![UiEffect::OpenConfigScreen]);
    }

    #[test]
    fn reducer_data_rate_actions_update_rate_and_emit_effect() {
        let mut app_state = AppState {
            data_rate: DataRate::Rate1s,
            ..Default::default()
        };

        let slower = reduce_ui_action(&mut app_state, UiAction::DataRateSlower);
        assert_eq!(app_state.data_rate.as_ms(), DataRate::RateHalf.as_ms());
        assert_eq!(
            slower.effects,
            vec![UiEffect::BroadcastManagerDataRate(
                DataRate::RateHalf.as_ms()
            )]
        );

        let faster = reduce_ui_action(&mut app_state, UiAction::DataRateFaster);
        assert_eq!(app_state.data_rate.as_ms(), DataRate::Rate1s.as_ms());
        assert_eq!(
            faster.effects,
            vec![UiEffect::BroadcastManagerDataRate(DataRate::Rate1s.as_ms())]
        );
    }

    #[test]
    fn reducer_theme_actions_emit_effects() {
        let mut app_state = AppState::default();

        let prev = reduce_ui_action(&mut app_state, UiAction::ThemePrev);
        let next = reduce_ui_action(&mut app_state, UiAction::ThemeNext);

        assert_eq!(prev.effects, vec![UiEffect::ApplyThemePrev]);
        assert_eq!(next.effects, vec![UiEffect::ApplyThemeNext]);
    }

    #[test]
    fn reducer_toggle_pause_selected_toggles_state_and_emits_command_effect() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0;
        let hash = b"hash_a".to_vec();

        if let Some(t) = app_state.torrents.get_mut(&hash) {
            t.latest_state.torrent_control_state = TorrentControlState::Running;
        }

        let paused = reduce_ui_action(&mut app_state, UiAction::TogglePauseSelected);
        assert_eq!(paused.effects, vec![UiEffect::SendPause(hash.clone())]);
        assert_eq!(
            app_state
                .torrents
                .get(&hash)
                .expect("selected torrent exists")
                .latest_state
                .torrent_control_state,
            TorrentControlState::Paused
        );

        let resumed = reduce_ui_action(&mut app_state, UiAction::TogglePauseSelected);
        assert_eq!(resumed.effects, vec![UiEffect::SendResume(hash.clone())]);
        assert_eq!(
            app_state
                .torrents
                .get(&hash)
                .expect("selected torrent exists")
                .latest_state
                .torrent_control_state,
            TorrentControlState::Running
        );
    }

    #[test]
    fn reducer_sort_by_selected_column_updates_torrent_sort() {
        let mut app_state = create_test_app_state();
        app_state.screen_area = Rect::new(0, 0, 220, 80);
        app_state.ui.selected_header = SelectedHeader::Torrent(1);
        app_state.torrent_sort = (TorrentSortColumn::Down, SortDirection::Descending);

        if let Some(t) = app_state.torrents.get_mut("hash_a".as_bytes()) {
            t.latest_state.number_of_pieces_total = 10;
            t.latest_state.number_of_pieces_completed = 5;
            t.smoothed_download_speed_bps = 100;
            t.smoothed_upload_speed_bps = 50;
        }
        if let Some(t) = app_state.torrents.get_mut("hash_b".as_bytes()) {
            t.latest_state.number_of_pieces_total = 10;
            t.latest_state.number_of_pieces_completed = 10;
            t.smoothed_download_speed_bps = 200;
            t.smoothed_upload_speed_bps = 100;
        }

        let _ = reduce_ui_action(&mut app_state, UiAction::SortBySelectedColumn);

        assert_eq!(app_state.torrent_sort.0, TorrentSortColumn::Name);
        assert_eq!(app_state.torrent_sort.1, SortDirection::Descending);
    }

    #[test]
    fn reducer_sort_by_selected_column_updates_peer_sort() {
        let mut app_state = create_test_app_state();
        app_state.screen_area = Rect::new(0, 0, 220, 80);
        app_state.ui.selected_torrent_index = 0;
        app_state.ui.selected_header = SelectedHeader::Peer(0);
        app_state.peer_sort = (PeerSortColumn::Address, SortDirection::Ascending);

        let _ = reduce_ui_action(&mut app_state, UiAction::SortBySelectedColumn);

        assert_eq!(app_state.peer_sort.0, PeerSortColumn::Flags);
        assert_eq!(app_state.peer_sort.1, SortDirection::Descending);
    }

    #[test]
    fn reducer_open_help_emits_help_effect() {
        let mut app_state = create_test_app_state();
        let out = reduce_ui_action(&mut app_state, UiAction::OpenHelp);
        assert!(out.redraw);
        assert_eq!(out.effects, vec![UiEffect::OpenHelpScreen]);
    }

    #[test]
    fn reducer_paste_text_emits_paste_effect() {
        let mut app_state = create_test_app_state();
        let out = reduce_ui_action(
            &mut app_state,
            UiAction::PasteText("magnet:?xt=urn:btih:test".to_string()),
        );
        assert!(out.redraw);
        assert_eq!(
            out.effects,
            vec![UiEffect::HandlePastedText(
                "magnet:?xt=urn:btih:test".to_string()
            )]
        );
    }
}
