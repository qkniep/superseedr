// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::symbols::Marker;
use ratatui::{prelude::*, symbols, widgets::*};

use crate::tui::formatters::*;
use crate::tui::screens::{browser, config, delete_confirm, help, normal, power, welcome};

use crate::app::GraphDisplayMode;
use crate::app::PeerInfo;
use crate::app::{AppMode, AppState, SelectedHeader};
use crate::theme::ThemeContext;

use crate::tui::layout::get_peer_columns;
use crate::tui::layout::PeerColumnId;
use crate::tui::layout::{calculate_layout, compute_smart_table_layout, LayoutContext, SmartCol};

use crate::config::{Settings, SortDirection};
use throbber_widgets_tui::Throbber;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SECONDS_HISTORY_MAX: usize = 3600;
pub const MINUTES_HISTORY_MAX: usize = 48 * 60;

pub fn draw(f: &mut Frame, app_state: &mut AppState, settings: &Settings) {
    let area = f.area();

    let frame_wall_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let activity_speed_multiplier = compute_effects_activity_speed_multiplier(app_state, settings);
    if app_state.effects_last_wall_time <= 0.0 {
        app_state.effects_last_wall_time = frame_wall_time;
    }
    let frame_dt = (frame_wall_time - app_state.effects_last_wall_time).clamp(0.0, 0.25);
    app_state.effects_last_wall_time = frame_wall_time;
    app_state.effects_speed_multiplier = activity_speed_multiplier;
    app_state.effects_phase_time += frame_dt * activity_speed_multiplier;

    let ctx = ThemeContext::new(app_state.theme, app_state.effects_phase_time);

    if app_state.show_help {
        help::draw(f, app_state, &ctx);
        apply_theme_effects_to_frame(f, &ctx);
        return;
    }

    match &app_state.mode {
        AppMode::Welcome => {
            welcome::draw(f, settings, &ctx);
            apply_theme_effects_to_frame(f, &ctx);
            return;
        }
        AppMode::PowerSaving => {
            power::draw(f, app_state, settings, &ctx);
            apply_theme_effects_to_frame(f, &ctx);
            return;
        }
        AppMode::Config {
            settings_edit,
            selected_index,
            items,
            editing,
        } => {
            config::draw(f, settings_edit, *selected_index, items, editing, &ctx);
            apply_theme_effects_to_frame(f, &ctx);
            return;
        }
        AppMode::DeleteConfirm { .. } => {
            delete_confirm::draw(f, app_state, &ctx);
            apply_theme_effects_to_frame(f, &ctx);
            return;
        }
        AppMode::FileBrowser {
            state,
            data,
            browser_mode,
        } => {
            browser::draw(f, app_state, state, data, browser_mode, &ctx);
            apply_theme_effects_to_frame(f, &ctx);
            return;
        }
        _ => {}
    }

    let layout_ctx = LayoutContext::new(area, app_state, 35);
    let plan = calculate_layout(area, &layout_ctx);

    normal::draw_torrent_list(f, app_state, plan.list, &ctx);
    normal::draw_footer(f, app_state, settings, plan.footer, &ctx);
    normal::draw_details_panel(f, app_state, plan.details, &ctx);
    draw_peers_table(f, app_state, plan.peers, &ctx);

    if let Some(r) = plan.chart {
        draw_network_chart(f, app_state, r, &ctx);
    }
    if let Some(r) = plan.sparklines {
        draw_torrent_sparklines(f, app_state, r, &ctx);
    }
    if let Some(r) = plan.peer_stream {
        draw_peer_stream(f, app_state, r, &ctx);
    }
    if let Some(r) = plan.block_stream {
        draw_vertical_block_stream(f, app_state, r, &ctx);
    }
    if let Some(r) = plan.stats {
        draw_stats_panel(f, app_state, settings, r, &ctx);
    }

    if let Some(msg) = plan.warning_message {
        f.render_widget(
            Paragraph::new(msg).style(
                Style::default()
                    .fg(ctx.state_error())
                    .bg(ctx.theme.semantic.surface0),
            ),
            plan.list,
        );
    }
    if let Some(error_text) = &app_state.system_error {
        normal::draw_status_error_popup(f, error_text, &ctx);
    }
    if app_state.should_quit {
        normal::draw_shutdown_screen(f, app_state, &ctx);
    }

    apply_theme_effects_to_frame(f, &ctx);
}

fn compute_effects_activity_speed_multiplier(app_state: &AppState, settings: &Settings) -> f64 {
    let dl_bps = app_state.avg_download_history.last().copied().unwrap_or(0) as f64;
    let ul_bps = app_state.avg_upload_history.last().copied().unwrap_or(0) as f64;

    let dl_ref = if settings.global_download_limit_bps > 0 {
        settings.global_download_limit_bps as f64
    } else {
        4_000_000.0
    };
    let ul_ref = if settings.global_upload_limit_bps > 0 {
        settings.global_upload_limit_bps as f64
    } else {
        1_000_000.0
    };

    let dl_activity = (dl_bps / dl_ref).clamp(0.0, 1.0);
    let ul_activity = (ul_bps / ul_ref).clamp(0.0, 1.0);

    let activity_score = (dl_activity * 0.60) + (ul_activity * 0.40);

    // Keep effects at baseline when idle, then scale up smoothly with throughput.
    1.0 + (activity_score * 2.0)
}

