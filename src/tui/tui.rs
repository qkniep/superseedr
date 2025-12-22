// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::symbols::Marker;
use ratatui::{prelude::*, symbols, widgets::*};

use crate::tui::tui_formatters::*;
use crate::tui::layout::{get_torrent_columns, ColumnId};

use crate::app::{AppMode, AppState, ConfigItem, SelectedHeader, TorrentControlState};
use crate::app::PeerInfo;
use crate::app::GraphDisplayMode;

// Import the new Layout System
// Make sure `pub mod layout;` is in your main.rs/lib.rs!
use crate::tui::layout::{
    calculate_layout, compute_smart_table_layout, LayoutContext, SmartCol,
};

use throbber_widgets_tui::Throbber;
use crate::config::get_app_paths;
use crate::config::{PeerSortColumn, Settings, SortDirection, TorrentSortColumn};
use strum::IntoEnumIterator;
use crate::theme;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::{SystemTime, UNIX_EPOCH};

static APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const SECONDS_HISTORY_MAX: usize = 3600; 
pub const MINUTES_HISTORY_MAX: usize = 48 * 60; 

pub fn draw(f: &mut Frame, app_state: &AppState, settings: &Settings) {
    let area = f.area();

    // --- 1. OVERLAYS (Block everything else) ---
    if app_state.show_help {
        draw_help_popup(f, app_state, &app_state.mode);
        return;
    }

    match &app_state.mode {
        AppMode::Welcome => { draw_welcome_screen(f); return; }
        AppMode::PowerSaving => { draw_power_saving_screen(f, app_state, settings); return; }
        AppMode::ConfigPathPicker { file_explorer, for_item, .. } => {
            let title = format!("Select a Folder - {:?}", for_item);
            draw_file_picker(f, file_explorer, title);
            return;
        }
        AppMode::Config { settings_edit, selected_index, items, editing } => {
            draw_config_screen(f, settings_edit, *selected_index, items, editing);
            return;
        }
        AppMode::DeleteConfirm { .. } => {
            draw_delete_confirm_dialog(f, app_state);
            return;
        }
        AppMode::DownloadPathPicker(file_explorer) => {
            draw_file_picker(f, file_explorer, "Select Download Folder".to_string());
            return;
        }
        _ => {}
    }

    // --- 2. CALCULATE LAYOUT ---
    // We assume 35% for sidebar if not in settings yet.
    let ctx = LayoutContext::new(area, app_state, 35);
    let plan = calculate_layout(area, &ctx);

    // --- 3. RENDER THE PLAN ---

    // A. Core Widgets
    draw_torrent_list(f, app_state, plan.list);
    draw_footer(f, app_state, settings, plan.footer);
    
    // Note: The planner splits details into "Text" and "Table" (peers).
    draw_details_panel(f, app_state, plan.details);

    draw_peers_table(f, app_state, plan.peers);

    // B. Optional Widgets
    if let Some(r) = plan.chart {
        draw_network_chart(f, app_state, r);
    }

    if let Some(r) = plan.sparklines {
        draw_torrent_sparklines(f, app_state, r);
    }
    
    if let Some(r) = plan.peer_stream {
        draw_peer_stream(f, app_state, r);
    }

    if let Some(r) = plan.block_stream {
        draw_vertical_block_stream(f, app_state, r);
    }

    if let Some(r) = plan.stats {
        draw_stats_panel(f, app_state, settings, r);
    }

    // C. Warnings / Popups
    if let Some(msg) = plan.warning_message {
         f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(theme::RED).bg(theme::SURFACE0)), 
            plan.list 
        );
    }

    if let Some(error_text) = &app_state.system_error {
        draw_status_error_popup(f, error_text);
    }

    if app_state.should_quit {
        draw_shutdown_screen(f, app_state);
    }
}

fn draw_file_picker(f: &mut Frame, file_explorer: &ratatui_explorer::FileExplorer, title: String) {
    let area = centered_rect(80, 70, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(theme::MAUVE)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
    let footer_text = Line::from(vec![
        Span::styled("[Tab]", Style::default().fg(theme::GREEN)), Span::raw(" Confirm | "),
        Span::styled("[Esc]", Style::default().fg(theme::RED)), Span::raw(" Cancel | "),
        Span::styled("←→↑↓", Style::default().fg(theme::BLUE)), Span::raw(" Navigate"),
    ]).alignment(Alignment::Center);
    f.render_widget(block, area);
    f.render_widget(&file_explorer.widget(), chunks[0]);
    f.render_widget(Paragraph::new(footer_text).style(Style::default().fg(theme::SUBTEXT1)), chunks[1]);
}

fn draw_torrent_list(f: &mut Frame, app_state: &AppState, area: Rect) {
    let mut table_state = TableState::default();
    if matches!(app_state.selected_header, SelectedHeader::Torrent(_)) {
        table_state.select(Some(app_state.selected_torrent_index));
    }

    // 1. GET COLUMNS FROM SOURCE OF TRUTH
    let all_cols = get_torrent_columns();

    // 2. Convert to SmartCol for the Layout Engine
    let smart_cols: Vec<SmartCol> = all_cols.iter().map(|c| SmartCol {
        header: c.header,
        min_width: c.min_width,
        priority: c.priority,
        constraint: c.default_constraint,
    }).collect();

    // 3. Calculate Layout
    let (constraints, visible_indices) = compute_smart_table_layout(&smart_cols, area.width, 1);

    // 4. Build Header
    let (sort_col, sort_dir) = app_state.torrent_sort;

    let header_cells: Vec<Cell> = visible_indices.iter().enumerate().map(|(visual_idx, &real_idx)| {
        let def = &all_cols[real_idx];
        
        // Highlight logic
        let is_selected = app_state.selected_header == SelectedHeader::Torrent(visual_idx);
        
        // Sorting Logic: Use the definition directly
        let is_sorting = def.sort_enum.map_or(false, |s| s == sort_col);

        let mut style = Style::default().fg(theme::YELLOW);
        if is_sorting { style = style.fg(theme::MAUVE); }

        let mut spans = vec![];
        let mut text_span = Span::styled(def.header, style);
        if is_selected { text_span = text_span.underlined().bold(); }
        spans.push(text_span);

        if is_sorting {
            let arrow = if sort_dir == SortDirection::Ascending { " ▲" } else { " ▼" };
            spans.push(Span::styled(arrow, style));
        }
        Cell::from(Line::from(spans))
    }).collect();

    let header = Row::new(header_cells).height(1);

    // 5. Build Rows
    let rows = app_state.torrent_list_order.iter().enumerate().map(|(i, info_hash)| {
        match app_state.torrents.get(info_hash) {
            Some(torrent) => {
                let state = &torrent.latest_state;
                let is_selected = i == app_state.selected_torrent_index;

                let mut row_style = match state.torrent_control_state {
                    TorrentControlState::Running => Style::default().fg(theme::TEXT),
                    TorrentControlState::Paused => Style::default().fg(theme::SURFACE1),
                    TorrentControlState::Deleting => Style::default().fg(theme::RED),
                };
                if is_selected {
                    row_style = row_style.add_modifier(Modifier::BOLD);
                }

                // Map visible indices back to the data
                let cells: Vec<Cell> = visible_indices.iter().map(|&real_idx| {
                    let def = &all_cols[real_idx];
                    
                    match def.id {
                        ColumnId::Status => {
                            let p = if state.number_of_pieces_total > 0 {
                                (state.number_of_pieces_completed as f64 / state.number_of_pieces_total as f64) * 100.0
                            } else { 0.0 };
                            Cell::from(format!("{:.1}%", p))
                        },
                        ColumnId::Name => {
                             let name = if app_state.anonymize_torrent_names {
                                format!("Torrent {}", i + 1)
                            } else {
                                state.torrent_name.clone()
                            };
                            let mut c = Cell::from(name);
                            if is_selected { c = c.style(Style::default().fg(theme::YELLOW)); }
                            c
                        },
                        ColumnId::DownSpeed => {
                            Cell::from(format_speed(torrent.smoothed_download_speed_bps))
                                .style(speed_to_style(torrent.smoothed_download_speed_bps))
                        },
                        ColumnId::UpSpeed => {
                            Cell::from(format_speed(torrent.smoothed_upload_speed_bps))
                                .style(speed_to_style(torrent.smoothed_upload_speed_bps))
                        },
                    }
                }).collect();

                Row::new(cells).style(row_style)
            }
            None => Row::new(vec![Cell::from("Error retrieving data")]),
        }
    });

    let border_style = if matches!(app_state.selected_header, SelectedHeader::Torrent(_)) {
        Style::default().fg(theme::MAUVE)
    } else {
        Style::default().fg(theme::SURFACE2)
    };

    let mut title_spans = Vec::new();
    if app_state.is_searching {
        title_spans.push(Span::raw("Search: /"));
        title_spans.push(Span::styled(&app_state.search_query, Style::default().fg(theme::YELLOW)));
    } else if !app_state.search_query.is_empty() {
        title_spans.push(Span::styled(format!("[{}] ", app_state.search_query), Style::default().fg(theme::SUBTEXT1).add_modifier(Modifier::ITALIC)));
    }
    
    if !app_state.is_searching {
        if let Some(info_hash) = app_state.torrent_list_order.get(app_state.selected_torrent_index) {
            if let Some(torrent) = app_state.torrents.get(info_hash) {
                let name = if app_state.anonymize_torrent_names { "Torrent..." } else { &torrent.latest_state.torrent_name };
                let avail_width = area.width.saturating_sub(10) as usize;
                let display_name = truncate_with_ellipsis(name, avail_width);
                title_spans.push(Span::styled(display_name, Style::default().fg(theme::YELLOW)));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Line::from(title_spans));

    let table = Table::new(rows, constraints)
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_stateful_widget(table, area, &mut table_state);
}


fn draw_network_chart(f: &mut Frame, app_state: &AppState, chart_chunk: Rect) {
    if chart_chunk.width < 5 || chart_chunk.height < 5 { return; }

    let smooth_data = |data: &[u64], alpha: f64| -> Vec<u64> {
        if data.is_empty() { return Vec::new(); }
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
        | GraphDisplayMode::TwentyFourHours => {
            (
                &app_state.minute_avg_dl_history,
                &app_state.minute_avg_ul_history,
                &app_state.minute_disk_backoff_history_ms, 
                MINUTES_HISTORY_MAX, 
                60,
            )
        }
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

    let backoff_dataset = Dataset::default()
        .name("File Limits") 
        .marker(Marker::Braille) 
        .graph_type(GraphType::Scatter) 
        .style(Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)) 
        .data(&backoff_marker_data);

    let datasets = vec![
        Dataset::default() 
            .name("Download")
            .marker(Marker::Braille)
            .style(
                Style::default()
                    .fg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&dl_data),
        Dataset::default() 
            .name("Upload")
            .marker(Marker::Braille)
            .style(
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&ul_data),
        backoff_dataset, 
    ];

    let y_speed_axis_labels = vec![
        Span::raw("0"),
        Span::styled(
            format_speed(nice_max_speed / 2),
            Style::default().fg(theme::SUBTEXT0),
        ),
        Span::styled(
            format_speed(nice_max_speed),
            Style::default().fg(theme::SUBTEXT0),
        ),
    ];
    let x_labels = generate_x_axis_labels(app_state.graph_mode);

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
        Style::default().fg(theme::PEACH),
    )];
    for (i, &mode) in all_modes.iter().enumerate() {
        let is_active = mode == app_state.graph_mode;
        let mode_str = mode.to_string();

        let style = if is_active {
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::SURFACE0)
        };

        title_spans.push(Span::styled(mode_str, style));

        if i < all_modes.len().saturating_sub(1) {
            title_spans.push(Span::styled(" ", Style::default().fg(theme::SURFACE2)));
        }
    }
    let chart_title = Line::from(title_spans);

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(chart_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE2)),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(theme::OVERLAY0))
                .bounds([0.0, points_to_show.saturating_sub(1) as f64]) 
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(theme::OVERLAY0))
                .bounds([0.0, nice_max_speed as f64]) 
                .labels(y_speed_axis_labels),
        )
        .legend_position(Some(LegendPosition::TopRight));

    f.render_widget(chart, chart_chunk);
}

