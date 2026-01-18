// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::symbols::Marker;
use ratatui::{prelude::*, symbols, widgets::*};

use crate::tui::formatters::*;
use crate::tui::layout::calculate_file_browser_layout; // Import the new function
use crate::tui::layout::{get_torrent_columns, ColumnId};
use crate::tui::tree;
use crate::tui::tree::{TreeFilter, TreeMathHelper};

use crate::app::FileBrowserMode;
use crate::app::FileMetadata;
use crate::app::GraphDisplayMode;
use crate::app::PeerInfo;
use crate::app::{AppMode, AppState, ConfigItem, SelectedHeader, TorrentControlState};
use crate::app::{BrowserPane, FilePriority};

use crate::tui::layout::get_peer_columns;
use crate::tui::layout::PeerColumnId;
use crate::tui::layout::{calculate_layout, compute_smart_table_layout, LayoutContext, SmartCol};

use crate::config::get_app_paths;
use crate::config::{Settings, SortDirection};
use crate::theme;
use throbber_widgets_tui::Throbber;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::{SystemTime, UNIX_EPOCH};

static APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const SECONDS_HISTORY_MAX: usize = 3600;
pub const MINUTES_HISTORY_MAX: usize = 48 * 60;

pub fn draw(f: &mut Frame, app_state: &AppState, settings: &Settings) {
    let area = f.area();

    if app_state.show_help {
        draw_help_popup(f, app_state);
        return;
    }

    match &app_state.mode {
        AppMode::Welcome => {
            draw_welcome_screen(f);
            return;
        }
        AppMode::PowerSaving => {
            draw_power_saving_screen(f, app_state, settings);
            return;
        }
        AppMode::Config {
            settings_edit,
            selected_index,
            items,
            editing,
        } => {
            draw_config_screen(f, settings_edit, *selected_index, items, editing);
            return;
        }
        AppMode::DeleteConfirm { .. } => {
            draw_delete_confirm_dialog(f, app_state);
            return;
        }
        AppMode::FileBrowser {
            state,
            data,
            browser_mode,
        } => {
            let _is_torrent_mode = app_state.pending_torrent_path.is_some()
                || !app_state.pending_torrent_link.is_empty();
            draw_file_browser(f, app_state, state, data, browser_mode);
            return;
        }
        _ => {}
    }

    let ctx = LayoutContext::new(area, app_state, 35);
    let plan = calculate_layout(area, &ctx);

    draw_torrent_list(f, app_state, plan.list);
    draw_footer(f, app_state, settings, plan.footer);
    draw_details_panel(f, app_state, plan.details);
    draw_peers_table(f, app_state, plan.peers);

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

    if let Some(msg) = plan.warning_message {
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(theme::RED).bg(theme::SURFACE0)),
            plan.list,
        );
    }
    if let Some(error_text) = &app_state.system_error {
        draw_status_error_popup(f, error_text);
    }
    if app_state.should_quit {
        draw_shutdown_screen(f, app_state);
    }
}

fn draw_torrent_list(f: &mut Frame, app_state: &AppState, area: Rect) {
    let mut table_state = TableState::default();
    if matches!(app_state.selected_header, SelectedHeader::Torrent(_)) {
        table_state.select(Some(app_state.selected_torrent_index));
    }

    let all_cols = get_torrent_columns();
    let smart_cols: Vec<SmartCol> = all_cols
        .iter()
        .map(|c| SmartCol {
            min_width: c.min_width,
            priority: c.priority,
            constraint: c.default_constraint,
        })
        .collect();

    let (constraints, visible_indices) = compute_smart_table_layout(&smart_cols, area.width, 1);

    let (sort_col, sort_dir) = app_state.torrent_sort;
    let header_cells: Vec<Cell> = visible_indices
        .iter()
        .enumerate()
        .map(|(visual_idx, &real_idx)| {
            let def = &all_cols[real_idx];
            let is_selected = app_state.selected_header == SelectedHeader::Torrent(visual_idx);
            let is_sorting = def.sort_enum == Some(sort_col);

            let mut style = Style::default().fg(theme::YELLOW);
            if is_sorting {
                style = style.fg(theme::MAUVE);
            }

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
                    let is_selected = i == app_state.selected_torrent_index;

                    let mut row_style = match state.torrent_control_state {
                        TorrentControlState::Running => Style::default().fg(theme::TEXT),
                        TorrentControlState::Paused => Style::default().fg(theme::SURFACE1),
                        TorrentControlState::Deleting => Style::default().fg(theme::RED),
                    };

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
                                    let total = state.number_of_pieces_total;
                                    let skipped_count = state
                                        .file_priorities
                                        .values()
                                        .filter(|&&p| p == FilePriority::Skip)
                                        .count()
                                        as u32;
                                    let effective_total = total.saturating_sub(skipped_count);

                                    let display_pct = if state.number_of_pieces_total > 0
                                        && effective_total > 0
                                    {
                                        let completed = state.number_of_pieces_completed;
                                        if torrent.latest_state.activity_message.contains("Seeding")
                                            || torrent
                                                .latest_state
                                                .activity_message
                                                .contains("Finished")
                                        {
                                            100.0
                                        } else {
                                            // Use effective_total to show progress relative to what is wanted
                                            ((completed as f64 / effective_total as f64) * 100.0)
                                                .min(100.0)
                                        }
                                    } else {
                                        0.0
                                    };
                                    Cell::from(format!("{:.1}%", display_pct))
                                }
                                ColumnId::Name => {
                                    let name = if app_state.anonymize_torrent_names {
                                        format!("Torrent {}", i + 1)
                                    } else {
                                        state.torrent_name.clone()
                                    };
                                    let mut c = Cell::from(name);
                                    if is_selected {
                                        c = c.style(Style::default().fg(theme::YELLOW));
                                    }
                                    c
                                }
                                ColumnId::DownSpeed => {
                                    Cell::from(format_speed(torrent.smoothed_download_speed_bps))
                                        .style(speed_to_style(torrent.smoothed_download_speed_bps))
                                }
                                ColumnId::UpSpeed => {
                                    Cell::from(format_speed(torrent.smoothed_upload_speed_bps))
                                        .style(speed_to_style(torrent.smoothed_upload_speed_bps))
                                }
                            }
                        })
                        .collect();

                    Row::new(cells).style(row_style)
                }
                None => Row::new(vec![Cell::from("Error retrieving data")]),
            });

    let border_style = if matches!(app_state.selected_header, SelectedHeader::Torrent(_)) {
        Style::default().fg(theme::MAUVE)
    } else {
        Style::default().fg(theme::SURFACE2)
    };

    let mut title_spans = Vec::new();
    if app_state.is_searching {
        title_spans.push(Span::raw("Search: /"));
        title_spans.push(Span::styled(
            &app_state.search_query,
            Style::default().fg(theme::YELLOW),
        ));
    } else if !app_state.search_query.is_empty() {
        title_spans.push(Span::styled(
            format!("[{}] ", app_state.search_query),
            Style::default()
                .fg(theme::SUBTEXT1)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    if !app_state.is_searching {
        if let Some(info_hash) = app_state
            .torrent_list_order
            .get(app_state.selected_torrent_index)
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
                    &path_cow
                };

                let avail_width = area.width.saturating_sub(10) as usize;
                let display_name = truncate_with_ellipsis(text_to_show, avail_width);

                title_spans.push(Span::styled(
                    display_name,
                    Style::default().fg(theme::YELLOW),
                ));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Line::from(title_spans));
    let table = Table::new(rows, constraints).header(header).block(block);
    f.render_stateful_widget(table, area, &mut table_state);
}

fn draw_network_chart(f: &mut Frame, app_state: &AppState, chart_chunk: Rect) {
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

    let total_library_size: u64 = app_state
        .torrents
        .values()
        .map(|t| t.latest_state.total_size)
        .sum();

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
        // --- UPDATED: Torrents Line with Size ---
        Line::from(vec![
            Span::styled("Torrents: ", Style::default().fg(theme::PEACH)),
            Span::raw(format!(
                "{} ({})",
                app_state.torrents.len(),
                format_bytes(total_library_size)
            )),
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

    let (lvl, progress) = calculate_player_stats(app_state);
    // "Stats | Lvl 999 " is roughly 16 chars. Borders are 2. Total static overhead ~18.
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
        Span::styled("Stats", Style::default().fg(Color::White)),
        Span::raw(" | "),
        Span::styled(
            format!("Lvl {}", lvl),
            Style::default().fg(theme::YELLOW).bold(),
        ),
        Span::raw(" "),
        Span::styled(gauge_str, Style::default().fg(theme::GREEN)),
    ];

    if show_pct {
        title_spans.push(Span::styled(
            format!(" {:.0}%", progress * 100.0),
            Style::default().fg(theme::SUBTEXT1),
        ));
    }

    let stats_paragraph = Paragraph::new(stats_text)
        .block(
            Block::default()
                .title(Line::from(title_spans))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE2)),
        )
        .style(Style::default().fg(theme::TEXT));

    f.render_widget(stats_paragraph, stats_chunk);
}