fn apply_theme_effects_to_frame(f: &mut Frame, ctx: &ThemeContext) {
    if !ctx.theme.effects.enabled() {
        return;
    }

    let area = f.area();
    let buf = f.buffer_mut();

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.fg != Color::Reset {
                    cell.fg = ctx.apply_effects_to_color_at(cell.fg, x, y, area.width, area.height);
                }
            }
        }
    }
}

fn draw_network_chart(f: &mut Frame, app_state: &AppState, chart_chunk: Rect, ctx: &ThemeContext) {
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
        .style(
            ctx.apply(
                Style::default()
                    .fg(ctx.state_error())
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .data(&backoff_marker_data);

    let datasets = vec![
        Dataset::default()
            .name("Download")
            .marker(Marker::Braille)
            .style(
                ctx.apply(
                    Style::default()
                        .fg(ctx.state_info())
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .data(&dl_data),
        Dataset::default()
            .name("Upload")
            .marker(Marker::Braille)
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

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(chart_title)
                .borders(Borders::ALL)
                .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border))),
        )
        .x_axis(
            Axis::default()
                .style(ctx.apply(Style::default().fg(ctx.theme.semantic.overlay0)))
                .bounds([0.0, points_to_show.saturating_sub(1) as f64])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(ctx.apply(Style::default().fg(ctx.theme.semantic.overlay0)))
                .bounds([0.0, nice_max_speed as f64])
                .labels(y_speed_axis_labels),
        )
        .legend_position(Some(LegendPosition::TopRight));

    f.render_widget(chart, chart_chunk);
}

fn draw_stats_panel(
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

fn draw_peer_stream(f: &mut Frame, app_state: &AppState, area: Rect, ctx: &ThemeContext) {
    if area.height < 3 || area.width < 10 {
        return;
    }

    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
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
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::DIM),
            )
            .data(&conn_points_small),
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::DIM),
            )
            .data(&disc_points_small),
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::DIM),
            )
            .data(&disconn_points_small),
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_connected)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&conn_points_large),
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_discovered)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&disc_points_large),
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .style(
                Style::default()
                    .fg(color_disconnected)
                    .add_modifier(Modifier::BOLD),
            )
            .data(&disconn_points_large),
    ];

    let x_bound = disc_slice.len().max(1).saturating_sub(1) as f64;

    let chart = Chart::new(datasets)
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
        .x_axis(Axis::default().bounds([0.0, x_bound]))
        .y_axis(Axis::default().bounds([0.5, 3.5]));

    f.render_widget(chart, area);
}

fn draw_vertical_block_stream(f: &mut Frame, app_state: &AppState, area: Rect, ctx: &ThemeContext) {
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

fn draw_torrent_sparklines(f: &mut Frame, app_state: &AppState, area: Rect, ctx: &ThemeContext) {
    let torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
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

        let dl_sparkline = Sparkline::default()
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
        let ul_sparkline = Sparkline::default()
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

        let dl_sparkline = Sparkline::default()
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

        let ul_sparkline = Sparkline::default()
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

fn draw_peers_table(f: &mut Frame, app_state: &AppState, peers_chunk: Rect, ctx: &ThemeContext) {
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
                                app_state.selected_header == SelectedHeader::Peer(visual_idx);
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
        // [UPDATED] Render a fake greyed-out heatmap grid
        let available_width = inner_area.width as usize;
        let available_height = inner_area.height as usize;
        let mut lines = Vec::with_capacity(available_height);

        // Fill with light shades in a dim color to represent empty slots
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

pub(crate) fn calculate_player_stats(app_state: &AppState) -> (u32, f64) {
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

    #[test]
    fn test_truncate_theme_label_preserves_fx_suffix_when_truncated() {
        let out =
            crate::tui::screens::normal::truncate_theme_label_preserving_fx("Bioluminescent Reef", true, 13);
        assert_eq!(out, "Biolum...[FX]");
    }

    #[test]
    fn test_truncate_theme_label_shows_full_fx_label_when_space_allows() {
        let out =
            crate::tui::screens::normal::truncate_theme_label_preserving_fx("Bioluminescent Reef", true, 25);
        assert_eq!(out, "Bioluminescent Reef [FX]");
    }

    #[test]
    fn test_footer_left_width_expands_with_terminal_width() {
        let small = crate::tui::screens::normal::compute_footer_left_width(90, false);
        let large = crate::tui::screens::normal::compute_footer_left_width(180, false);
        assert!(
            large > small,
            "left footer width should expand on wider terminals"
        );
    }

    #[test]
    fn test_footer_left_width_respects_bounds() {
        assert_eq!(crate::tui::screens::normal::compute_footer_left_width(90, false), 51);
        assert_eq!(crate::tui::screens::normal::compute_footer_left_width(200, false), 90);
        assert_eq!(crate::tui::screens::normal::compute_footer_left_width(200, true), 110);
    }
}