fn draw_stats_panel(f: &mut Frame, app_state: &AppState, settings: &Settings, stats_chunk: Rect) {
    let total_peers = app_state
        .torrents
        .values()
        .map(|t| t.latest_state.number_of_successfully_connected_peers)
        .sum::<usize>();

    let dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
    let dl_limit = settings.global_download_limit_bps;

    let mut dl_spans = vec![
        Span::styled("DL Speed: ", Style::default().fg(theme::SKY).bold()),
        Span::styled(
            format_speed(dl_speed),
            Style::default().fg(theme::SKY).bold(),
        ),
        Span::raw(" / "),
    ];
    if dl_limit > 0 && dl_speed >= dl_limit {
        dl_spans.push(Span::styled(
            format_limit_bps(dl_limit),
            Style::default().fg(theme::RED),
        ));
    } else {
        dl_spans.push(Span::styled(
            format_limit_bps(dl_limit),
            Style::default().fg(theme::SUBTEXT0),
        ));
    }

    let ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);
    let ul_limit = settings.global_upload_limit_bps;

    let mut ul_spans = vec![
        Span::styled("UL Speed: ", Style::default().fg(theme::GREEN).bold()),
        Span::styled(
            format_speed(ul_speed),
            Style::default().fg(theme::GREEN).bold(),
        ),
        Span::raw(" / "),
    ];

    if ul_limit > 0 && ul_speed >= ul_limit {
        ul_spans.push(Span::styled(
            format_limit_bps(ul_limit),
            Style::default().fg(theme::RED),
        ));
    } else {
        ul_spans.push(Span::styled(
            format_limit_bps(ul_limit),
            Style::default().fg(theme::SUBTEXT0),
        ));
    }

    let thrash_text: String;
    let thrash_style: Style;

    let baseline_val = app_state.adaptive_max_scpb;
    let thrash_score_val = app_state.global_disk_thrash_score;
    let thrash_score_str = format!("{:.0}", thrash_score_val);

    if thrash_score_val < 0.01 {
        thrash_text = format!("- ({})", thrash_score_str);
        thrash_style = Style::default().fg(theme::SUBTEXT0);
    } else if baseline_val == 0.0 {
        thrash_text = format!("∞ ({})", thrash_score_str);
        thrash_style = Style::default().fg(theme::RED).bold();
    } else {
        let diff = thrash_score_val - baseline_val;
        let thrash_percentage = (diff / baseline_val) * 100.0;

        if thrash_percentage > -0.01 && thrash_percentage < 0.01 {
            thrash_text = format!("0.0% ({})", thrash_score_str);
            thrash_style = Style::default().fg(theme::TEXT);
        } else {
            thrash_text = format!("{:+.1}% ({})", thrash_percentage, thrash_score_str);
            if thrash_percentage > 15.0 {
                thrash_style = Style::default().fg(theme::RED).bold();
            } else if thrash_percentage > 0.0 {
                thrash_style = Style::default().fg(theme::YELLOW);
            } else {
                thrash_style = Style::default().fg(theme::GREEN);
            }
        }
    }

    let stats_text = vec![
        Line::from(vec![
            Span::styled("Run Time: ", Style::default().fg(theme::TEAL)),
            Span::raw(format_time(app_state.run_time)),
        ]),
        Line::from(vec![
            Span::styled("Torrents: ", Style::default().fg(theme::PEACH)),
            Span::raw(app_state.torrents.len().to_string()),
        ]),
        Line::from(""),
        Line::from(dl_spans),
        Line::from(vec![
            Span::styled("Session DL: ", Style::default().fg(theme::SKY)),
            Span::raw(format_bytes(app_state.session_total_downloaded)),
        ]),
        Line::from(vec![
            Span::styled("Lifetime DL: ", Style::default().fg(theme::SKY)),
            Span::raw(format_bytes(
                app_state.lifetime_downloaded_from_config + app_state.session_total_downloaded,
            )),
        ]),
        Line::from(""),
        Line::from(ul_spans),
        Line::from(vec![
            Span::styled("Session UL: ", Style::default().fg(theme::GREEN)),
            Span::raw(format_bytes(app_state.session_total_uploaded)),
        ]),
        Line::from(vec![
            Span::styled("Lifetime UL: ", Style::default().fg(theme::GREEN)),
            Span::raw(format_bytes(
                app_state.lifetime_uploaded_from_config + app_state.session_total_uploaded,
            )),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("CPU: ", Style::default().fg(theme::RED)),
            Span::raw(format!("{:.1}%", app_state.cpu_usage)),
        ]),
        Line::from(vec![
            Span::styled("RAM: ", Style::default().fg(theme::YELLOW)),
            Span::raw(format!("{:.1}%", app_state.ram_usage_percent)),
        ]),
        Line::from(vec![
            Span::styled("App RAM: ", Style::default().fg(theme::FLAMINGO)),
            Span::raw(format_memory(app_state.app_ram_usage)),
        ]),
        Line::from(vec![
            Span::styled("Disk    ", Style::default().fg(theme::TEXT)),
            Span::styled("↑ ", Style::default().fg(theme::GREEN)), 
            Span::styled(
                format!("{:<12}", format_speed(app_state.avg_disk_read_bps)),
                Style::default().fg(theme::GREEN),
            ),
            Span::styled("↓ ", Style::default().fg(theme::SKY)), 
            Span::styled(
                format_speed(app_state.avg_disk_write_bps),
                Style::default().fg(theme::SKY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Seek    ", Style::default().fg(theme::TEXT)),
            Span::styled("↑ ", Style::default().fg(theme::GREEN)), 
            Span::styled(
                format!(
                    "{:<12}",
                    format_bytes(app_state.global_disk_read_thrash_score)
                ),
                Style::default().fg(theme::GREEN),
            ),
            Span::styled("↓ ", Style::default().fg(theme::SKY)), 
            Span::styled(
                format_bytes(app_state.global_disk_write_thrash_score),
                Style::default().fg(theme::SKY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Latency ", Style::default().fg(theme::TEXT)),
            Span::styled("↑ ", Style::default().fg(theme::GREEN)), 
            Span::styled(
                format!("{:<12}", format_latency(app_state.avg_disk_read_latency)),
                Style::default().fg(theme::GREEN),
            ),
            Span::styled("↓ ", Style::default().fg(theme::SKY)), 
            Span::styled(
                format_latency(app_state.avg_disk_write_latency),
                Style::default().fg(theme::SKY),
            ),
        ]),
        Line::from(vec![
            Span::styled("IOPS    ", Style::default().fg(theme::TEXT)),
            Span::styled("↑ ", Style::default().fg(theme::GREEN)), 
            Span::styled(
                format!("{:<12}", format_iops(app_state.read_iops)),
                Style::default().fg(theme::GREEN),
            ),
            Span::styled("↓ ", Style::default().fg(theme::SKY)), 
            Span::styled(
                format_iops(app_state.write_iops),
                Style::default().fg(theme::SKY),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Next Tuning in: ", Style::default().fg(theme::TEXT)),
            Span::raw(format!("{}s", app_state.tuning_countdown)),
        ]),
        Line::from(vec![
            Span::styled("Disk Thrash: ", Style::default().fg(theme::TEAL)),
            Span::styled(thrash_text, thrash_style),
        ]),
        Line::from(vec![
            Span::styled("Reserve Pool:  ", Style::default().fg(theme::TEAL)), 
            Span::raw(app_state.limits.reserve_permits.to_string()),
            format_limit_delta(
                app_state.limits.reserve_permits,
                app_state.last_tuning_limits.reserve_permits,
            ),
        ]),
        {
            let mut spans = format_permits_spans(
                "Peer Slots: ",
                total_peers,
                app_state.limits.max_connected_peers,
                theme::MAUVE,
            );
            spans.push(format_limit_delta(
                app_state.limits.max_connected_peers,
                app_state.last_tuning_limits.max_connected_peers,
            ));
            Line::from(spans)
        },
        Line::from(vec![
            Span::styled("Disk Reads:    ", Style::default().fg(theme::GREEN)),
            Span::raw(app_state.limits.disk_read_permits.to_string()),
            format_limit_delta(
                app_state.limits.disk_read_permits,
                app_state.last_tuning_limits.disk_read_permits,
            ),
        ]),
        Line::from(vec![
            Span::styled("Disk Writes:   ", Style::default().fg(theme::SKY)),
            Span::raw(app_state.limits.disk_write_permits.to_string()),
            format_limit_delta(
                app_state.limits.disk_write_permits,
                app_state.last_tuning_limits.disk_write_permits,
            ),
        ]),
    ];

    let stats_paragraph = Paragraph::new(stats_text)
        .block(
            Block::default()
                .title(Span::styled("Stats", Style::default().fg(Color::White)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE2)),
        )
        .style(Style::default().fg(theme::TEXT));

    f.render_widget(stats_paragraph, stats_chunk);
}

fn draw_details_panel(
    f: &mut Frame,
    app_state: &AppState,
    details_text_chunk: Rect,
) {
    if let Some(info_hash) = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
    {
        if let Some(torrent) = app_state.torrents.get(info_hash) {
            let state = &torrent.latest_state;

            let details_block = Block::default()
                .title(Span::styled("Details", Style::default().fg(theme::MAUVE)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE2));
            let details_inner_chunk = details_block.inner(details_text_chunk);
            f.render_widget(details_block, details_text_chunk);

            let detail_rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(details_inner_chunk);

            let progress_chunks = Layout::horizontal([Constraint::Length(11), Constraint::Min(0)])
                .split(detail_rows[0]);

            f.render_widget(Paragraph::new("Progress: "), progress_chunks[0]);

            let (progress_ratio, progress_label_text) = if state.number_of_pieces_total > 0 {
                let ratio =
                    state.number_of_pieces_completed as f64 / state.number_of_pieces_total as f64;
                (ratio, format!("{:.1}%", ratio * 100.0))
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
                .filled_style(Style::default().fg(theme::GREEN));
            f.render_widget(line_gauge, progress_chunks[1]);

            let status_text = if state.activity_message.is_empty() {
                "Waiting..."
            } else {
                state.activity_message.as_str()
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Status:   ", Style::default().fg(theme::TEXT)),
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
                    Span::styled("Peers:    ", Style::default().fg(theme::TEXT)),
                    Span::raw(format!(
                        "{} (",
                        state.number_of_successfully_connected_peers
                    )),
                    Span::styled(format!("{}", seeds), Style::default().fg(theme::GREEN)),
                    Span::raw(" / "),
                    Span::styled(format!("{}", leeches), Style::default().fg(theme::RED)),
                    Span::raw(")"),
                ])),
                detail_rows[2],
            );

            let written_size_spans =
                if state.number_of_pieces_completed < state.number_of_pieces_total {
                    vec![
                        Span::styled("Written:  ", Style::default().fg(theme::TEXT)),
                        Span::raw(format_bytes(state.bytes_written)),
                        Span::raw(format!(" / {}", format_bytes(state.total_size))),
                    ]
                } else {
                    vec![
                        Span::styled("Size:     ", Style::default().fg(theme::TEXT)),
                        Span::raw(format_bytes(state.total_size)),
                    ]
                };
            f.render_widget(
                Paragraph::new(Line::from(written_size_spans)),
                detail_rows[3],
            );

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Pieces:   ", Style::default().fg(theme::TEXT)),
                    Span::raw(format!(
                        "{}/{}",
                        state.number_of_pieces_completed, state.number_of_pieces_total
                    )),
                ])),
                detail_rows[4],
            );

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("ETA:      ", Style::default().fg(theme::TEXT)),
                    Span::raw(format_duration(state.eta)),
                ])),
                detail_rows[5],
            );

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Announce: ", Style::default().fg(theme::TEXT)),
                    Span::raw(format_countdown(state.next_announce_in)),
                ])),
                detail_rows[6],
            );
        }
    }
}