fn draw_details_panel(f: &mut Frame, app_state: &AppState, details_text_chunk: Rect) {
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
                if state.torrent_control_state != TorrentControlState::Running
                    || state.activity_message.contains("Seeding")
                    || state.activity_message.contains("Finished")
                {
                    (1.0, "100.0%".to_string())
                } else {
                    let ratio = state.number_of_pieces_completed as f64
                        / state.number_of_pieces_total as f64;
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

fn draw_peers_table(f: &mut Frame, app_state: &AppState, peers_chunk: Rect) {
    if peers_chunk.height < 2 || peers_chunk.width < 2 {
        return;
    }

    if let Some(info_hash) = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
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
                    if matches!(app_state.selected_header, SelectedHeader::Peer(_)) {
                        Style::default().fg(theme::MAUVE)
                    } else {
                        Style::default().fg(theme::SURFACE2)
                    };

                if peers_to_display.is_empty() {
                    draw_swarm_heatmap(f, &state.peers, state.number_of_pieces_total, peers_chunk);
                } else {
                    let header_cells: Vec<Cell> = visible_indices
                        .iter()
                        .enumerate()
                        .map(|(visual_idx, &real_idx)| {
                            let def = &all_peer_cols[real_idx];

                            let is_selected =
                                app_state.selected_header == SelectedHeader::Peer(visual_idx);
                            let is_sorting = def.sort_enum == Some(sort_by);

                            let mut style = Style::default().fg(theme::YELLOW);
                            if is_sorting {
                                style = style.fg(theme::MAUVE);
                            }

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
                                theme::SURFACE1
                            } else {
                                ip_to_color(&peer.address)
                            };

                        let cells: Vec<Cell> = visible_indices
                            .iter()
                            .map(|&real_idx| {
                                let def = &all_peer_cols[real_idx];
                                match def.id {
                                    PeerColumnId::Flags => Line::from(vec![
                                        Span::styled(
                                            "■",
                                            Style::default().fg(if peer.am_interested {
                                                theme::SAPPHIRE
                                            } else {
                                                theme::SURFACE1
                                            }),
                                        ),
                                        Span::styled(
                                            "■",
                                            Style::default().fg(if peer.peer_choking {
                                                theme::MAROON
                                            } else {
                                                theme::SURFACE1
                                            }),
                                        ),
                                        Span::styled(
                                            "■",
                                            Style::default().fg(if peer.peer_interested {
                                                theme::TEAL
                                            } else {
                                                theme::SURFACE1
                                            }),
                                        ),
                                        Span::styled(
                                            "■",
                                            Style::default().fg(if peer.am_choking {
                                                theme::PEACH
                                            } else {
                                                theme::SURFACE1
                                            }),
                                        ),
                                    ])
                                    .into(),
                                    PeerColumnId::Address => {
                                        let display = if app_state.anonymize_torrent_names {
                                            "xxx.xxx..."
                                        } else {
                                            &peer.address
                                        };
                                        Cell::from(display.to_string())
                                    }
                                    PeerColumnId::Client => {
                                        Cell::from(parse_peer_id(&peer.peer_id))
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
                        Row::new(cells).style(Style::default().fg(row_color))
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
    }
}

fn draw_footer(f: &mut Frame, app_state: &AppState, settings: &Settings, footer_chunk: Rect) {
    let show_branding = footer_chunk.width >= 80;

    let footer_layout = if show_branding {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(30),    // Fixed space for Branding
                Constraint::Min(0),     // Center takes all remaining space
                Constraint::Length(21), // Fixed space for Port Status
            ])
            .split(footer_chunk)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(0),  // Hidden
                Constraint::Min(0),     // Maximize center
                Constraint::Length(21), // Keep Status
            ])
            .split(footer_chunk)
    };

    let client_id_chunk = footer_layout[0];
    let commands_chunk = footer_layout[1];
    let status_chunk = footer_layout[2];

    // --- LEFT: Branding ---
    if show_branding {
        let _current_dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
        let _current_ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);

        let client_display_line = if let Some(new_version) = &app_state.update_available {
            // REPLACE branding with update message and crossed-out old version
            Line::from(vec![
                Span::styled(
                    "UPDATE AVAILABLE: ",
                    Style::default().fg(theme::YELLOW).bold(),
                ),
                Span::styled(
                    format!("v{}", APP_VERSION),
                    Style::default()
                        .fg(theme::SURFACE2)
                        .add_modifier(Modifier::CROSSED_OUT),
                ),
                Span::styled(
                    format!(" v{}", new_version),
                    Style::default().fg(theme::YELLOW).bold(),
                ),
                Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
                Span::styled(
                    app_state.data_rate.to_string(),
                    Style::default().fg(theme::SUBTEXT1),
                ),
            ])
        } else {
            // Standard branding logic (Existing implementation)
            #[cfg(all(feature = "dht", feature = "pex"))]
            {
                Line::from(vec![
                    Span::styled(
                        "super",
                        speed_to_style(_current_dl_speed).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "seedr",
                        speed_to_style(_current_ul_speed).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" v{}", APP_VERSION),
                        Style::default().fg(theme::SUBTEXT1),
                    ),
                    Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
                    Span::styled(
                        app_state.data_rate.to_string(),
                        Style::default().fg(theme::YELLOW).bold(),
                    ),
                ])
            }
            #[cfg(not(all(feature = "dht", feature = "pex")))]
            {
                Line::from(vec![
                    Span::styled("superseedr", Style::default().fg(theme::SURFACE2))
                        .add_modifier(Modifier::CROSSED_OUT),
                    Span::styled(
                        " [PRIVATE]",
                        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" v{}", APP_VERSION),
                        Style::default().fg(theme::SUBTEXT1),
                    ),
                    Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
                    Span::styled(
                        app_state.data_rate.to_string(),
                        Style::default().fg(theme::YELLOW).bold(),
                    ),
                ])
            }
        };

        let client_id_paragraph = Paragraph::new(client_display_line).alignment(Alignment::Left);
        f.render_widget(client_id_paragraph, client_id_chunk);
    }

    // --- CENTER: Size-Aware Commands ---
    let width = commands_chunk.width;
    let mut spans = Vec::new();

    // Priority 4 (Lowest): Aux tools
    if width > 110 {
        spans.extend(vec![
            Span::styled("[t]", Style::default().fg(theme::SAPPHIRE)),
            Span::raw("ime | "),
            Span::styled("[/]", Style::default().fg(theme::YELLOW)),
            Span::raw("search | "),
            Span::styled("[c]", Style::default().fg(theme::LAVENDER)),
            Span::raw("onfig | "),
        ]);
    }

    // Priority 3: Management
    if width > 90 {
        spans.extend(vec![
            Span::styled("[a]", Style::default().fg(theme::GREEN)),
            Span::raw("dd | "),
            Span::styled("[d]", Style::default().fg(theme::YELLOW)),
            Span::raw("elete | "),
            Span::styled("[s]", Style::default().fg(theme::MAUVE)),
            Span::raw("ort | "),
        ]);
    }

    // Priority 2: Actions
    if width > 65 {
        spans.extend(vec![
            Span::styled("[v]", Style::default().fg(theme::TEAL)),
            Span::raw("paste | "),
            Span::styled("[p]", Style::default().fg(theme::GREEN)),
            Span::raw("ause | "),
        ]);
    }

    // Priority 1: Navigation & Quit
    if width > 45 {
        spans.extend(vec![
            Span::styled("Arrows", Style::default().fg(theme::BLUE)),
            Span::raw(" nav | "),
            Span::styled("[q]", Style::default().fg(theme::RED)),
            Span::raw("uit | "),
        ]);
    }

    // Priority 0: Help (Always Shown)
    if app_state.system_warning.is_some() {
        spans.extend(vec![
            Span::styled("[m]", Style::default().fg(theme::TEAL)),
            Span::styled("anual (warning)", Style::default().fg(theme::YELLOW)),
        ]);
    } else {
        spans.extend(vec![
            Span::styled("[m]", Style::default().fg(theme::TEAL)),
            Span::raw("anual"),
        ]);
    }

    let footer_paragraph = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center) // Center whatever commands fit
        .style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(footer_paragraph, commands_chunk);

    // --- RIGHT: Port Status ---
    let port_style = if app_state.externally_accessable_port {
        Style::default().fg(theme::GREEN)
    } else {
        Style::default().fg(theme::RED)
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

    let status_paragraph =
        Paragraph::new(footer_status).style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(status_paragraph, status_chunk);
}

fn draw_peer_stream(f: &mut Frame, app_state: &AppState, area: Rect) {
    if area.height < 3 || area.width < 10 {
        return;
    }

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
            .title(Span::styled(
                "Peer Stream",
                Style::default().fg(color_title),
            ))
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
        Span::styled("Connected:", Style::default().fg(color_connected)),
        Span::styled(
            connected_count.to_string(),
            Style::default().fg(color_connected),
        ),
        Span::raw(" "),
        Span::styled("Discovered:", Style::default().fg(color_discovered)),
        Span::styled(
            discovered_count.to_string(),
            Style::default().fg(color_discovered),
        ),
        Span::raw(" "),
        Span::styled("Disconnected:", Style::default().fg(color_disconnected)),
        Span::styled(
            disconnected_count.to_string(),
            Style::default().fg(color_disconnected),
        ),
        Span::raw(" "),
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
        if v == 0 {
            continue;
        }
        let norm_val = v as f64 / max_disc;
        let y_val = y_discovered;
        if norm_val < 0.33 {
            disc_data_light.push((i as f64, y_val));
        } else if norm_val < 0.66 {
            disc_data_medium.push((i as f64, y_val));
        } else {
            disc_data_dark.push((i as f64, y_val));
        }
    }

    for (i, &v) in conn_slice.iter().enumerate() {
        if v == 0 {
            continue;
        }
        let norm_val = v as f64 / max_conn;
        let y_val = y_connected;
        if norm_val < 0.33 {
            conn_data_light.push((i as f64, y_val));
        } else if norm_val < 0.66 {
            conn_data_medium.push((i as f64, y_val));
        } else {
            conn_data_dark.push((i as f64, y_val));
        }
    }

    for (i, &v) in disconn_slice.iter().enumerate() {
        if v == 0 {
            continue;
        }
        let norm_val = v as f64 / max_disconn;
        let y_val = y_disconnected;
        if norm_val < 0.33 {
            disconn_data_light.push((i as f64, y_val));
        } else if norm_val < 0.66 {
            disconn_data_medium.push((i as f64, y_val));
        } else {
            disconn_data_dark.push((i as f64, y_val));
        }
    }

    let datasets = vec![
        Dataset::default()
            .data(&disc_data_light)
            .marker(small_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::DIM),
            ),
        Dataset::default()
            .data(&disc_data_medium)
            .marker(medium_marker)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(color_discovered)),
        Dataset::default()
            .data(&disc_data_dark)
            .marker(large_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::BOLD),
            ),
        Dataset::default()
            .data(&conn_data_light)
            .marker(small_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::DIM),
            ),
        Dataset::default()
            .data(&conn_data_medium)
            .marker(medium_marker)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(color_connected)),
        Dataset::default()
            .data(&conn_data_dark)
            .marker(large_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::BOLD),
            ),
        Dataset::default()
            .data(&disconn_data_light)
            .marker(small_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::DIM),
            ),
        Dataset::default()
            .data(&disconn_data_medium)
            .marker(medium_marker)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(color_disconnected)),
        Dataset::default()
            .data(&disconn_data_dark)
            .marker(large_marker)
            .graph_type(GraphType::Scatter)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::BOLD),
            ),
    ];

    let discovery_chart = Chart::new(datasets)
        .block(
            Block::default()
                .title_top(
                    Line::from(Span::styled(
                        "Peer Stream",
                        Style::default().fg(color_title),
                    ))
                    .alignment(Alignment::Left),
                )
                .title_top(legend_line.alignment(Alignment::Right))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color_border)),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(color_axis))
                .bounds([0.0, disc_slice.len().saturating_sub(1) as f64]),
        )
        .y_axis(Axis::default().bounds([0.5, 3.5]));

    f.render_widget(discovery_chart, area);
}

