// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ratatui::{prelude::*, widgets::*};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::{AppMode, AppState};
use crate::config::Settings;
use crate::theme::ThemeContext;
use crate::tui::formatters::{centered_rect, format_limit_bps, format_speed};
use crate::tui::view::calculate_player_stats;
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};

pub fn handle_event(event: CrosstermEvent, app_state: &mut AppState) {
    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press {
            if let KeyCode::Char('z') = key.code {
                app_state.mode = AppMode::Normal;
            }
        }
    }
}

pub fn draw(f: &mut Frame, app_state: &AppState, settings: &Settings, ctx: &ThemeContext) {
    const LEVEL_GAUGE_WIDTH: usize = 16;
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
    let (level, level_progress) = calculate_player_stats(app_state);
    let level_filled_len = (level_progress * LEVEL_GAUGE_WIDTH as f64).round() as usize;
    let level_empty_len = LEVEL_GAUGE_WIDTH.saturating_sub(level_filled_len);
    let level_gauge = format!(
        "[{}{}] L{} ({:.0}%)",
        "=".repeat(level_filled_len),
        "-".repeat(level_empty_len),
        level,
        level_progress * 100.0
    );

    let area = centered_rect(40, 60, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(ctx.apply(Style::default().fg(ctx.theme.semantic.border)));
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
        Span::styled("DL: ", ctx.apply(Style::default().fg(ctx.accent_sky()))),
        Span::styled(
            format_speed(dl_speed),
            ctx.apply(Style::default().fg(ctx.accent_sky())),
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

    let mut ul_spans = vec![
        Span::styled("UL: ", ctx.apply(Style::default().fg(ctx.accent_teal()))),
        Span::styled(
            format_speed(ul_speed),
            ctx.apply(Style::default().fg(ctx.accent_teal())),
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
            Span::styled("super", ctx.apply(Style::default().fg(ctx.accent_sky()))),
            Span::styled("seedr", ctx.apply(Style::default().fg(ctx.accent_teal()))),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            current_message,
            ctx.apply(Style::default().fg(ctx.theme.semantic.subtext1)),
        )),
        Line::from(""),
        Line::from(dl_spans),
        Line::from(ul_spans),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Level: ",
                ctx.apply(Style::default().fg(ctx.state_selected())),
            ),
            Span::styled(
                level_gauge,
                ctx.apply(Style::default().fg(ctx.state_success())),
            ),
        ]),
    ];
    let main_paragraph = Paragraph::new(main_content_lines).alignment(Alignment::Center);
    let footer_line = Line::from(Span::styled(
        "Press [z] to resume",
        ctx.apply(Style::default().fg(ctx.theme.semantic.subtext0)),
    ));
    let footer_paragraph = Paragraph::new(footer_line).alignment(Alignment::Center);

    f.render_widget(main_paragraph, content_area);
    f.render_widget(footer_paragraph, footer_area);
}