fn draw_peers_table(
    f: &mut Frame,
    app_state: &AppState,
    peers_chunk: Rect,
) {

    if peers_chunk.height < 2 || peers_chunk.width < 2 { return; }

    if let Some(info_hash) = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
    {
        if let Some(torrent) = app_state.torrents.get(info_hash) {
            let state = &torrent.latest_state;

            // PEERS TABLE (Only render if height > 0 to be safe)
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
                    let ordering = match sort_by {
                        PeerSortColumn::Flags => {
                            let mut a_score = 0;
                            if !a.peer_choking { a_score += 2; }
                            if !a.am_choking { a_score += 1; }
                            let mut b_score = 0;
                            if !b.peer_choking { b_score += 2; }
                            if !b.am_choking { b_score += 1; }
                            b_score.cmp(&a_score)
                        }
                        PeerSortColumn::Completed => {
                            let total_pieces = state.number_of_pieces_total as usize;
                            if total_pieces == 0 { return std::cmp::Ordering::Equal; }
                            let a_completed = a.bitfield.iter().take(total_pieces).filter(|&&h| h).count();
                            let a_percent = a_completed as f64 / total_pieces as f64;
                            let b_completed = b.bitfield.iter().take(total_pieces).filter(|&&h| h).count();
                            let b_percent = b_completed as f64 / total_pieces as f64;
                            b_percent.total_cmp(&a_percent)
                        }
                        PeerSortColumn::Address => a.address.cmp(&b.address),
                        PeerSortColumn::Client => a.peer_id.cmp(&b.peer_id),
                        PeerSortColumn::Action => a.last_action.cmp(&b.last_action),
                        PeerSortColumn::DL => {
                            a.download_speed_bps.cmp(&b.download_speed_bps)
                                .then(b.upload_speed_bps.cmp(&a.upload_speed_bps))
                                .then(a.total_downloaded.cmp(&b.total_downloaded))
                        }
                        PeerSortColumn::UL => {
                            a.upload_speed_bps.cmp(&b.upload_speed_bps)
                                .then(b.download_speed_bps.cmp(&a.download_speed_bps))
                                .then(a.total_uploaded.cmp(&b.total_uploaded))
                        }
                    };

                    if sort_direction == SortDirection::Ascending { ordering } else { ordering.reverse() }
                });

                let peer_border_style = if matches!(app_state.selected_header, SelectedHeader::Peer(_)) {
                    Style::default().fg(theme::MAUVE)
                } else {
                    Style::default().fg(theme::SURFACE2)
                };

                if peers_to_display.is_empty() {
                    draw_swarm_heatmap(f, &state.peers, state.number_of_pieces_total, peers_chunk);
                } else {
                    let peer_header_cells = PeerSortColumn::iter().enumerate().map(|(i, h)| {
                        let is_selected = app_state.selected_header == SelectedHeader::Peer(i);
                        let (sort_col, sort_dir) = app_state.peer_sort;
                        let is_sorting_by_this = sort_col == h;
                        let mut style = Style::default().fg(theme::YELLOW);
                        let text = match h {
                            PeerSortColumn::Flags => "Flags",
                            PeerSortColumn::Address => "Address",
                            PeerSortColumn::Client => "Client",
                            PeerSortColumn::Action => "Action",
                            PeerSortColumn::Completed => "Done %",
                            PeerSortColumn::UL => "Up (Total)",
                            PeerSortColumn::DL => "Down (Total)",
                        };

                        let mut text_with_indicator = text.to_string();
                        if is_sorting_by_this {
                            style = style.fg(theme::MAUVE);
                            let indicator = if sort_dir == SortDirection::Ascending { " ▲" } else { " ▼" };
                            text_with_indicator.push_str(indicator);
                        }
                        let mut text_span = Span::styled(text, style);
                        if is_selected { text_span = text_span.underlined().bold(); }
                        let mut spans = vec![text_span];
                        if is_sorting_by_this {
                            let indicator = if sort_dir == SortDirection::Ascending { " ▲" } else { " ▼" };
                            spans.push(Span::styled(indicator, style));
                        }
                        Cell::from(Line::from(spans))
                    });
                    let peer_header = Row::new(peer_header_cells).height(1);

                    let peer_rows = peers_to_display.iter().map(|peer| {
                        let row_color = if peer.download_speed_bps == 0 && peer.upload_speed_bps == 0 {
                            theme::SURFACE1
                        } else {
                            ip_to_color(&peer.address)
                        };

                        let flags_spans = Line::from(vec![
                            Span::styled("■", Style::default().fg(if peer.am_interested { theme::SAPPHIRE } else { theme::SURFACE1 })),
                            Span::styled("■", Style::default().fg(if peer.peer_choking { theme::MAROON } else { theme::SURFACE1 })),
                            Span::styled("■", Style::default().fg(if peer.peer_interested { theme::TEAL } else { theme::SURFACE1 })),
                            Span::styled("■", Style::default().fg(if peer.am_choking { theme::PEACH } else { theme::SURFACE1 })),
                        ]);

                        let total_pieces_from_torrent = state.number_of_pieces_total as usize;
                        let percentage = if total_pieces_from_torrent > 0 {
                            let completed_pieces = peer.bitfield.iter().take(total_pieces_from_torrent).filter(|&&have| have).count();
                            if completed_pieces == total_pieces_from_torrent {
                                100.0
                            } else {
                                (completed_pieces as f64 / total_pieces_from_torrent as f64) * 100.0
                            }
                        } else {
                            0.0
                        };

                        let display_address = if app_state.anonymize_torrent_names {
                             "xxx.xxx.xxx.xxx".to_string()
                        } else {
                            peer.address.clone()
                        };

                        Row::new(vec![
                            Cell::from(flags_spans),
                            Cell::from(format!("{:.1}%", percentage)),
                            Cell::from(display_address), 
                            Cell::from(parse_peer_id(&peer.peer_id)),
                            Cell::from(peer.last_action.clone()),
                            Cell::from(format!("{} ({})", format_speed(peer.upload_speed_bps), format_bytes(peer.total_uploaded))),
                            Cell::from(format!("{} ({})", format_speed(peer.download_speed_bps), format_bytes(peer.total_downloaded))),
                        ]).style(Style::default().fg(row_color))
                    });

                    let peer_widths = [
                        Constraint::Length(5),      
                        Constraint::Percentage(5),  
                        Constraint::Percentage(15), 
                        Constraint::Percentage(15), 
                        Constraint::Percentage(20), 
                        Constraint::Percentage(20), 
                        Constraint::Percentage(20), 
                    ];

                    let peers_table = Table::new(peer_rows, peer_widths)
                        .header(peer_header)
                        .block(Block::default());

                    let table_rows_needed: u16 = 1 + peers_to_display.len() as u16;
                    let peer_block_height_needed: u16 = table_rows_needed + 1;

                    let available_height = peers_chunk.height;
                    let remaining_height = available_height.saturating_sub(peer_block_height_needed);
                    const MIN_HEATMAP_HEIGHT: u16 = 4;

                    let peers_block = Block::default()
                        .padding(Padding::new(1, 1, 0, 0))
                        .border_style(peer_border_style);

                    if remaining_height >= MIN_HEATMAP_HEIGHT {
                        let layout_chunks = Layout::vertical([
                            Constraint::Length(peer_block_height_needed),
                            Constraint::Min(0),
                        ]).split(peers_chunk);

                        let peers_panel_area = layout_chunks[0];
                        let heatmap_panel_area = layout_chunks[1];

                        let inner_peers_area = peers_block.inner(peers_panel_area);
                        f.render_widget(peers_block, peers_panel_area);
                        f.render_widget(peers_table, inner_peers_area);

                        draw_swarm_heatmap(f, &state.peers, state.number_of_pieces_total, heatmap_panel_area);
                    } else {
                        let inner_peers_area = peers_block.inner(peers_chunk);
                        f.render_widget(peers_block, peers_chunk);
                        f.render_widget(peers_table, inner_peers_area);
                    }
                }
            }
        }
    }
}