fn draw_vertical_block_stream(f: &mut Frame, app_state: &AppState, area: Rect) {
    if area.width < 2 {
        return;
    }
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
        vec![Span::styled(
            title_str,
            Style::default().fg(theme::SUBTEXT0),
        )]
    } else {
        let blue_ratio = total_in as f64 / total_ops as f64;
        let blue_chars = (blue_ratio * title_len as f64).round() as usize;
        let (blue_part, green_part) = title_str.split_at(blue_chars.min(title_len));
        let mut spans = Vec::new();
        if !blue_part.is_empty() {
            spans.push(Span::styled(blue_part, Style::default().fg(color_inflow)));
        }
        if !green_part.is_empty() {
            spans.push(Span::styled(green_part, Style::default().fg(color_outflow)));
        }
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
            spans.push(Span::styled(SEPARATOR, Style::default().fg(color_empty)));
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

fn draw_torrent_sparklines(f: &mut Frame, app_state: &AppState, area: Rect) {
    let torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let Some(torrent) = torrent else {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE2));
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
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("DL Activity (Peak: {})", format_speed(nice_max_speed)),
                        Style::default().fg(theme::SUBTEXT0),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE2)),
            )
            .data(dl_slice)
            .max(nice_max_speed)
            .style(Style::default().fg(theme::BLUE));
        f.render_widget(dl_sparkline, area);
    } else if !has_dl_activity && has_ul_activity {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let ul_slice = &ul_history[ul_history.len().saturating_sub(width)..];
        let max_speed = ul_slice.iter().max().copied().unwrap_or(1);
        let nice_max_speed = calculate_nice_upper_bound(max_speed).max(1);
        let ul_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("UL Activity (Peak: {})", format_speed(nice_max_speed)),
                        Style::default().fg(theme::SUBTEXT0),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE2)),
            )
            .data(ul_slice)
            .max(nice_max_speed)
            .style(Style::default().fg(theme::GREEN));
        f.render_widget(ul_sparkline, area);
    } else if !has_dl_activity && !has_ul_activity {
        let style = Style::default().fg(theme::MAUVE);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE2));
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
        let throbber_style = Style::default()
            .fg(theme::LAVENDER)
            .add_modifier(Modifier::BOLD);
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

        let dl_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("DL (Peak: {})", format_speed(dl_nice_max)),
                        Style::default().fg(theme::SUBTEXT0),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE2)),
            )
            .data(dl_slice)
            .max(dl_nice_max)
            .style(Style::default().fg(theme::BLUE));
        f.render_widget(dl_sparkline, dl_sparkline_chunk);

        let ul_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("UL (Peak: {})", format_speed(ul_nice_max)),
                        Style::default().fg(theme::SUBTEXT0),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE2)),
            )
            .data(ul_slice)
            .max(ul_nice_max)
            .style(Style::default().fg(theme::GREEN));
        f.render_widget(ul_sparkline, ul_sparkline_chunk);
    }
}

fn draw_delete_confirm_dialog(f: &mut Frame, app_state: &AppState) {
    if let AppMode::DeleteConfirm {
        info_hash,
        with_files,
    } = &app_state.mode
    {
        if let Some(torrent_to_delete) = app_state.torrents.get(info_hash) {
            let terminal_area = f.area();

            // Adaptive scaling: use more screen percentage on smaller windows
            let rect_width = if terminal_area.width < 60 { 90 } else { 50 };
            let rect_height = if terminal_area.height < 20 { 95 } else { 18 };

            let area = centered_rect(rect_width, rect_height, terminal_area);
            f.render_widget(Clear, area);

            // Adaptive padding: remove vertical padding if space is tight
            let vert_padding = if area.height < 10 { 0 } else { 1 };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::RED))
                .padding(Padding::new(2, 2, vert_padding, vert_padding));

            let inner_area = block.inner(area);
            f.render_widget(block, area);

            // Use Min(0) for the middle chunk to allow it to collapse on small screens
            let chunks = Layout::vertical([
                Constraint::Length(2), // Torrent Name & Path
                Constraint::Min(0),    // Warning Body (Collapses if needed)
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Keybinds (Pinned footer)
            ])
            .split(inner_area);

            // 1. Torrent Identity
            let name = &torrent_to_delete.latest_state.torrent_name;
            let path = torrent_to_delete
                .latest_state
                .download_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown Path".to_string());

            f.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        name,
                        Style::default().fg(theme::YELLOW).bold().underlined(),
                    )),
                    Line::from(Span::styled(path, Style::default().fg(theme::TEXT))),
                ])
                .alignment(Alignment::Center),
                chunks[0],
            );

            // 2. Warning Body (Only render if there is enough height)
            if chunks[1].height > 0 {
                let body = if *with_files {
                    vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "⚠️ PERMANENT TORRENT FILES DELETION ON ⚠️",
                            Style::default().fg(theme::RED).bold(),
                        )),
                        Line::from(vec![
                            Span::raw("All local data for this torrent will be "),
                            Span::styled(
                                "ERASED",
                                Style::default().fg(theme::RED).bold().underlined(),
                            ),
                        ]),
                    ]
                } else {
                    vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "Safe Removal (Files Kept)",
                            Style::default().fg(theme::GREEN),
                        )),
                        Line::from(vec![
                            Span::raw("Use "),
                            Span::styled("[D]", Style::default().fg(theme::YELLOW).bold()),
                            Span::raw(" to remove files..."),
                        ]),
                    ]
                };
                f.render_widget(
                    Paragraph::new(body)
                        .alignment(Alignment::Center)
                        .wrap(Wrap { trim: true }),
                    chunks[1],
                );
            }

            // 3. Action Buttons (Footer)
            let actions = Line::from(vec![
                Span::styled("[Enter]", Style::default().fg(theme::GREEN).bold()),
                Span::raw(" Confirm  "),
                Span::styled("[Esc]", Style::default().fg(theme::RED)),
                Span::raw(" Cancel"),
            ]);

            f.render_widget(
                Paragraph::new(actions).alignment(Alignment::Center),
                chunks[3],
            );
        }
    }
}

fn draw_status_error_popup(f: &mut Frame, error_text: &str) {
    let popup_width_percent: u16 = 50;
    let popup_height: u16 = 8;
    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(popup_height),
        Constraint::Min(0),
    ])
    .split(f.area());
    let area = Layout::horizontal([
        Constraint::Percentage((100 - popup_width_percent) / 2),
        Constraint::Percentage(popup_width_percent),
        Constraint::Percentage((100 - popup_width_percent) / 2),
    ])
    .split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let text = vec![
        Line::from(Span::styled(
            "Error",
            Style::default().fg(theme::RED).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(error_text, Style::default().fg(theme::YELLOW))),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "[Press Esc to dismiss]",
            Style::default().fg(theme::SUBTEXT1),
        )),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::RED));
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn draw_shutdown_screen(f: &mut Frame, app_state: &AppState) {
    const POPUP_WIDTH: u16 = 40;
    const POPUP_HEIGHT: u16 = 3;
    let area = f.area();
    let width = POPUP_WIDTH.min(area.width);
    let height = POPUP_HEIGHT.min(area.height);
    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(height),
        Constraint::Min(0),
    ])
    .split(area);
    let area = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(width),
        Constraint::Min(0),
    ])
    .split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let container_block = Block::default()
        .title(Span::styled(" Exiting ", Style::default().fg(theme::PEACH)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = container_block.inner(area);
    f.render_widget(container_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1)])
        .split(inner_area);
    let progress_label = format!("{:.0}%", (app_state.shutdown_progress * 100.0).min(100.0));
    let progress_bar = Gauge::default()
        .ratio(app_state.shutdown_progress)
        .label(progress_label)
        .gauge_style(Style::default().fg(theme::MAUVE).bg(theme::SURFACE0));
    f.render_widget(progress_bar, chunks[0]);
}