fn draw_footer(f: &mut Frame, app_state: &AppState, settings: &Settings, footer_chunk: Rect) {
    let footer_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(60),
            Constraint::Percentage(15),
        ])
        .split(footer_chunk);

    let client_id_chunk = footer_layout[0];
    let _current_dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
    let _current_ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);

    #[cfg(all(feature = "dht", feature = "pex"))]
    let client_display_line = Line::from(vec![
        Span::styled("super", speed_to_style(_current_dl_speed).add_modifier(Modifier::BOLD)),
        Span::styled("seedr", speed_to_style(_current_ul_speed).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{}", APP_VERSION), Style::default().fg(theme::SUBTEXT1)),
        Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
        Span::styled(app_state.data_rate.to_string(), Style::default().fg(theme::YELLOW).bold()),
    ]);

    #[cfg(not(all(feature = "dht", feature = "pex")))]
    let client_display_line = Line::from(vec![
        Span::styled("super", Style::default().fg(theme::SURFACE2)).add_modifier(Modifier::CROSSED_OUT),
        Span::styled("seedr", Style::default().fg(theme::SURFACE2)).add_modifier(Modifier::CROSSED_OUT),
        Span::styled(" [PRIVATE]", Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{}", APP_VERSION), Style::default().fg(theme::SUBTEXT1)),
        Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
        Span::styled(app_state.data_rate.to_string(), Style::default().fg(theme::YELLOW).bold()),
    ]);

    let client_id_paragraph = Paragraph::new(client_display_line)
        .style(Style::default().fg(theme::SUBTEXT1))
        .alignment(Alignment::Left);
    f.render_widget(client_id_paragraph, client_id_chunk);

    let commands_chunk = footer_layout[1];
    let status_chunk = footer_layout[2];

    let help_key = if app_state.system_warning.is_some() {
        vec![
            Span::styled("[m]", Style::default().fg(theme::TEAL)),
            Span::styled("anual/help (warning)", Style::default().fg(theme::YELLOW)),
        ]
    } else {
        vec![
            Span::styled("[m]", Style::default().fg(theme::TEAL)),
            Span::raw("anual/help"),
        ]
    };
    let mut footer_spans = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(theme::BLUE)), Span::raw(" "),
        Span::styled("←→", Style::default().fg(theme::BLUE)), Span::raw(" navigate | "),
        Span::styled("[q]", Style::default().fg(theme::RED)), Span::raw("uit | "),
        Span::styled("[v]", Style::default().fg(theme::TEAL)), Span::raw("paste | "),
        Span::styled("[p]", Style::default().fg(theme::GREEN)), Span::raw("ause/resume | "),
        Span::styled("[d]", Style::default().fg(theme::YELLOW)), Span::raw("elete | "),
        Span::styled("[s]", Style::default().fg(theme::MAUVE)), Span::raw("ort | "),
        Span::styled("[c]", Style::default().fg(theme::LAVENDER)), Span::raw("onfig | "),
        Span::styled("[t]", Style::default().fg(theme::SAPPHIRE)), Span::raw("time | "),
        Span::styled("[/]", Style::default().fg(theme::YELLOW)), Span::raw("search | "),
    ]);
    footer_spans.extend(help_key);

    let footer_keys = footer_spans.alignment(Alignment::Center);
    let footer_paragraph = Paragraph::new(footer_keys).style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(footer_paragraph, commands_chunk);

    let port_style = if app_state.externally_accessable_port { Style::default().fg(theme::GREEN) } else { Style::default().fg(theme::RED) };
    let port_text = if app_state.externally_accessable_port { "Open" } else { "Closed" };

    let footer_status = Line::from(vec![
        Span::raw("Port: "),
        Span::styled(settings.client_port.to_string(), port_style),
        Span::raw(" ["),
        Span::styled(port_text, port_style),
        Span::raw("]"),
    ]).alignment(Alignment::Right);

    let status_paragraph = Paragraph::new(footer_status).style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(status_paragraph, status_chunk);
}