fn draw_power_saving_screen(f: &mut Frame, app_state: &AppState, settings: &Settings) {
    const TRANQUIL_MESSAGES: &[&str] = &[
        "Quietly seeding...",
        "Awaiting peers...",
        "Sharing data...",
        "Connecting to the swarm...",
        "Sharing pieces...",
        "The network is vast...",
        "Listening for connections...",
        "Seeding the cloud...",
        "Uptime is a gift...",
        "Data flows...",
        "Maintaining the ratio...",
        "A torrent of tranquility...",
        "A piece at a time...",
        "The swarm is peaceful...",
        "Be the torrent...",
        "Nurturing the swarm...",
        "Awaiting the handshake...",
        "Distributing packets...",
        "The ratio is balanced...",
        "Each piece finds its home...",
        "Announcing to the tracker...",
        "The bitfield is complete...",
    ];

    let dl_speed = *app_state.avg_download_history.last().unwrap_or(&0);
    let ul_speed = *app_state.avg_upload_history.last().unwrap_or(&0);
    let dl_limit = settings.global_download_limit_bps;
    let ul_limit = settings.global_upload_limit_bps;

    let area = centered_rect(40, 60, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE1));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(8),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner_area);
    let content_area = vertical_chunks[1];
    let footer_area = vertical_chunks[3];

    let mut dl_spans = vec![
        Span::styled("DL: ", Style::default().fg(theme::SKY)),
        Span::styled(format_speed(dl_speed), Style::default().fg(theme::SKY)),
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

    let mut ul_spans = vec![
        Span::styled("UL: ", Style::default().fg(theme::TEAL)),
        Span::styled(format_speed(ul_speed), Style::default().fg(theme::TEAL)),
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

    const MESSAGE_INTERVAL_SECONDS: u64 = 500;
    let seconds_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let seed = seconds_since_epoch / MESSAGE_INTERVAL_SECONDS;
    let mut rng = StdRng::seed_from_u64(seed);
    let message_index = rng.random_range(0..TRANQUIL_MESSAGES.len());
    let current_message = TRANQUIL_MESSAGES[message_index];

    let main_content_lines = vec![
        Line::from(vec![
            Span::styled("super", Style::default().fg(theme::SKY)),
            Span::styled("seedr", Style::default().fg(theme::TEAL)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            current_message,
            Style::default().fg(theme::SUBTEXT1),
        )),
        Line::from(""),
        Line::from(dl_spans),
        Line::from(ul_spans),
    ];
    let main_paragraph = Paragraph::new(main_content_lines).alignment(Alignment::Center);
    let footer_line = Line::from(Span::styled(
        "Press [z] to resume",
        Style::default().fg(theme::SUBTEXT0),
    ));
    let footer_paragraph = Paragraph::new(footer_line).alignment(Alignment::Center);

    f.render_widget(main_paragraph, content_area);
    f.render_widget(footer_paragraph, footer_area);
}

pub fn draw_file_browser(
    f: &mut Frame,
    app_state: &AppState,
    state: &tree::TreeViewState,
    data: &[tree::RawNode<FileMetadata>],
    browser_mode: &FileBrowserMode,
) {
    let has_preview_content = match browser_mode {
        FileBrowserMode::DownloadLocSelection { .. } => {
            app_state.pending_torrent_path.is_some() || !app_state.pending_torrent_link.is_empty()
        }
        FileBrowserMode::File(_) => state
            .cursor_path
            .as_ref()
            .is_some_and(|p| p.extension().is_some_and(|ext| ext == "torrent")),
        _ => false,
    };

    let preview_file_path = match browser_mode {
        FileBrowserMode::DownloadLocSelection { .. } => app_state.pending_torrent_path.as_ref(),
        FileBrowserMode::File(_) => state.cursor_path.as_ref(),
        _ => None,
    };

    let default_pane = BrowserPane::FileSystem;
    let focused_pane =
        if let FileBrowserMode::DownloadLocSelection { focused_pane, .. } = browser_mode {
            focused_pane
        } else {
            &default_pane
        };

    let max_area = centered_rect(90, 80, f.area());
    f.render_widget(Clear, max_area);

    let area = if has_preview_content {
        if f.area().width < 60 {
            f.area()
        } else {
            centered_rect(90, 80, f.area())
        }
    } else if f.area().width < 40 {
        f.area()
    } else {
        centered_rect(75, 80, f.area())
    };

    let layout = calculate_file_browser_layout(
        area,
        has_preview_content,
        app_state.is_searching,
        focused_pane,
    );

    // Styles logic (omitted for brevity, same as your source)
    let (files_border_style, preview_border_style) =
        if let FileBrowserMode::DownloadLocSelection { focused_pane, .. } = browser_mode {
            match focused_pane {
                BrowserPane::FileSystem => (
                    Style::default().fg(theme::MAUVE),
                    Style::default().fg(theme::SURFACE2),
                ),
                BrowserPane::TorrentPreview => (
                    Style::default().fg(theme::SURFACE2),
                    Style::default().fg(theme::MAUVE),
                ),
            }
        } else {
            (
                Style::default().fg(theme::MAUVE),
                Style::default().fg(theme::SAPPHIRE),
            )
        };

    if let Some(preview_area) = layout.preview {
        draw_torrent_preview_panel(
            f,
            preview_area,
            preview_file_path.map(|p| p.as_path()), // Passes Option<&Path>
            browser_mode,
            preview_border_style,
            &state.current_path,
        );
    }
    if let Some(search_area) = layout.search {
        let search_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::YELLOW))
            .title(" Search Filter ");
        let search_text = Line::from(vec![
            Span::styled("/", Style::default().fg(theme::SUBTEXT0)),
            Span::raw(&app_state.search_query),
            Span::styled(
                "_",
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]);
        f.render_widget(Paragraph::new(search_text).block(search_block), search_area);
    }

    let mut footer_spans = Vec::new();
    match browser_mode {
        FileBrowserMode::ConfigPathSelection { .. } | FileBrowserMode::Directory => {
            footer_spans.push(Span::styled(
                "[Arrows/Vim]",
                Style::default().fg(theme::BLUE),
            ));
            footer_spans.push(Span::raw(" Nav | "));
            footer_spans.push(Span::styled(
                "[Backspace]",
                Style::default().fg(theme::YELLOW),
            ));
            footer_spans.push(Span::raw(" Up | "));
            footer_spans.push(Span::styled("[Enter]", Style::default().fg(theme::YELLOW)));
            footer_spans.push(Span::raw(" Down | "));
            footer_spans.push(Span::styled("[Shift+Y]", Style::default().fg(theme::GREEN)));
            footer_spans.push(Span::raw(" Confirm Selection | "));
        }
        FileBrowserMode::DownloadLocSelection {
            focused_pane,
            use_container,
            ..
        } => {
            footer_spans.push(Span::styled("[Tab]", Style::default().fg(theme::SAPPHIRE)));
            footer_spans.push(Span::raw(" Switch Pane | "));

            match focused_pane {
                BrowserPane::FileSystem => {
                    // Help for FS is generally just Arrows/Enter which are intuitive
                }
                BrowserPane::TorrentPreview => {
                    footer_spans.push(Span::styled("[Space]", Style::default().fg(theme::YELLOW)));
                    footer_spans.push(Span::raw(" Priority | "));
                }
            }

            footer_spans.push(Span::styled("[x]", Style::default().fg(theme::MAUVE)));
            footer_spans.push(Span::raw(" Container Folder | "));

            if *use_container {
                footer_spans.push(Span::styled("[r]", Style::default().fg(theme::SKY)));
                footer_spans.push(Span::raw(" Rename | "));
            }

            footer_spans.push(Span::styled("[Shift+Y]", Style::default().fg(theme::GREEN)));
            footer_spans.push(Span::raw(" Confirm"));
        }
        FileBrowserMode::File(_) => {
            // Changed [Enter] to [c]
            footer_spans.push(Span::styled("[Shift+Y]", Style::default().fg(theme::GREEN)));
            footer_spans.push(Span::raw(" Confirm File | "));
        }
    }
    footer_spans.push(Span::raw(" | "));
    footer_spans.push(Span::styled("[Esc]", Style::default().fg(theme::RED)));
    footer_spans.push(Span::raw(" Cancel"));

    let footer = Paragraph::new(Line::from(footer_spans))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(footer, layout.footer);

    // 6. DRAW LIST (SANITIZED & SIMPLIFIED)
    let inner_height = layout.list.height.saturating_sub(2) as usize;
    let list_width = layout.list.width.saturating_sub(2) as usize;

    let filter = match browser_mode {
        FileBrowserMode::Directory
        | FileBrowserMode::DownloadLocSelection { .. }
        | FileBrowserMode::ConfigPathSelection { .. } => {
            TreeFilter::from_text(&app_state.search_query)
        }
        FileBrowserMode::File(extensions) => {
            let exts = extensions.clone();
            tree::TreeFilter::new(&app_state.search_query, move |node| {
                node.is_dir
                    || exts
                        .iter()
                        .any(|ext| node.name.to_lowercase().ends_with(ext))
            })
        }
    };

    let abs_path = state.current_path.to_string_lossy();
    let item_count = data.len();
    let count_label = if item_count == 0 {
        " (empty)".to_string()
    } else {
        format!(" ({} items)", item_count)
    };
    let left_title = format!(" {}/{} ", abs_path, count_label);

    // ENHANCEMENT 3: Removed "Select Download Location" from right title
    let right_title = match browser_mode {
        FileBrowserMode::Directory => " Select Directory ".to_string(),
        FileBrowserMode::DownloadLocSelection { .. } => String::new(), // <--- Empty now
        FileBrowserMode::ConfigPathSelection { .. } => " Select Config Path ".to_string(),
        FileBrowserMode::File(exts) => format!(" Select File [{}] ", exts.join(", ")),
    };

    let layout = calculate_file_browser_layout(
        area,
        has_preview_content,
        app_state.is_searching,
        focused_pane,
    );

    let visible_items = TreeMathHelper::get_visible_slice(data, state, filter, inner_height);
    let mut list_items = Vec::new();

    if data.is_empty() {
        list_items.push(ListItem::new(Line::from(vec![Span::styled(
            "   (Directory is empty)",
            Style::default().fg(theme::OVERLAY0).italic(),
        )])));
    } else if visible_items.is_empty() {
        list_items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("   (No matching files among {} items)", item_count),
            Style::default().fg(theme::OVERLAY0).italic(),
        )])));
    } else {
        for item in visible_items {
            let is_cursor = item.is_cursor;

            // 1. Basic Dimensions
            let indent_str = "  ".repeat(item.depth);
            let indent_len = indent_str.len();
            let icon_str = if item.node.is_dir {
                "  "
            } else {
                "   "
            };
            let icon_len = 4;

            // 2. Prepare Date String
            let (meta_str, meta_len) = if !item.node.is_dir {
                let datetime: chrono::DateTime<chrono::Local> = item.node.payload.modified.into();
                let size_str = format_bytes(item.node.payload.size);
                let s = format!(" {} ({})", size_str, datetime.format("%b %d %H:%M"));
                (s.clone(), s.len())
            } else {
                (String::new(), 0)
            };

            // 3. Determine available space for filename
            let fixed_used = indent_len + icon_len + meta_len + 1;
            let available_for_name = list_width.saturating_sub(fixed_used);

            // SANITIZE FILENAME (Fix for Icon\r etc)
            let clean_name: String = item
                .node
                .name
                .chars()
                .map(|c| if c.is_control() { '?' } else { c })
                .collect();

            // 4. Truncate name
            let display_name = truncate_with_ellipsis(&clean_name, available_for_name);

            // 5. Styles
            let (icon_style, text_style) = if is_cursor {
                (
                    Style::default()
                        .fg(theme::YELLOW)
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .fg(theme::YELLOW)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                let i_style = if item.node.is_dir {
                    Style::default().fg(theme::BLUE)
                } else {
                    Style::default().fg(theme::TEXT)
                };
                (i_style, Style::default().fg(theme::TEXT))
            };

            // 6. Construct Line
            let mut line_spans = vec![
                Span::raw(indent_str),
                Span::styled(icon_str, icon_style),
                Span::styled(display_name, text_style),
            ];

            // Simply append the date with a single space if it's a file
            if !item.node.is_dir {
                line_spans.push(Span::raw(" "));
                line_spans.push(Span::styled(
                    meta_str,
                    Style::default().fg(theme::SURFACE2).italic(),
                ));
            }

            list_items.push(ListItem::new(Line::from(line_spans)));
        }
    }

    f.render_widget(
        List::new(list_items)
            .block(
                Block::default()
                    .title_top(
                        Line::from(Span::styled(
                            left_title,
                            Style::default().fg(theme::MAUVE).bold(),
                        ))
                        .alignment(Alignment::Left),
                    )
                    .title_top(
                        Line::from(Span::styled(
                            right_title,
                            Style::default().fg(theme::MAUVE).italic(),
                        ))
                        .alignment(Alignment::Right),
                    )
                    .borders(Borders::ALL)
                    .border_style(files_border_style),
            )
            .highlight_symbol("▶ "),
        layout.list,
    );
}