fn draw_peer_stream(f: &mut Frame, app_state: &AppState, area: Rect) {
    if area.height < 3 || area.width < 10 { return; }
    
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let color_discovered = theme::YELLOW;
    let color_connected = theme::TEAL;
    let color_disconnected = theme::MAROON;
    let color_title = theme::SUBTEXT0;
    let color_border = theme::SURFACE2;
    let color_axis = theme::OVERLAY0;

    let y_discovered = 2.0;
    let y_connected = 3.0;
    let y_disconnected = 1.0;

    let small_marker = Marker::Block;
    let medium_marker = Marker::Block;
    let large_marker = Marker::Block;

    let Some(torrent) = selected_torrent else {
        let block = Block::default()
            .title(Span::styled("Peer Stream", Style::default().fg(color_title)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color_border));
        f.render_widget(block, area);
        return;
    };

    let width = area.width.saturating_sub(2).max(1) as usize;

    let disc_history = &torrent.peer_discovery_history;
    let conn_history = &torrent.peer_connection_history;
    let disconn_history = &torrent.peer_disconnect_history;

    let disc_slice = &disc_history[disc_history.len().saturating_sub(width)..];
    let conn_slice = &conn_history[conn_history.len().saturating_sub(width)..];
    let disconn_slice = &disconn_history[disconn_history.len().saturating_sub(width)..];

    let discovered_count: u64 = disc_slice.iter().sum();
    let connected_count: u64 = conn_slice.iter().sum();
    let disconnected_count: u64 = disconn_slice.iter().sum();

    let legend_line = Line::from(vec![
        Span::styled("Connected:", Style::default().fg(color_connected)), Span::raw(connected_count.to_string()), Span::raw(" "),
        Span::styled("Discovered:", Style::default().fg(color_discovered)), Span::raw(discovered_count.to_string()), Span::raw(" "),
        Span::styled("Disconnected:", Style::default().fg(color_disconnected)), Span::raw(disconnected_count.to_string()), Span::raw(" "),
    ]);

    let max_disc = disc_slice.iter().max().copied().unwrap_or(1).max(1) as f64;
    let max_conn = conn_slice.iter().max().copied().unwrap_or(1).max(1) as f64;
    let max_disconn = disconn_slice.iter().max().copied().unwrap_or(1).max(1) as f64;

    let mut disc_data_light = Vec::new();
    let mut disc_data_medium = Vec::new();
    let mut disc_data_dark = Vec::new();

    let mut conn_data_light = Vec::new();
    let mut conn_data_medium = Vec::new();
    let mut conn_data_dark = Vec::new();

    let mut disconn_data_light = Vec::new();
    let mut disconn_data_medium = Vec::new();
    let mut disconn_data_dark = Vec::new();

    for (i, &v) in disc_slice.iter().enumerate() {
        if v == 0 { continue; }
        let norm_val = v as f64 / max_disc;
        let y_val = y_discovered;
        if norm_val < 0.33 { disc_data_light.push((i as f64, y_val)); }
        else if norm_val < 0.66 { disc_data_medium.push((i as f64, y_val)); }
        else { disc_data_dark.push((i as f64, y_val)); }
    }

    for (i, &v) in conn_slice.iter().enumerate() {
        if v == 0 { continue; }
        let norm_val = v as f64 / max_conn;
        let y_val = y_connected;
        if norm_val < 0.33 { conn_data_light.push((i as f64, y_val)); }
        else if norm_val < 0.66 { conn_data_medium.push((i as f64, y_val)); }
        else { conn_data_dark.push((i as f64, y_val)); }
    }

    for (i, &v) in disconn_slice.iter().enumerate() {
        if v == 0 { continue; }
        let norm_val = v as f64 / max_disconn;
        let y_val = y_disconnected;
        if norm_val < 0.33 { disconn_data_light.push((i as f64, y_val)); }
        else if norm_val < 0.66 { disconn_data_medium.push((i as f64, y_val)); }
        else { disconn_data_dark.push((i as f64, y_val)); }
    }

    let datasets = vec![
        Dataset::default().data(&disc_data_light).marker(small_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_discovered).add_modifier(Modifier::DIM)),
        Dataset::default().data(&disc_data_medium).marker(medium_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_discovered)),
        Dataset::default().data(&disc_data_dark).marker(large_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_discovered).add_modifier(Modifier::BOLD)),
        
        Dataset::default().data(&conn_data_light).marker(small_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_connected).add_modifier(Modifier::DIM)),
        Dataset::default().data(&conn_data_medium).marker(medium_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_connected)),
        Dataset::default().data(&conn_data_dark).marker(large_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_connected).add_modifier(Modifier::BOLD)),
        
        Dataset::default().data(&disconn_data_light).marker(small_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_disconnected).add_modifier(Modifier::DIM)),
        Dataset::default().data(&disconn_data_medium).marker(medium_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_disconnected)),
        Dataset::default().data(&disconn_data_dark).marker(large_marker).graph_type(GraphType::Scatter).style(Style::default().fg(color_disconnected).add_modifier(Modifier::BOLD)),
    ];

    let discovery_chart = Chart::new(datasets)
        .block(
            Block::default()
                .title_top(Line::from(Span::styled("Peer Stream", Style::default().fg(color_title))).alignment(Alignment::Left))
                .title_top(legend_line.alignment(Alignment::Right))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color_border)),
        )
        .x_axis(Axis::default().style(Style::default().fg(color_axis)).bounds([0.0, disc_slice.len().saturating_sub(1) as f64]))
        .y_axis(Axis::default().bounds([0.5, 3.5]));

    f.render_widget(discovery_chart, area);
}

fn draw_vertical_block_stream(f: &mut Frame, app_state: &AppState, area: Rect) {
    if area.width < 2 { return; }
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    const UP_TRIANGLE: &str = "▲";
    const DOWN_TRIANGLE: &str = "▼";
    const SEPARATOR: &str = "·";

    let color_inflow = theme::BLUE;
    let color_outflow = theme::GREEN;
    let color_border = theme::SURFACE2;
    let color_empty = theme::SURFACE0;

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
        vec![Span::styled(title_str, Style::default().fg(theme::SUBTEXT0))]
    } else {
        let blue_ratio = total_in as f64 / total_ops as f64;
        let blue_chars = (blue_ratio * title_len as f64).round() as usize;
        let (blue_part, green_part) = title_str.split_at(blue_chars.min(title_len));
        let mut spans = Vec::new();
        if !blue_part.is_empty() { spans.push(Span::styled(blue_part, Style::default().fg(color_inflow))); }
        if !green_part.is_empty() { spans.push(Span::styled(green_part, Style::default().fg(color_outflow))); }
        spans
    };
    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color_border));

    let Some(torrent) = selected_torrent else {
        f.render_widget(block, area);
        return;
    };

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let history_len = inner_area.height as usize;
    let content_width = inner_area.width as usize;

    if history_len == 0 || content_width == 0 { return; }

    let in_history = &torrent.latest_state.blocks_in_history;
    let out_history = &torrent.latest_state.blocks_out_history;

    let in_slice = &in_history[in_history.len().saturating_sub(history_len)..];
    let out_slice = &out_history[out_history.len().saturating_sub(history_len)..];

    let slice_len = in_slice.len();
    let mut lines: Vec<Line> = Vec::with_capacity(history_len);
    let frame_seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;

    for i in 0..history_len {
        let mut spans = Vec::new();
        let dl_slice_index = slice_len.saturating_sub(1).saturating_sub(i);
        let raw_blocks_in = if i < slice_len { *in_slice.get(dl_slice_index).unwrap_or(&0) } else { 0 };
        let upload_padding = history_len.saturating_sub(slice_len);
        let ul_slice_index = i.saturating_sub(upload_padding);
        let raw_blocks_out = if i >= upload_padding { *out_slice.get(ul_slice_index).unwrap_or(&0) } else { 0 };

        let total_raw = raw_blocks_in + raw_blocks_out;
        let mut blocks_in: u64;
        let mut blocks_out: u64;

        if total_raw > content_width as u64 {
            blocks_in = (raw_blocks_in as f64 / total_raw as f64 * content_width as f64).round() as u64;
            blocks_out = (raw_blocks_out as f64 / total_raw as f64 * content_width as f64).round() as u64;
            if raw_blocks_in > 0 && blocks_in == 0 { blocks_in = 1; }
            if raw_blocks_out > 0 && blocks_out == 0 { blocks_out = 1; }
            
            let total_drawn = blocks_in + blocks_out;
             if total_drawn > content_width as u64 {
                let overfill = total_drawn - content_width as u64;
                if raw_blocks_in > raw_blocks_out { blocks_in = blocks_in.saturating_sub(overfill); } 
                else { blocks_out = blocks_out.saturating_sub(overfill); }
            } else if total_drawn < content_width as u64 {
                let remainder = (content_width as u64) - total_drawn;
                if raw_blocks_in > raw_blocks_out { blocks_in += remainder; } 
                else { blocks_out += remainder; }
            }
        } else {
            blocks_in = raw_blocks_in;
            blocks_out = raw_blocks_out;
        }

        let total_blocks = (blocks_in + blocks_out) as usize;
        if total_blocks == 0 {
            let padding = " ".repeat(content_width.saturating_sub(1) / 2);
            let trailing_padding = content_width.saturating_sub(1).saturating_sub(padding.len());
            spans.push(Span::raw(padding));
            spans.push(Span::styled(SEPARATOR, Style::default().fg(color_empty)));
            spans.push(Span::raw(" ".repeat(trailing_padding)));
        } else {
            let padding = (content_width.saturating_sub(total_blocks)) / 2;
            let trailing_padding = content_width.saturating_sub(total_blocks).saturating_sub(padding);
            
            let (larger_stream_count, smaller_stream_count, larger_symbol, smaller_symbol, larger_color, smaller_color, larger_seed_salt, smaller_seed_salt) = if blocks_in >= blocks_out {
                (blocks_in, blocks_out, DOWN_TRIANGLE, UP_TRIANGLE, color_inflow, color_outflow, dl_slice_index as u64, (ul_slice_index as u64) ^ 0xABCDEF)
            } else {
                (blocks_out, blocks_in, UP_TRIANGLE, DOWN_TRIANGLE, color_outflow, color_inflow, (ul_slice_index as u64) ^ 0xABCDEF, dl_slice_index as u64)
            };

            let mut order_rng = StdRng::seed_from_u64((dl_slice_index as u64) ^ (ul_slice_index as u64) ^ 0xDEADBEEF);
            let total_scaled_blocks_f64 = (larger_stream_count + smaller_stream_count) as f64;
            let ratio_smaller = smaller_stream_count as f64 / total_scaled_blocks_f64;
            let smaller_first: bool = order_rng.random_bool(1.0 - ratio_smaller);

            spans.push(Span::raw(" ".repeat(padding)));
            if smaller_first {
                render_sparkles(&mut spans, smaller_symbol, smaller_stream_count, smaller_color, frame_seed ^ smaller_seed_salt);
                render_sparkles(&mut spans, larger_symbol, larger_stream_count, larger_color, frame_seed ^ larger_seed_salt);
            } else {
                render_sparkles(&mut spans, larger_symbol, larger_stream_count, larger_color, frame_seed ^ larger_seed_salt);
                render_sparkles(&mut spans, smaller_symbol, smaller_stream_count, smaller_color, frame_seed ^ smaller_seed_salt);
            }
            spans.push(Span::raw(" ".repeat(trailing_padding)));
        }
        lines.push(Line::from(spans));
    }
    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner_area);
}