fn draw_torrent_preview_panel(
    f: &mut Frame,
    area: Rect,
    path: Option<&std::path::Path>,
    browser_mode: &FileBrowserMode,
    border_style: Style,
    current_fs_path: &std::path::Path,
) {
    let is_narrow = area.width < 50; // Threshold for vertical/narrow mode
    let raw_title = "Torrent Preview";

    // Dynamically truncate title based on available width
    let avail_width = area.width.saturating_sub(4) as usize;
    let title = if is_narrow {
        truncate_with_ellipsis("Preview", avail_width)
    } else {
        truncate_with_ellipsis(raw_title, avail_width)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // --- CASE A: Interactive Mode (Download Location Selection) ---
    if let FileBrowserMode::DownloadLocSelection {
        preview_tree,
        preview_state,
        container_name,
        use_container,
        is_editing_name,
        cursor_pos,
        ..
    } = browser_mode
    {
        let filter = tree::TreeFilter::default();
        let header_lines = if *use_container { 2 } else { 1 };
        let list_height = inner_area.height.saturating_sub(header_lines) as usize;

        let visible_rows =
            TreeMathHelper::get_visible_slice(preview_tree, preview_state, filter, list_height);

        let mut list_items = Vec::new();

        // Render Root Node with adaptive path display
        let root_style = Style::default()
            .fg(theme::BLUE)
            .add_modifier(Modifier::BOLD);

        let path_display = if is_narrow {
            current_fs_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string())
        } else {
            current_fs_path.to_string_lossy().to_string()
        };

        list_items.push(ListItem::new(Line::from(vec![
            Span::styled("▼  ", root_style),
            Span::styled(path_display, root_style),
        ])));

        // Render Container Header
        if *use_container {
            let container_style = if *is_editing_name {
                Style::default().fg(theme::SKY).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD)
            };

            let mut spans = vec![Span::raw("  "), Span::styled("▼  ", container_style)];

            if *is_editing_name {
                let (before, after) = container_name.split_at(*cursor_pos);
                spans.push(Span::styled(before, container_style));
                spans.push(Span::styled(
                    "█",
                    Style::default()
                        .fg(theme::SKY)
                        .add_modifier(Modifier::SLOW_BLINK),
                ));
                spans.push(Span::styled(after, container_style));
            } else {
                spans.push(Span::styled(container_name.clone(), container_style));
                if !is_narrow {
                    spans.push(Span::styled(
                        " (New)",
                        Style::default()
                            .fg(theme::SURFACE2)
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
            }
            list_items.push(ListItem::new(Line::from(spans)));
        }

        // Render Tree Items with reduced indentation for narrow screens
        let tree_items: Vec<ListItem> = visible_rows
            .iter()
            .map(|item| {
                let is_cursor = item.is_cursor;
                let base_indent_level = if *use_container { 2 } else { 1 };

                // Reduce indent multiplier from 2 to 1 on narrow screens to save space
                let indent_multiplier = if is_narrow { 1 } else { 2 };
                let indent_str = " ".repeat((base_indent_level + item.depth) * indent_multiplier);

                let icon = if item.node.is_dir {
                    "  "
                } else {
                    "   "
                };

                let (base_content_style, tag) = match item.node.payload.priority {
                    FilePriority::Skip => (
                        Style::default()
                            .fg(theme::SURFACE1)
                            .add_modifier(Modifier::CROSSED_OUT),
                        "[S] ",
                    ),
                    FilePriority::High => (
                        Style::default()
                            .fg(theme::GREEN)
                            .add_modifier(Modifier::BOLD),
                        "[H] ",
                    ),
                    FilePriority::Mixed => (
                        Style::default()
                            .fg(theme::YELLOW)
                            .add_modifier(Modifier::ITALIC),
                        "[*] ",
                    ),
                    FilePriority::Normal => (
                        if item.node.is_dir {
                            Style::default().fg(theme::BLUE)
                        } else {
                            Style::default().fg(theme::TEXT)
                        },
                        "",
                    ),
                };

                let final_content_style = if is_cursor {
                    base_content_style
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED)
                } else {
                    base_content_style
                };

                let structure_style = final_content_style
                    .remove_modifier(Modifier::CROSSED_OUT)
                    .remove_modifier(Modifier::UNDERLINED);
                let mut spans = vec![
                    Span::styled(indent_str, structure_style),
                    Span::styled(icon, structure_style),
                    Span::styled(&item.node.name, final_content_style),
                ];

                if !item.node.is_dir {
                    if !is_narrow {
                        spans.push(Span::styled(
                            format!(" ({}) ", format_bytes(item.node.payload.size)),
                            structure_style,
                        ));
                    }
                    if !tag.is_empty() {
                        spans.push(Span::styled(tag, structure_style));
                    }
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        list_items.extend(tree_items);
        f.render_widget(List::new(list_items), inner_area);
        return;
    }

    // --- CASE B: Static Preview (Browsing .torrent files) ---
    if let Some(p) = path {
        let file_bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                f.render_widget(
                    Paragraph::new(format!("Read Error: {}", e))
                        .style(Style::default().fg(theme::RED)),
                    inner_area,
                );
                return;
            }
        };

        let torrent = match crate::torrent_file::parser::from_bytes(&file_bytes) {
            Ok(t) => t,
            Err(e) => {
                f.render_widget(
                    Paragraph::new(format!("Invalid Torrent: {}", e))
                        .style(Style::default().fg(theme::RED)),
                    inner_area,
                );
                return;
            }
        };

        let total_size = torrent.info.total_length();
        let protocol_version = match torrent.info.meta_version {
            Some(2) => {
                if !torrent.info.pieces.is_empty() {
                    "BitTorrent v2 (Hybrid)"
                } else {
                    "BitTorrent v2 (Pure)"
                }
            }
            _ => "BitTorrent v1",
        };
        let info_text = vec![
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(theme::SUBTEXT0)),
                Span::raw(&torrent.info.name),
            ]),
            Line::from(vec![
                Span::styled("Protocol: ", Style::default().fg(theme::SUBTEXT0)),
                Span::styled(protocol_version, Style::default().fg(theme::MAUVE).bold()),
            ]),
            Line::from(vec![
                Span::styled("Size: ", Style::default().fg(theme::SUBTEXT0)),
                Span::raw(format_bytes(total_size as u64)),
            ]),
        ];

        let layout = Layout::vertical([
            Constraint::Length(info_text.len() as u16 + 1),
            Constraint::Min(0),
        ])
        .split(inner_area);
        f.render_widget(
            Paragraph::new(info_text).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme::SURFACE2)),
            ),
            layout[0],
        );

        let file_list_payloads: Vec<(Vec<String>, crate::app::TorrentPreviewPayload)> = torrent
            .file_list()
            .into_iter()
            .map(|(path, size)| {
                (
                    path,
                    crate::app::TorrentPreviewPayload {
                        file_index: None,
                        size,
                        priority: FilePriority::Normal,
                    },
                )
            })
            .collect();

        let final_nodes = crate::tui::tree::RawNode::from_path_list(None, file_list_payloads);
        let mut temp_state = crate::tui::tree::TreeViewState::default();
        for node in &final_nodes {
            node.expand_all(&mut temp_state);
        }

        let visible_rows = TreeMathHelper::get_visible_slice(
            &final_nodes,
            &temp_state,
            tree::TreeFilter::default(),
            layout[1].height as usize,
        );

        let list_items: Vec<ListItem> = visible_rows
            .iter()
            .map(|item| {
                let indent = if is_narrow {
                    " ".repeat(item.depth)
                } else {
                    "  ".repeat(item.depth)
                };
                let icon = if item.node.is_dir {
                    "  "
                } else {
                    "   "
                };
                let style = if item.node.is_dir {
                    Style::default().fg(theme::BLUE)
                } else {
                    Style::default().fg(theme::TEXT)
                };
                let mut spans = vec![
                    Span::raw(indent),
                    Span::styled(icon, style),
                    Span::styled(&item.node.name, style),
                ];
                if !item.node.is_dir && !is_narrow {
                    spans.push(Span::styled(
                        format!(" ({})", format_bytes(item.node.payload.size)),
                        Style::default().fg(theme::SURFACE2),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        f.render_widget(List::new(list_items), layout[1]);
    }
}

fn draw_welcome_screen(f: &mut Frame) {
    let text = vec![
        Line::from(Span::styled(
            "A BitTorrent Client in your Terminal",
            Style::default().add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "How to Get Started:",
            Style::default().fg(theme::YELLOW).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ★ ", Style::default().fg(theme::GREEN)),
            Span::raw("Paste ("),
            Span::styled("Ctrl+V", Style::default().fg(theme::SKY).bold()),
            Span::raw(") a "),
            Span::styled("magnet link", Style::default().fg(theme::PEACH)),
            Span::raw(" from your clipboard."),
        ]),
        Line::from(vec![
            Span::raw("      - "),
            Span::styled(
                "e.g. \"magnet:?xt=urn:btih:...\"",
                Style::default()
                    .fg(theme::SURFACE2)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]),
        Line::from(vec![
            Span::styled(" ★ ", Style::default().fg(theme::GREEN)),
            Span::raw("Press "),
            Span::styled("[a]", Style::default().fg(theme::MAUVE).bold()),
            Span::raw(" to open the file picker and select a "),
            Span::styled("`.torrent`", Style::default().fg(theme::PEACH)),
            Span::raw(" file."),
        ]),
        Line::from(vec![
            Span::styled(" ★ ", Style::default().fg(theme::GREEN)),
            Span::raw("Use the "),
            Span::styled("CLI", Style::default().fg(theme::SKY).bold()),
            Span::raw(" from another terminal:"),
        ]),
        // Sub-bullet 1: CLI Magnet
        Line::from(vec![
            Span::raw("      - magnet: "),
            Span::styled(
                "superseedr add \"magnet:?xt=urn:btih:...\"",
                Style::default().fg(theme::SURFACE2),
            ),
        ]),
        // Sub-bullet 2: CLI Torrent File
        Line::from(vec![
            Span::raw("      - file:   "),
            Span::styled(
                "superseedr add \"/path/to/my.torrent\"",
                Style::default().fg(theme::SURFACE2),
            ),
        ]),
        Line::from(vec![
            Span::styled(" ★ ", Style::default().fg(theme::GREEN)),
            Span::raw("Drop files into your "),
            Span::styled("Watch Folder", Style::default().fg(theme::SKY).bold()),
            Span::raw(" to add them automatically."),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Browser Support: ",
                Style::default().fg(theme::YELLOW).bold(),
            ),
            Span::raw("To open magnet links directly from your browser,"),
        ]),
        Line::from(vec![
            Span::raw("   natively install superseedr: "),
            Span::styled(
                "https://github.com/Jagalite/superseedr/releases",
                Style::default().fg(theme::BLUE).underlined(),
            ),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [m] ", Style::default().fg(theme::TEAL)),
            Span::styled("Manual/Help", Style::default().fg(theme::SUBTEXT1)),
            Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
            Span::styled(" [c] ", Style::default().fg(theme::MAUVE)),
            Span::styled("Config", Style::default().fg(theme::SUBTEXT1)),
            Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
            Span::styled(" [q] ", Style::default().fg(theme::RED)), // Added Quit command
            Span::styled("Quit", Style::default().fg(theme::SUBTEXT1)),
            Span::styled(" | ", Style::default().fg(theme::SURFACE2)),
            Span::styled(" [Esc] ", Style::default().fg(theme::RED)),
            Span::styled("Dismiss", Style::default().fg(theme::SUBTEXT1)),
        ]),
    ];

    // ... (rest of the centering and rendering logic remains the same)
    let text_height = text.len() as u16;
    let text_width = text.iter().map(|line| line.width()).max().unwrap_or(0) as u16;
    let horizontal_padding: u16 = 4;
    let vertical_padding: u16 = 2;
    let box_width = (text_width + horizontal_padding + 2).min(f.area().width);
    let box_height = (text_height + vertical_padding + 2).min(f.area().height);

    let vertical_chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(box_height),
        Constraint::Min(0),
    ])
    .split(f.area());
    let area = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(box_width),
        Constraint::Min(0),
    ])
    .split(vertical_chunks[1])[1];

    f.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(
            " Welcome to superseedr! ",
            Style::default().fg(theme::MAUVE).bold(),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let vertical_chunks_inner = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(text_height),
        Constraint::Min(0),
    ])
    .split(inner_area);
    let horizontal_chunks_inner = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(text_width),
        Constraint::Min(0),
    ])
    .split(vertical_chunks_inner[1]);
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(theme::TEXT))
        .alignment(Alignment::Left);
    f.render_widget(paragraph, horizontal_chunks_inner[1]);
}