fn draw_torrent_sparklines(f: &mut Frame, app_state: &AppState, area: Rect) {
    let torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let Some(torrent) = torrent else {
        let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
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

        let dl_sparkline = Sparkline::default()
            .block(Block::default().title(Span::styled(format!("DL Activity (Peak: {})", format_speed(nice_max_speed)), Style::default().fg(theme::SUBTEXT0))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2)))
            .data(dl_slice).max(nice_max_speed).style(Style::default().fg(theme::BLUE));
        f.render_widget(dl_sparkline, area);
    } else if !has_dl_activity && has_ul_activity {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let ul_slice = &ul_history[ul_history.len().saturating_sub(width)..];
        let max_speed = ul_slice.iter().max().copied().unwrap_or(1);
        let nice_max_speed = calculate_nice_upper_bound(max_speed).max(1);
        let ul_sparkline = Sparkline::default()
            .block(Block::default().title(Span::styled(format!("UL Activity (Peak: {})", format_speed(nice_max_speed)), Style::default().fg(theme::SUBTEXT0))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2)))
            .data(ul_slice).max(nice_max_speed).style(Style::default().fg(theme::GREEN));
        f.render_widget(ul_sparkline, area);
    } else if !has_dl_activity && !has_ul_activity {
        let style = Style::default().fg(theme::MAUVE);
        let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
        let inner_area = block.inner(area);
        f.render_widget(block, area);

        let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Min(0)]).split(inner_area);
        let throbber_width = 23;
        let horizontal_chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(throbber_width), Constraint::Min(0)]).split(vertical_chunks[1]);
        let inner_chunks = Layout::horizontal([Constraint::Length(1), Constraint::Length(21), Constraint::Length(1)]).split(horizontal_chunks[1]);

        let throbber_left_area = inner_chunks[0];
        let label_area = inner_chunks[1];
        let throbber_right_area = inner_chunks[2];

        let label_text = Paragraph::new(" Searching for Peers ").style(style).alignment(Alignment::Center);
        let throbber_style = Style::default().fg(theme::LAVENDER).add_modifier(Modifier::BOLD);
        let throbber_widget = Throbber::default().style(throbber_style);

        f.render_widget(label_text, label_area);
        f.render_stateful_widget(throbber_widget.clone(), throbber_left_area, &mut app_state.throbber_holder.borrow_mut().torrent_sparkline);
        f.render_stateful_widget(throbber_widget, throbber_right_area, &mut app_state.throbber_holder.borrow_mut().torrent_sparkline);
    } else {
        let sparkline_chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
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

        let dl_sparkline = Sparkline::default()
            .block(Block::default().title(Span::styled(format!("DL (Peak: {})", format_speed(dl_nice_max)), Style::default().fg(theme::SUBTEXT0))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2)))
            .data(dl_slice).max(dl_nice_max).style(Style::default().fg(theme::BLUE));
        f.render_widget(dl_sparkline, dl_sparkline_chunk);

        let ul_sparkline = Sparkline::default()
            .block(Block::default().title(Span::styled(format!("UL (Peak: {})", format_speed(ul_nice_max)), Style::default().fg(theme::SUBTEXT0))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2)))
            .data(ul_slice).max(ul_nice_max).style(Style::default().fg(theme::GREEN));
        f.render_widget(ul_sparkline, ul_sparkline_chunk);
    }
}

fn draw_delete_confirm_dialog(f: &mut Frame, app_state: &AppState) {
    if let AppMode::DeleteConfirm { info_hash, with_files } = &app_state.mode {
        if let Some(torrent_to_delete) = app_state.torrents.get(info_hash) {
            let area = centered_rect(50, 25, f.area());
            f.render_widget(Clear, area);

            let torrent_name = &torrent_to_delete.latest_state.torrent_name;
            let download_path_str = torrent_to_delete.latest_state.download_path.to_string_lossy();

            let mut text = vec![
                Line::from(Span::styled("Confirm Deletion", Style::default().fg(theme::RED))),
                Line::from(""),
                Line::from(torrent_name.as_str()),
                Line::from(Span::styled(download_path_str.to_string(), Style::default().fg(theme::SUBTEXT1))),
                Line::from(""),
            ];

            if *with_files {
                text.push(Line::from("Are you sure you want to remove this torrent?"));
                text.push(Line::from(""));
                text.push(Line::from(Span::styled("This will also permanently delete associated files.", Style::default().fg(theme::YELLOW).bold().underlined())));
            } else {
                text.push(Line::from("Are you sure you want to remove this torrent?"));
                text.push(Line::from(""));
                text.push(Line::from(vec![
                    Span::raw("The downloaded files will "),
                    Span::styled("NOT", Style::default().fg(theme::YELLOW).bold().underlined()),
                    Span::raw(" be deleted."),
                ]));
                text.push(Line::from(""));
                text.push(Line::from(vec![
                    Span::styled("Press ", Style::default().fg(theme::SUBTEXT1)),
                    Span::styled("[D]", Style::default().fg(theme::YELLOW).bold()),
                    Span::styled(" instead to remove the torrent and delete associated files.", Style::default().fg(theme::SUBTEXT1)),
                ]));
            }

            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled("[Enter]", Style::default().fg(theme::GREEN)), Span::raw(" Confirm  "),
                Span::styled("[Esc]", Style::default().fg(theme::RED)), Span::raw(" Cancel"),
            ]));

            let block = Block::default().title("Confirmation").borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
            let paragraph = Paragraph::new(text).block(block).style(Style::default().fg(theme::TEXT));
            f.render_widget(paragraph, area);
        }
    }
}

fn draw_status_error_popup(f: &mut Frame, error_text: &str) {
    let popup_width_percent: u16 = 50;
    let popup_height: u16 = 8;
    let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(popup_height), Constraint::Min(0)]).split(f.area());
    let area = Layout::horizontal([
        Constraint::Percentage((100 - popup_width_percent) / 2),
        Constraint::Percentage(popup_width_percent),
        Constraint::Percentage((100 - popup_width_percent) / 2),
    ]).split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let text = vec![
        Line::from(Span::styled("Error", Style::default().fg(theme::RED).bold())),
        Line::from(""),
        Line::from(Span::styled(error_text, Style::default().fg(theme::YELLOW))),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("[Press Esc to dismiss]", Style::default().fg(theme::SUBTEXT1))),
    ];
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::RED));
    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Center).wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn draw_shutdown_screen(f: &mut Frame, app_state: &AppState) {
    const POPUP_WIDTH: u16 = 40;
    const POPUP_HEIGHT: u16 = 3;
    let area = f.area();
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);
    let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(height), Constraint::Min(0)]).split(area);
    let area = Layout::horizontal([Constraint::Min(0), Constraint::Length(width), Constraint::Min(0)]).split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let container_block = Block::default().title(Span::styled(" Exiting ", Style::default().fg(theme::PEACH))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = container_block.inner(area);
    f.render_widget(container_block, area);

    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1)]).split(inner_area);
    let progress_label = format!("{:.0}%", (app_state.shutdown_progress * 100.0).min(100.0));
    let progress_bar = Gauge::default().ratio(app_state.shutdown_progress).label(progress_label).gauge_style(Style::default().fg(theme::MAUVE).bg(theme::SURFACE0));
    f.render_widget(progress_bar, chunks[0]);
}

fn draw_power_saving_screen(f: &mut Frame, app_state: &AppState, settings: &Settings) {
    const TRANQUIL_MESSAGES: &[&str] = &[
        "Quietly seeding...", "Awaiting peers...", "Sharing data...", "Connecting to the swarm...",
        "Sharing pieces...", "The network is vast...", "Listening for connections...", "Seeding the cloud...",
        "Uptime is a gift...", "Data flows...", "Maintaining the ratio...", "A torrent of tranquility...",
        "A piece at a time...", "The swarm is peaceful...", "Be the torrent...", "Nurturing the swarm...",
        "Awaiting the handshake...", "Distributing packets...", "The ratio is balanced...", "Each piece finds its home...",
        "Announcing to the tracker...", "The bitfield is complete...",
    ];

    let dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
    let ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);
    let dl_limit = settings.global_download_limit_bps;
    let ul_limit = settings.global_upload_limit_bps;

    let area = centered_rect(40, 60, f.area());
    f.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE1));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(8), Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
    let content_area = vertical_chunks[1];
    let footer_area = vertical_chunks[3];

    let mut dl_spans = vec![Span::styled("DL: ", Style::default().fg(theme::SKY)), Span::styled(format_speed(dl_speed), Style::default().fg(theme::SKY)), Span::raw(" / ")];
    if dl_limit > 0 && dl_speed >= dl_limit { dl_spans.push(Span::styled(format_limit_bps(dl_limit), Style::default().fg(theme::RED))); } else { dl_spans.push(Span::styled(format_limit_bps(dl_limit), Style::default().fg(theme::SUBTEXT0))); }

    let mut ul_spans = vec![Span::styled("UL: ", Style::default().fg(theme::TEAL)), Span::styled(format_speed(ul_speed), Style::default().fg(theme::TEAL)), Span::raw(" / ")];
    if ul_limit > 0 && ul_speed >= ul_limit { ul_spans.push(Span::styled(format_limit_bps(ul_limit), Style::default().fg(theme::RED))); } else { ul_spans.push(Span::styled(format_limit_bps(ul_limit), Style::default().fg(theme::SUBTEXT0))); }

    const MESSAGE_INTERVAL_SECONDS: u64 = 500;
    let seconds_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let seed = seconds_since_epoch / MESSAGE_INTERVAL_SECONDS;
    let mut rng = StdRng::seed_from_u64(seed);
    let message_index = rng.random_range(0..TRANQUIL_MESSAGES.len());
    let current_message = TRANQUIL_MESSAGES[message_index];

    let main_content_lines = vec![
        Line::from(vec![Span::styled("super", Style::default().fg(theme::SKY)), Span::styled("seedr", Style::default().fg(theme::TEAL))]),
        Line::from(""),
        Line::from(Span::styled(current_message, Style::default().fg(theme::SUBTEXT1))),
        Line::from(""),
        Line::from(dl_spans),
        Line::from(ul_spans),
    ];
    let main_paragraph = Paragraph::new(main_content_lines).alignment(Alignment::Center);
    let footer_line = Line::from(Span::styled("Press [z] to resume", Style::default().fg(theme::SUBTEXT0)));
    let footer_paragraph = Paragraph::new(footer_line).alignment(Alignment::Center);

    f.render_widget(main_paragraph, content_area);
    f.render_widget(footer_paragraph, footer_area);
}

fn draw_welcome_screen(f: &mut Frame) {
    let text = vec![
        Line::from(Span::styled("A BitTorrent Client in your Terminal", Style::default())),
        Line::from(""),
        Line::from(Span::styled("How to Get Started:", Style::default().fg(theme::YELLOW).bold())),
        Line::from(""),
        Line::from(vec![Span::styled(" 1. ", Style::default().fg(theme::GREEN)), Span::raw("Paste (Ctrl+V) a "), Span::styled("magnet link", Style::default().fg(theme::PEACH)), Span::raw(" or "), Span::styled("`.torrent` file path", Style::default().fg(theme::PEACH)), Span::raw(".") ]),
        Line::from("    A file picker will appear to choose a download location for magnet links."),
        Line::from(""),
        Line::from(vec![Span::styled(" 2. ", Style::default().fg(theme::GREEN)), Span::raw("Use the CLI in another terminal while this TUI is running:")]),
        Line::from(Span::styled("   $ superseedr \"magnet:?xt=urn:btih:...\"", Style::default().fg(theme::SURFACE2))),
        Line::from(Span::styled("   $ superseedr \"/path/to/my.torrent\"", Style::default().fg(theme::SURFACE2))),
        Line::from(vec![Span::raw("    Note: CLI requires a default download path. Press "), Span::styled("[c]", Style::default().fg(theme::MAUVE)), Span::raw(" to configure.")]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![Span::styled(" [m] ", Style::default().fg(theme::TEAL)), Span::styled("for manual/help", Style::default().fg(theme::SUBTEXT1)), Span::styled(" | ", Style::default().fg(theme::SURFACE2)), Span::styled("[Esc] ", Style::default().fg(theme::RED)), Span::styled("to dismiss", Style::default().fg(theme::SUBTEXT1))]),
    ];

    let text_height = text.len() as u16;
    let text_width = text.iter().map(|line| line.width()).max().unwrap_or(0) as u16;
    let horizontal_padding: u16 = 4;
    let vertical_padding: u16 = 2;
    let box_width = (text_width + horizontal_padding + 2).min(f.area().width);
    let box_height = (text_height + vertical_padding + 2).min(f.area().height);

    let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(box_height), Constraint::Min(0)]).split(f.area());
    let area = Layout::horizontal([Constraint::Min(0), Constraint::Length(box_width), Constraint::Min(0)]).split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let block = Block::default().title(Span::styled(" Welcome to superseedr! ", Style::default().fg(theme::MAUVE))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let vertical_chunks_inner = Layout::vertical([Constraint::Min(0), Constraint::Length(text_height), Constraint::Min(0)]).split(inner_area);
    let horizontal_chunks_inner = Layout::horizontal([Constraint::Min(0), Constraint::Length(text_width), Constraint::Min(0)]).split(vertical_chunks_inner[1]);
    let paragraph = Paragraph::new(text).style(Style::default().fg(theme::TEXT)).alignment(Alignment::Left);
    f.render_widget(paragraph, horizontal_chunks_inner[1]);
}

fn draw_help_popup(f: &mut Frame, app_state: &AppState, mode: &AppMode) {
    let (settings_path_str, log_path_str) = if let Some((config_dir, data_dir)) = get_app_paths() {
        (config_dir.join("settings.toml").to_string_lossy().to_string(), data_dir.join("logs").join("app.log").to_string_lossy().to_string())
    } else { ("Unknown location".to_string(), "Unknown location".to_string()) };

    let area = centered_rect(60, 100, f.area());
    f.render_widget(Clear, area);

    if let Some(warning_text) = &app_state.system_warning {
         let warning_width = area.width.saturating_sub(2).max(1) as usize;
         let warning_lines = (warning_text.len() as f64 / warning_width as f64).ceil() as u16;
         let warning_block_height = warning_lines.saturating_add(2).max(3);
         let max_warning_height = (area.height as f64 * 0.25).round() as u16;
         let final_warning_height = warning_block_height.min(max_warning_height);
         let chunks = Layout::vertical([Constraint::Length(final_warning_height), Constraint::Min(0), Constraint::Length(3)]).split(area);

         let warning_paragraph = Paragraph::new(warning_text.as_str()).wrap(Wrap { trim: true }).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::RED))).style(Style::default().fg(theme::YELLOW));
         f.render_widget(warning_paragraph, chunks[0]);
         draw_help_table(f, mode, chunks[1]);

         let footer_block = Block::default().border_style(Style::default().fg(theme::SURFACE2));
         let footer_inner_area = footer_block.inner(chunks[2]);
         f.render_widget(footer_block, chunks[2]);
         let footer_lines = vec![
             Line::from(vec![Span::styled("Settings: ", Style::default().fg(theme::TEXT)), Span::styled(truncate_with_ellipsis(&settings_path_str, footer_inner_area.width as usize - 10), Style::default().fg(theme::SUBTEXT0))]),
             Line::from(vec![Span::styled("Log File: ", Style::default().fg(theme::TEXT)), Span::styled(truncate_with_ellipsis(&log_path_str, footer_inner_area.width as usize - 10), Style::default().fg(theme::SUBTEXT0))]),
         ];
         let footer_paragraph = Paragraph::new(footer_lines).style(Style::default().fg(theme::TEXT));
         f.render_widget(footer_paragraph, footer_inner_area);
    } else {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
        draw_help_table(f, mode, chunks[0]);
        let footer_block = Block::default().border_style(Style::default().fg(theme::SURFACE2));
        let footer_inner_area = footer_block.inner(chunks[1]);
        f.render_widget(footer_block, chunks[1]);
        let footer_lines = vec![
             Line::from(vec![Span::styled("Settings: ", Style::default().fg(theme::TEXT)), Span::styled(truncate_with_ellipsis(&settings_path_str, footer_inner_area.width as usize - 10), Style::default().fg(theme::SUBTEXT0))]),
             Line::from(vec![Span::styled("Log File: ", Style::default().fg(theme::TEXT)), Span::styled(truncate_with_ellipsis(&log_path_str, footer_inner_area.width as usize - 10), Style::default().fg(theme::SUBTEXT0))]),
        ];
        let footer_paragraph = Paragraph::new(footer_lines).style(Style::default().fg(theme::TEXT));
        f.render_widget(footer_paragraph, footer_inner_area);
    }
}

fn draw_help_table(f: &mut Frame, mode: &AppMode, area: Rect) {
    let (title, rows) = match mode {
        AppMode::Normal | AppMode::Welcome => (
            " Manual / Help ",
            vec![
                Row::new(vec![Cell::from(Span::styled("Ctrl +", Style::default().fg(theme::TEAL))), Cell::from("Zoom in (increase font size)")]),
                Row::new(vec![Cell::from(Span::styled("Ctrl -", Style::default().fg(theme::TEAL))), Cell::from("Zoom out (decrease font size)")]),
                Row::new(vec![Cell::from(Span::styled("q", Style::default().fg(theme::RED))), Cell::from("Quit the application")]),
                Row::new(vec![Cell::from(Span::styled("m", Style::default().fg(theme::MAUVE))), Cell::from("Toggle this help screen")]),
                Row::new(vec![Cell::from(Span::styled("c", Style::default().fg(theme::PEACH))), Cell::from("Open Config screen")]),
                Row::new(vec![Cell::from(Span::styled("z", Style::default().fg(theme::SUBTEXT0))), Cell::from("Toggle Zen/Power Saving mode")]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled("List Navigation", Style::default().fg(theme::YELLOW)))]),
                Row::new(vec![Cell::from(Span::styled("↑ / ↓ / k / j", Style::default().fg(theme::BLUE))), Cell::from("Navigate torrents list")]),
                Row::new(vec![Cell::from(Span::styled("← / → / h / l", Style::default().fg(theme::BLUE))), Cell::from("Navigate between header columns")]),
                Row::new(vec![Cell::from(Span::styled("s", Style::default().fg(theme::GREEN))), Cell::from("Change sort order for the selected column")]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled("Torrent Actions", Style::default().fg(theme::YELLOW)))]),
                Row::new(vec![Cell::from(Span::styled("p", Style::default().fg(theme::GREEN))), Cell::from("Pause / Resume selected torrent")]),
                Row::new(vec![Cell::from(Span::styled("d / D", Style::default().fg(theme::RED))), Cell::from("Delete torrent (D includes downloaded files)")]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled("Adding Torrents", Style::default().fg(theme::YELLOW)))]),
                Row::new(vec![Cell::from(Span::styled("Paste | v", Style::default().fg(theme::SAPPHIRE))), Cell::from("Paste a magnet link or local file path to add")]),
                Row::new(vec![Cell::from(Span::styled("CLI", Style::default().fg(theme::SAPPHIRE))), Cell::from("Use `superseedr add ...` from another terminal")]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled("Graph & Panes", Style::default().fg(theme::YELLOW)))]),
                Row::new(vec![Cell::from(Span::styled("t / T", Style::default().fg(theme::TEAL))), Cell::from("Switch network graph time scale forward/backward")]),
                Row::new(vec![Cell::from(Span::styled("[ / ]", Style::default().fg(theme::TEAL))), Cell::from("Change UI refresh rate (FPS)")]),
                Row::new(vec![Cell::from(Span::styled("x", Style::default().fg(theme::TEAL))), Cell::from("Anonymize torrent names")]),
            ]
        ),
        AppMode::Config { .. } => (
            " Help / Config ",
            vec![
                Row::new(vec![Cell::from(Span::styled("Esc / q", Style::default().fg(theme::GREEN))), Cell::from("Save and exit config")]),
                Row::new(vec![Cell::from(Span::styled("↑ / ↓ / k / j", Style::default().fg(theme::BLUE))), Cell::from("Navigate items")]),
                Row::new(vec![Cell::from(Span::styled("← / → / h / l", Style::default().fg(theme::BLUE))), Cell::from("Decrease / Increase value")]),
                Row::new(vec![Cell::from(Span::styled("Enter", Style::default().fg(theme::YELLOW))), Cell::from("Start or confirm editing")]),
            ],
        ),
        AppMode::ConfigPathPicker { .. } | AppMode::DownloadPathPicker { .. } => (
            " Help / File Browser ",
            vec![
                Row::new(vec![Cell::from(Span::styled("Esc", Style::default().fg(theme::RED))), Cell::from("Cancel selection")]),
                Row::new(vec![Cell::from(Span::styled("Tab", Style::default().fg(theme::GREEN))), Cell::from("Confirm selection")]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled("↑ / ↓", Style::default().fg(theme::BLUE))), Cell::from("Navigate files")]),
                Row::new(vec![Cell::from(Span::styled("←", Style::default().fg(theme::BLUE))), Cell::from("Go to parent directory")]),
                Row::new(vec![Cell::from(Span::styled("→ / Enter", Style::default().fg(theme::BLUE))), Cell::from("Enter directory")]),
            ],
        ),
        _ => (" Help ", vec![Row::new(vec![Cell::from("No help available for this view.")])]),
    };

    let help_table = Table::new(rows, [Constraint::Length(20), Constraint::Min(30)]).block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2)));
    f.render_widget(Clear, area);
    f.render_widget(help_table, area);
}