fn draw_help_popup(f: &mut Frame, app_state: &AppState) {
    let (settings_path_str, log_path_str) = if let Some((config_dir, data_dir)) = get_app_paths() {
        (
            config_dir
                .join("settings.toml")
                .to_string_lossy()
                .to_string(),
            data_dir
                .join("logs")
                .join("app.log")
                .to_string_lossy()
                .to_string(),
        )
    } else {
        (
            "Unknown location".to_string(),
            "Unknown location".to_string(),
        )
    };

    let area = centered_rect(60, 100, f.area());
    f.render_widget(Clear, area);

    if let Some(warning_text) = &app_state.system_warning {
        let warning_width = area.width.saturating_sub(2).max(1) as usize;
        let warning_lines = (warning_text.len() as f64 / warning_width as f64).ceil() as u16;
        let warning_block_height = warning_lines.saturating_add(2).max(3);
        let max_warning_height = (area.height as f64 * 0.25).round() as u16;
        let final_warning_height = warning_block_height.min(max_warning_height);
        let chunks = Layout::vertical([
            Constraint::Length(final_warning_height),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        let warning_paragraph = Paragraph::new(warning_text.as_str())
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::RED)),
            )
            .style(Style::default().fg(theme::YELLOW));
        f.render_widget(warning_paragraph, chunks[0]);
        draw_help_table(f, app_state, chunks[1]);

        let footer_block = Block::default().border_style(Style::default().fg(theme::SURFACE2));
        let footer_inner_area = footer_block.inner(chunks[2]);
        f.render_widget(footer_block, chunks[2]);
        let footer_lines = vec![
            Line::from(vec![
                Span::styled("Settings: ", Style::default().fg(theme::TEXT)),
                Span::styled(
                    truncate_with_ellipsis(
                        &settings_path_str,
                        footer_inner_area.width as usize - 10,
                    ),
                    Style::default().fg(theme::SUBTEXT0),
                ),
            ]),
            Line::from(vec![
                Span::styled("Log File: ", Style::default().fg(theme::TEXT)),
                Span::styled(
                    truncate_with_ellipsis(&log_path_str, footer_inner_area.width as usize - 10),
                    Style::default().fg(theme::SUBTEXT0),
                ),
            ]),
        ];
        let footer_paragraph = Paragraph::new(footer_lines).style(Style::default().fg(theme::TEXT));
        f.render_widget(footer_paragraph, footer_inner_area);
    } else {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
        draw_help_table(f, app_state, chunks[0]);
        let footer_block = Block::default().border_style(Style::default().fg(theme::SURFACE2));
        let footer_inner_area = footer_block.inner(chunks[1]);
        f.render_widget(footer_block, chunks[1]);
        let footer_lines = vec![
            Line::from(vec![
                Span::styled("Settings: ", Style::default().fg(theme::TEXT)),
                Span::styled(
                    truncate_with_ellipsis(
                        &settings_path_str,
                        footer_inner_area.width as usize - 10,
                    ),
                    Style::default().fg(theme::SUBTEXT0),
                ),
            ]),
            Line::from(vec![
                Span::styled("Log File: ", Style::default().fg(theme::TEXT)),
                Span::styled(
                    truncate_with_ellipsis(&log_path_str, footer_inner_area.width as usize - 10),
                    Style::default().fg(theme::SUBTEXT0),
                ),
            ]),
        ];
        let footer_paragraph = Paragraph::new(footer_lines).style(Style::default().fg(theme::TEXT));
        f.render_widget(footer_paragraph, footer_inner_area);
    }
}
fn draw_help_table(f: &mut Frame, app_state: &AppState, area: Rect) {
    let mode = &app_state.mode;

    let (lvl, progress) = calculate_player_stats(app_state);

    // Bar styling
    let gauge_width = 15;
    let filled_len = (progress * gauge_width as f64).round() as usize;
    let empty_len = gauge_width - filled_len;
    let gauge_str = format!("[{}{}]", "=".repeat(filled_len), "-".repeat(empty_len));

    // Text styling
    let level_text = format!("Level {} ({:.0}%)", lvl, progress * 100.0);

    let (title, rows) = match mode {
        AppMode::Normal | AppMode::Welcome => (
            " Manual / Help ",
            vec![
                Row::new(vec![Cell::from(Span::styled(
                    "General Controls",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("Ctrl +", Style::default().fg(theme::TEAL))),
                    Cell::from("Zoom in (increase font size)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("Ctrl -", Style::default().fg(theme::TEAL))),
                    Cell::from("Zoom out (decrease font size)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("q", Style::default().fg(theme::RED))),
                    Cell::from("Quit the application"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("m", Style::default().fg(theme::MAUVE))),
                    Cell::from("Toggle this help screen"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("c", Style::default().fg(theme::PEACH))),
                    Cell::from("Open Config screen"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("z", Style::default().fg(theme::SUBTEXT0))),
                    Cell::from("Toggle Zen/Power Saving mode"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                // --- List Navigation & Sorting ---
                Row::new(vec![Cell::from(Span::styled(
                    "List Navigation",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "↑ / ↓ / k / j",
                        Style::default().fg(theme::BLUE),
                    )),
                    Cell::from("Navigate torrents list"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "← / → / h / l",
                        Style::default().fg(theme::BLUE),
                    )),
                    Cell::from("Navigate between header columns"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("s", Style::default().fg(theme::GREEN))),
                    Cell::from("Change sort order for the selected column"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                // --- Torrent Management ---
                Row::new(vec![Cell::from(Span::styled(
                    "Torrent Actions",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("p", Style::default().fg(theme::GREEN))),
                    Cell::from("Pause / Resume selected torrent"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("d / D", Style::default().fg(theme::RED))),
                    Cell::from("Delete torrent (D includes downloaded files)"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled(
                    "Adding Torrents",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("a", Style::default().fg(theme::GREEN))),
                    Cell::from("Open file picker to add a .torrent file"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "Paste | v",
                        Style::default().fg(theme::SAPPHIRE),
                    )),
                    Cell::from("Paste a magnet link or local file path to add"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("CLI", Style::default().fg(theme::SAPPHIRE))),
                    Cell::from("Use `superseedr add ...` from another terminal"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                // --- Graph Controls ---
                Row::new(vec![Cell::from(Span::styled(
                    "Graph & Panes",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("t / T", Style::default().fg(theme::TEAL))),
                    Cell::from("Switch network graph time scale forward/backward"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("[ / ]", Style::default().fg(theme::TEAL))),
                    Cell::from("Change UI refresh rate (FPS)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("x", Style::default().fg(theme::TEAL))),
                    Cell::from("Anonymize torrent names"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                // --- Peer Flags Legend ---
                Row::new(vec![
                    // First Cell (for the first column)
                    Cell::from(Span::styled(
                        "Peer Flags Legend",
                        Style::default().fg(theme::YELLOW),
                    )),
                    // Second Cell (for the second column)
                    Cell::from(Line::from(vec![
                        // Legend pairing: DL/UL status
                        Span::raw("DL: (You "),
                        Span::styled("■", Style::default().fg(theme::SAPPHIRE)), // Toned-Down Interested
                        Span::styled("■", Style::default().fg(theme::MAROON)), // Toned-Down Choked
                        Span::raw(") | UL: (Peer "),
                        Span::styled("■", Style::default().fg(theme::TEAL)), // Toned-Down Interested
                        Span::styled("■", Style::default().fg(theme::PEACH)), // Toned-Down Choking
                        Span::raw(")"),
                    ])),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("■", Style::default().fg(theme::SAPPHIRE))),
                    Cell::from("You are interested (DL Potential)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("■", Style::default().fg(theme::MAROON))),
                    Cell::from("Peer is choking you (DL Block)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("■", Style::default().fg(theme::TEAL))),
                    Cell::from("Peer is interested (UL Opportunity)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("■", Style::default().fg(theme::PEACH))),
                    Cell::from("You are choking peer (UL Restriction)"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled(
                    "Disk Stats Legend",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("↑ (Read)", Style::default().fg(theme::GREEN))),
                    Cell::from("Data read from disk"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("↓ (Write)", Style::default().fg(theme::SKY))),
                    Cell::from("Data written to disk"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("Seek", Style::default().fg(theme::TEXT))),
                    Cell::from("Avg. distance between I/O ops (lower is better)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("Latency", Style::default().fg(theme::TEXT))),
                    Cell::from("Time to complete one I/O op (lower is better)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("IOPS", Style::default().fg(theme::TEXT))),
                    Cell::from("I/O Operations Per Second (total workload)"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled(
                    "Self-Tuning Legend",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("Best Score", Style::default().fg(theme::TEXT))),
                    Cell::from(
                        "Score measuring if randomized changes resulted in optimial speeds.",
                    ),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "Next seconds",
                        Style::default().fg(theme::TEXT),
                    )),
                    Cell::from("Countdown to try a new random resource adjustment (file handles)"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("(+/-)", Style::default().fg(theme::TEXT))),
                    Cell::from("Random setting change between resources. (Green=Good, Red=Bad)"),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                Row::new(vec![Cell::from(Span::styled(
                    "Build Features",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("DHT", Style::default().fg(theme::TEXT))),
                    Cell::from(Line::from(vec![
                        #[cfg(feature = "dht")]
                        Span::styled("ON", Style::default().fg(theme::GREEN)),
                        #[cfg(not(feature = "dht"))]
                        Span::styled(
                            "Not included in this [PRIVATE] build of superseedr.",
                            Style::default().fg(theme::RED),
                        ),
                    ])),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("Pex", Style::default().fg(theme::TEXT))),
                    Cell::from(Line::from(vec![
                        #[cfg(feature = "pex")]
                        Span::styled("ON", Style::default().fg(theme::GREEN)),
                        #[cfg(not(feature = "pex"))]
                        Span::styled(
                            "Not included in this [PRIVATE] build of superseedr.",
                            Style::default().fg(theme::RED),
                        ),
                    ])),
                ]),
                Row::new(vec![Cell::from(""), Cell::from("")]).height(1),
                // --- NEW: Session Stats at the Bottom ---
                Row::new(vec![Cell::from(Span::styled(
                    "Session Stats",
                    Style::default().fg(theme::YELLOW),
                ))]),
                Row::new(vec![
                    Cell::from(Span::styled("Level Up:", Style::default().fg(theme::MAUVE))),
                    Cell::from("Upload data or keep a large library seeding."),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(gauge_str, Style::default().fg(theme::GREEN))),
                    Cell::from(Span::styled(
                        level_text,
                        Style::default().fg(theme::YELLOW).bold(),
                    )),
                ]),
            ],
        ),
        AppMode::Config { .. } => (
            " Help / Config ",
            vec![
                Row::new(vec![
                    Cell::from(Span::styled("Esc / q", Style::default().fg(theme::GREEN))),
                    Cell::from("Save and exit config"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "↑ / ↓ / k / j",
                        Style::default().fg(theme::BLUE),
                    )),
                    Cell::from("Navigate items"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(
                        "← / → / h / l",
                        Style::default().fg(theme::BLUE),
                    )),
                    Cell::from("Decrease / Increase value"),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled("Enter", Style::default().fg(theme::YELLOW))),
                    Cell::from("Start or confirm editing"),
                ]),
            ],
        ),
        AppMode::FileBrowser { .. } => (
            " Help / File Browser ",
            vec![
                Row::new(vec![
                    Cell::from(Span::styled("Esc", Style::default().fg(theme::RED))),
                    Cell::from("Cancel selection"),
                ]),
                // ... rest of help items ...
            ],
        ),
        _ => (
            " Help ",
            vec![Row::new(vec![Cell::from(
                "No help available for this view.",
            )])],
        ),
    };

    let help_table = Table::new(rows, [Constraint::Length(20), Constraint::Min(30)]).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE2))
            .padding(Padding::new(2, 2, 1, 1)),
    );

    f.render_widget(Clear, area);
    f.render_widget(help_table, area);
}

fn draw_config_screen(
    f: &mut Frame,
    settings: &Settings,
    selected_index: usize,
    items: &[ConfigItem],
    editing: &Option<(ConfigItem, String)>,
) {
    let area = centered_rect(80, 60, f.area());
    f.render_widget(Clear, f.area());
    let block = Block::default()
        .title(Span::styled("Config", Style::default().fg(theme::MAUVE)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)])
        .split(inner_area);
    let settings_area = chunks[0];
    let footer_area = chunks[1];
    let rows_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            items
                .iter()
                .map(|_| Constraint::Length(1))
                .collect::<Vec<_>>(),
        )
        .split(settings_area);

    for (i, item) in items.iter().enumerate() {
        let (name_str, value_str) = match item {
            ConfigItem::ClientPort => ("Listen Port", settings.client_port.to_string()),
            ConfigItem::DefaultDownloadFolder => (
                "Default Download Folder",
                path_to_string(settings.default_download_folder.as_deref()),
            ),
            ConfigItem::WatchFolder => (
                "Torrent Watch Folder",
                path_to_string(settings.watch_folder.as_deref()),
            ),
            ConfigItem::GlobalDownloadLimit => (
                "Global DL Limit",
                format_limit_bps(settings.global_download_limit_bps),
            ),
            ConfigItem::GlobalUploadLimit => (
                "Global UL Limit",
                format_limit_bps(settings.global_upload_limit_bps),
            ),
        };

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(rows_layout[i]);
        let is_highlighted = if let Some((edited_item, _)) = editing {
            *edited_item == *item
        } else {
            i == selected_index
        };
        let row_style = if is_highlighted {
            Style::default().fg(theme::YELLOW)
        } else {
            Style::default().fg(theme::TEXT)
        };
        let name_with_selector = if is_highlighted {
            format!("▶ {}", name_str)
        } else {
            format!("  {}", name_str)
        };

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
        Line::from(vec![
            Span::styled("[Enter]", Style::default().fg(theme::GREEN)),
            Span::raw(" to confirm, "),
            Span::styled("[Esc]", Style::default().fg(theme::RED)),
            Span::raw(" to cancel."),
        ])
    } else {
        Line::from(vec![
            Span::raw("Use "),
            Span::styled("↑/↓/k/j", Style::default().fg(theme::YELLOW)),
            Span::raw(" to navigate. "),
            Span::styled("[Enter]", Style::default().fg(theme::YELLOW)),
            Span::raw(" to edit. "),
            Span::styled("[r]", Style::default().fg(theme::YELLOW)),
            Span::raw("eset to default. "),
            Span::styled("[Esc]|[q]", Style::default().fg(theme::GREEN)),
            Span::raw(" to Save & Exit, "),
        ])
    };

    let footer_paragraph = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme::SUBTEXT1));
    f.render_widget(footer_paragraph, footer_area);
}

fn draw_swarm_heatmap(f: &mut Frame, peers: &[PeerInfo], total_pieces: u32, area: Rect) {
    let color_status_low = Style::default().fg(theme::RED).add_modifier(Modifier::DIM);
    let color_status_medium = Style::default()
        .fg(theme::YELLOW)
        .add_modifier(Modifier::DIM);
    let color_status_high = Style::default().fg(theme::BLUE).add_modifier(Modifier::DIM);
    let color_status_complete = Style::default()
        .fg(theme::LAVENDER)
        .add_modifier(Modifier::BOLD);
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
            Style::default().fg(theme::LAVENDER),
        ),
        Span::styled(status_text, status_style),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::NONE)
        .padding(Padding::new(1, 1, 0, 1))
        .border_style(Style::default().fg(theme::SURFACE2));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    if total_pieces_usize == 0 {
        let center_text = Paragraph::new("Waiting for metadata...")
            .style(Style::default().fg(theme::SUBTEXT1))
            .alignment(Alignment::Center);
        let vertical_chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner_area);
        f.render_widget(center_text, vertical_chunks[1]);
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
                Style::default().fg(color),
            ));
        }
        lines.push(Line::from(spans));
    }
    let heatmap = Paragraph::new(lines);
    f.render_widget(heatmap, inner_area);
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

fn calculate_player_stats(app_state: &AppState) -> (u32, f64) {
    const XP_FOR_LEVEL_1: f64 = 5_000_000.0;

    const LEVEL_EXPONENT: f64 = 2.6;

    let total_seeding_size_bytes: u64 = app_state
        .torrents
        .values()
        .map(|t| t.latest_state.total_size)
        .sum();

    let total_gb = (total_seeding_size_bytes as f64) / 1_073_741_824.0;

    // - 100 GB Library -> ~500 XP/sec (~1.8 MB/hr)
    // - 1 TB Library   -> ~1500 XP/sec (~5.4 MB/hr)
    let passive_rate_per_sec = (total_gb + 1.0).powf(0.5) * 50.0;

    // Calculate total passive XP generated over the session runtime.
    let passive_xp = passive_rate_per_sec * (app_state.run_time as f64);

    // 1 Byte = 1 XP.
    let active_xp = app_state.session_total_uploaded as f64;

    let total_xp = active_xp + passive_xp;

    // Curve: Level = (XP / Base) ^ (1 / Exponent)
    // Inverse of: XP = Base * Level ^ Exponent
    //
    // L1   = 5 MB
    // L10  = 5 MB * 10^2.6 ~= 2 GB
    // L50  = 5 MB * 50^2.6 ~= 130 GB
    // L100 = 5 MB * 100^2.6 ~= 800 GB
    let raw_level = (total_xp / XP_FOR_LEVEL_1).powf(1.0 / LEVEL_EXPONENT);
    let current_level = raw_level.floor() as u32;

    // --- 5. PROGRESS BAR ---
    let xp_current_level_start = XP_FOR_LEVEL_1 * (current_level as f64).powf(LEVEL_EXPONENT);
    let xp_next_level_start = XP_FOR_LEVEL_1 * ((current_level + 1) as f64).powf(LEVEL_EXPONENT);

    let range = xp_next_level_start - xp_current_level_start;
    let progress_into_level = total_xp - xp_current_level_start;

    let ratio = if range <= 0.001 {
        0.0
    } else {
        progress_into_level / range
    };

    (current_level, ratio.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::layout::MIN_SIDEBAR_WIDTH;
    use ratatui::layout::Rect;

    /// Helper to create a LayoutContext manually since we don't want to mock AppState.
    /// Accessing the struct fields directly allows us to bypass `LayoutContext::new`.
    fn create_ctx(width: u16, height: u16) -> LayoutContext {
        LayoutContext {
            width,
            height,
            settings_sidebar_percent: 35, // Default sidebar percentage
        }
    }

    #[test]
    fn test_too_small_window_width() {
        let width = 39;
        let height = 50;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        assert!(plan.warning_message.is_some(), "Should warn if width < 40");
        assert_eq!(plan.warning_message.unwrap(), "Window too small");
    }

    #[test]
    fn test_too_small_window_height() {
        let width = 100;
        let height = 9;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        assert!(plan.warning_message.is_some(), "Should warn if height < 10");
        assert_eq!(plan.warning_message.unwrap(), "Window too small");
    }

    #[test]
    fn test_short_window_layout() {
        // Condition: height < 30 (but not too small)
        let width = 100;
        let height = 25;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        // Short layout specific checks
        assert!(
            plan.sparklines.is_some(),
            "Short layout should show sparklines"
        );
        assert!(plan.stats.is_some(), "Short layout should show stats");
        assert!(plan.chart.is_none(), "Short layout hides the large chart");

        // Ensure footer is at the bottom
        assert_eq!(plan.footer.height, 1);
        assert_eq!(plan.footer.y, height - 1);
    }

    #[test]
    fn test_narrow_vertical_layout() {
        // Condition: width < 100 (triggers "is_narrow")
        let width = 90;
        let height = 60;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        // Vertical/Narrow layout expectations
        assert!(plan.chart.is_some(), "Narrow layout should show chart");
        // In narrow mode (< 90 width), block stream is generally hidden or rearranged
        // The code for width < 90 splits info_cols into just details and block_stream(as vertical stack?)
        // Let's check logic: if ctx.width < 90: left_v split details/block_stream
        assert!(
            plan.block_stream.is_some(),
            "Narrow layout (w<90) preserves block stream in vertical stack"
        );
        assert!(
            plan.peer_stream.is_none(),
            "Height < 70 in vertical mode hides peer_stream"
        );
    }

    #[test]
    fn test_tall_vertical_layout() {
        // Condition: height > width * 0.6 AND height >= 70
        let width = 100;
        let height = 80; // 80 > 60
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        assert!(
            plan.peer_stream.is_some(),
            "Tall vertical layout (>70h) should show peer stream"
        );
        assert!(plan.chart.is_some());
    }

    #[test]
    fn test_standard_wide_layout_no_block_stream() {
        // Condition: Not short, not narrow (<100), not vertical aspect.
        // Width 120, Height 40.
        // Aspect: 40 vs 120*0.6=72. 40 < 72.
        // Wait, logic is: is_vertical_aspect = height > width * 0.6
        // If H=40, W=120, is_vertical=False.
        // is_narrow=False (120 > 100).
        // is_short=False (40 > 30).
        // -> Hits the "Standard/Wide" else block.

        let width = 120;
        let height = 40;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        assert!(
            plan.list.width >= MIN_SIDEBAR_WIDTH,
            "Sidebar should respect min width"
        );
        assert!(plan.sparklines.is_some());
        assert!(plan.peer_stream.is_some());

        // Width 120 is < 135, so block_stream should be hidden in standard view
        assert!(
            plan.block_stream.is_none(),
            "Standard width < 135 should hide block stream"
        );
    }

    #[test]
    fn test_ultra_wide_layout_with_block_stream() {
        // Condition: Width > 135
        let width = 150;
        let height = 60;
        let area = Rect::new(0, 0, width, height);
        let ctx = create_ctx(width, height);

        let plan = calculate_layout(area, &ctx);

        assert!(
            plan.block_stream.is_some(),
            "Wide width > 135 should show block stream"
        );

        // Ensure stats and block stream are splitting the bottom area
        if let Some(bs) = plan.block_stream {
            assert_eq!(
                bs.width, 14,
                "Block stream has fixed width of 14 in wide mode"
            );
        }
    }

    #[test]
    fn test_smart_table_layout_priorities() {
        // Test the smart column dropper logic
        let cols = vec![
            SmartCol {
                min_width: 10,
                priority: 0,
                constraint: Constraint::Length(10),
            }, // Must show
            SmartCol {
                min_width: 20,
                priority: 1,
                constraint: Constraint::Length(20),
            }, // High priority
            SmartCol {
                min_width: 50,
                priority: 2,
                constraint: Constraint::Length(50),
            }, // Low priority
        ];

        // 1. Very narrow: only priority 0 fits
        let (constraints, indices) = compute_smart_table_layout(&cols, 15, 0);
        assert_eq!(indices, vec![0], "Only priority 0 should fit in 15 width");
        assert_eq!(constraints.len(), 1);

        // 2. Medium: priority 0 + 1 fit (10 + 20 = 30 width needed)
        // With expansion_reserve logic: if width < 80, reserve is 15.
        // Available effective = 45 - 15 = 30.
        // Cost = 10 (p0) + 20 (p1) = 30. Fits exactly.
        let (_constraints, indices) = compute_smart_table_layout(&cols, 45, 0);
        assert!(indices.contains(&0));
        assert!(indices.contains(&1));
        assert!(!indices.contains(&2));

        // 3. Wide: all fit
        let (_constraints, indices) = compute_smart_table_layout(&cols, 200, 0);
        assert_eq!(indices.len(), 3, "All columns should fit in 200 width");
    }
}