fn draw_config_screen(f: &mut Frame, settings: &Settings, selected_index: usize, items: &[ConfigItem], editing: &Option<(ConfigItem, String)>) {
    let area = centered_rect(80, 60, f.area());
    f.render_widget(Clear, f.area());
    let block = Block::default().title(Span::styled("Config", Style::default().fg(theme::MAUVE))).borders(Borders::ALL).border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(2)]).split(inner_area);
    let settings_area = chunks[0];
    let footer_area = chunks[1];
    let rows_layout = Layout::default().direction(Direction::Vertical).constraints(items.iter().map(|_| Constraint::Length(1)).collect::<Vec<_>>()).split(settings_area);

    for (i, item) in items.iter().enumerate() {
        let (name_str, value_str) = match item {
            ConfigItem::ClientPort => ("Listen Port", settings.client_port.to_string()),
            ConfigItem::DefaultDownloadFolder => ("Default Download Folder", path_to_string(settings.default_download_folder.as_deref())),
            ConfigItem::WatchFolder => ("Torrent Watch Folder", path_to_string(settings.watch_folder.as_deref())),
            ConfigItem::GlobalDownloadLimit => ("Global DL Limit", format_limit_bps(settings.global_download_limit_bps)),
            ConfigItem::GlobalUploadLimit => ("Global UL Limit", format_limit_bps(settings.global_upload_limit_bps)),
        };

        let columns = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(60), Constraint::Percentage(40)]).split(rows_layout[i]);
        let is_highlighted = if let Some((edited_item, _)) = editing { *edited_item == *item } else { i == selected_index };
        let row_style = if is_highlighted { Style::default().fg(theme::YELLOW) } else { Style::default().fg(theme::TEXT) };
        let name_with_selector = if is_highlighted { format!("▶ {}", name_str) } else { format!("  {}", name_str) };

        let name_p = Paragraph::new(name_with_selector).style(row_style);
        f.render_widget(name_p, columns[0]);

        if let Some((_edited_item, buffer)) = editing {
            if is_highlighted {
                let edit_p = Paragraph::new(buffer.as_str()).style(row_style.fg(theme::YELLOW));
                f.set_cursor_position((columns[1].x + buffer.len() as u16, columns[1].y));
                f.render_widget(edit_p, columns[1]);
            } else {
                let value_p = Paragraph::new(value_str).style(row_style);
                f.render_widget(value_p, columns[1]);
            }
        } else {
            let value_p = Paragraph::new(value_str).style(row_style);
            f.render_widget(value_p, columns[1]);
        }
    }

    let help_text = if editing.is_some() {
        Line::from(vec![Span::styled("[Enter]", Style::default().fg(theme::GREEN)), Span::raw(" to confirm, "), Span::styled("[Esc]", Style::default().fg(theme::RED)), Span::raw(" to cancel.")])
    } else {
        Line::from(vec![Span::raw("Use "), Span::styled("↑/↓/k/j", Style::default().fg(theme::YELLOW)), Span::raw(" to navigate. "), Span::styled("[Enter]", Style::default().fg(theme::YELLOW)), Span::raw(" to edit. "), Span::styled("[r]", Style::default().fg(theme::YELLOW)), Span::raw("eset to default. "), Span::styled("[Esc]|[q]", Style::default().fg(theme::GREEN)), Span::raw(" to Save & Exit, ")])
    };

    let footer_paragraph = Paragraph::new(help_text).alignment(Alignment::Center).style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(footer_paragraph, footer_area);
}

fn draw_swarm_heatmap(f: &mut Frame, peers: &[PeerInfo], total_pieces: u32, area: Rect) {
    let color_status_low = Style::default().fg(theme::RED).add_modifier(Modifier::DIM);
    let color_status_medium = Style::default().fg(theme::YELLOW).add_modifier(Modifier::DIM);
    let color_status_high = Style::default().fg(theme::BLUE).add_modifier(Modifier::DIM);
    let color_status_complete = Style::default().fg(theme::LAVENDER).add_modifier(Modifier::BOLD);
    let color_status_empty = Style::default().fg(theme::SUBTEXT1);
    let color_status_waiting = Style::default().fg(theme::SUBTEXT1);

    let color_heatmap_low = theme::MAUVE;
    let color_heatmap_medium = theme::MAUVE;
    let color_heatmap_high = theme::MAUVE;
    let color_heatmap_empty = theme::SURFACE1;

    let shade_light = symbols::shade::LIGHT;
    let shade_medium = symbols::shade::MEDIUM;
    let shade_dark = symbols::shade::DARK;

    let total_pieces_usize = total_pieces as usize;
    let mut availability: Vec<u32> = vec![0; total_pieces_usize];
    if total_pieces_usize > 0 {
        for peer in peers {
            for (i, has_piece) in peer.bitfield.iter().enumerate().take(total_pieces_usize) {
                if *has_piece { availability[i] += 1; }
            }
        }
    }

    let max_avail = availability.iter().max().copied().unwrap_or(0);
    let pieces_available_in_swarm = availability.iter().filter(|&&count| count > 0).count();
    let is_swarm_complete = total_pieces_usize > 0 && pieces_available_in_swarm == total_pieces_usize;
    let total_peers = peers.len();

    let (status_text, status_style) = if total_pieces_usize == 0 { ("Waiting...".to_string(), color_status_waiting) }
    else if is_swarm_complete { ("Complete".to_string(), color_status_complete) }
    else if max_avail == 0 { ("Empty".to_string(), color_status_empty) }
    else if total_peers == 0 { ("Low (0%)".to_string(), color_status_low) }
    else {
        let availability_percentage = (pieces_available_in_swarm as f64 / total_pieces_usize as f64) * 100.0;
        if availability_percentage < 33.3 { (format!("Low ({:.0}%)", availability_percentage), color_status_low) }
        else if availability_percentage < 66.6 { (format!("Medium ({:.0}%)", availability_percentage), color_status_medium) }
        else { (format!("High ({:.0}%)", availability_percentage), color_status_high) }
    };

    let title = Line::from(vec![Span::styled(" Swarm Availability: ", Style::default().fg(theme::LAVENDER)), Span::styled(status_text, status_style)]);
    let block = Block::default().title(title).borders(Borders::NONE).padding(Padding::new(1, 1, 0, 1)).border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if total_pieces_usize == 0 {
        let center_text = Paragraph::new("Waiting for metadata...").style(Style::default().fg(theme::SUBTEXT1)).alignment(Alignment::Center);
        let vertical_chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Min(0)]).split(inner_area);
        f.render_widget(center_text, vertical_chunks[1]);
        return;
    }

    let max_avail_f64 = max_avail.max(1) as f64;
    let available_width = inner_area.width as usize;
    let available_height = inner_area.height as usize;
    let total_cells = (available_width * available_height) as u64;

    if total_cells == 0 { return; }

    let mut lines = Vec::with_capacity(available_height);
    let total_pieces_u64 = total_pieces_usize as u64;

    for y in 0..available_height {
        let mut spans = Vec::with_capacity(available_width);
        for x in 0..available_width {
            let cell_index = (y * available_width + x) as u64;
            let piece_index = ((cell_index * total_pieces_u64) / total_cells) as usize;
            if piece_index >= total_pieces_usize { spans.push(Span::raw(" ")); continue; }
            let count = availability[piece_index];
            let (piece_char, color) = if count == 0 { (shade_light, color_heatmap_empty) }
            else {
                let norm_val = count as f64 / max_avail_f64;
                if norm_val < 0.20 { (shade_light, color_heatmap_low) }
                else if norm_val < 0.80 { (shade_medium, color_heatmap_medium) }
                else { (shade_dark, color_heatmap_high) }
            };
            spans.push(Span::styled(piece_char.to_string(), Style::default().fg(color)));
        }
        lines.push(Line::from(spans));
    }
    let heatmap = Paragraph::new(lines);
    f.render_widget(heatmap, inner_area);
}

fn render_sparkles<'a>(spans: &mut Vec<Span<'a>>, symbol: &'a str, count: u64, color: Color, seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    for _ in 0..count {
        let is_bold: bool = rng.random();
        let mut style = Style::default().fg(color);
        style = if is_bold { style.add_modifier(Modifier::BOLD) } else { style.add_modifier(Modifier::DIM) };
        spans.push(Span::styled(symbol, style));
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
