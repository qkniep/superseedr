// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::app::{App, AppMode, ConfigItem, SelectedHeader, TorrentControlState};
use crate::torrent_manager::ManagerCommand;

use strum::EnumCount;
use strum::IntoEnumIterator;

use crate::config::PeerSortColumn;
use crate::config::SortDirection;
use crate::config::TorrentSortColumn;

use crate::tui::layout::get_torrent_columns;
use crate::tui::layout::get_peer_columns;
use crate::tui::layout::compute_smart_table_layout;
use crate::tui::layout::calculate_layout;
use crate::tui::layout::LayoutContext;
use crate::tui::layout::SmartCol;

use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::style::{Color, Style};
use ratatui_explorer::{FileExplorer, Theme};
use ratatui::prelude::Rect;

use std::path::Path;
use tracing::{event as tracing_event, Level};

use directories::UserDirs;

#[cfg(windows)]
use clipboard::{ClipboardContext, ClipboardProvider};

pub async fn handle_event(event: CrosstermEvent, app: &mut App) {

    if let CrosstermEvent::Resize(w, h) = &event {
        app.app_state.screen_area = Rect::new(0, 0, *w, *h);
        app.app_state.ui_needs_redraw = true;
        return;
    }

    if let CrosstermEvent::Key(key) = event {
        if app.app_state.is_searching && key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Esc => {
                    // Exit search mode and clear the filter
                    app.app_state.is_searching = false;
                    app.app_state.search_query.clear();
                    app.sort_and_filter_torrent_list();
                    app.app_state.selected_torrent_index = 0;
                }
                KeyCode::Enter => {
                    // Exit search mode, but keep the filter
                    app.app_state.is_searching = false;
                }
                KeyCode::Backspace => {
                    app.app_state.search_query.pop();
                    app.sort_and_filter_torrent_list(); // Re-filter
                    app.app_state.selected_torrent_index = 0;
                }
                KeyCode::Char(c) => {
                    app.app_state.search_query.push(c);
                    app.sort_and_filter_torrent_list(); // Re-filter
                    app.app_state.selected_torrent_index = 0;
                }
                _ => {} // Ignore other keys like Up/Down while typing
            }
            app.app_state.ui_needs_redraw = true;
            return;
        }

        #[cfg(windows)]
        {
            let mut help_key_handled = false;
            // On Windows, we only get Press, so we just toggle
            if key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Press {
                app.app_state.show_help = !app.app_state.show_help;
                help_key_handled = true;
            }

            if help_key_handled {
                app.app_state.ui_needs_redraw = true;
                return;
            }

            // If help is shown, consume all other key presses
            if app.app_state.show_help {
                return;
            }
        }

        #[cfg(not(windows))]
        {
            let mut help_key_handled = false;
            if app.app_state.show_help {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Release)
                {
                    app.app_state.show_help = false;
                    help_key_handled = true;
                }
            } else if key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Press {
                app.app_state.show_help = true;
                help_key_handled = true;
            }

            if help_key_handled {
                app.app_state.ui_needs_redraw = true;
                return;
            }
        }
    }

    match &mut app.app_state.mode {
        AppMode::Welcome => {
            if let CrosstermEvent::Key(key) = event {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
                    app.app_state.mode = AppMode::Normal;
                }
            }
        }
        AppMode::Normal => match event {
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
                            app.app_state.anonymize_torrent_names =
                                !app.app_state.anonymize_torrent_names;
                        }
                        KeyCode::Char('z') => {
                            app.app_state.mode = AppMode::PowerSaving;
                            return;
                        }
                        KeyCode::Char('q') => {
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
                        KeyCode::Char('[') => {
                            app.app_state.data_rate = app.app_state.data_rate.next_slower();
                            let new_rate = app.app_state.data_rate.as_ms();

                            for manager_tx in app.torrent_manager_command_txs.values() {
                                let _ = manager_tx.try_send(ManagerCommand::SetDataRate(new_rate));
                            }
                        }
                        KeyCode::Char(']') => {
                            app.app_state.data_rate = app.app_state.data_rate.next_faster();
                            let new_rate = app.app_state.data_rate.as_ms();

                            for manager_tx in app.torrent_manager_command_txs.values() {
                                let _ = manager_tx.try_send(ManagerCommand::SetDataRate(new_rate));
                            }
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
                                    let torrent_manager_command_tx_clone =
                                        torrent_manager_command_tx.clone();
                                    tokio::spawn(async move {
                                        let _ =
                                            torrent_manager_command_tx_clone.send(command).await;
                                    });
                                }
                            }
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
                            match app.app_state.selected_header {
                                SelectedHeader::Torrent(i) => {
                                    let cols = get_torrent_columns();
                                    
                                    if let Some(def) = cols.get(i) {
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

                                    // 1. Get the column definition for index `i`
                                    if let Some(def) = cols.get(i) {
                                        // 2. Check if this column has a sort_enum assigned
                                        if let Some(column) = def.sort_enum {
                                            if app.app_state.peer_sort.0 == column {
                                                app.app_state.peer_sort.1 = if app.app_state.peer_sort.1
                                                    == SortDirection::Ascending
                                                {
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
                                    app.app_state.system_error =
                                        Some(format!("Clipboard read error: {}", e));
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
        },
        AppMode::PowerSaving => {
            if let CrosstermEvent::Key(key) = event {
                if key.kind == KeyEventKind::Press {
                    if let KeyCode::Char('z') = key.code {
                        app.app_state.mode = AppMode::Normal;
                    }
                }
            }
        }
        AppMode::Config {
            settings_edit,
            selected_index,
            items,
            editing,
        } => {
            if let Some((item, buffer)) = editing {
                if let CrosstermEvent::Key(key) = event {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char(c) => {
                                if c.is_ascii_digit() {
                                    buffer.push(c);
                                }
                            }
                            KeyCode::Backspace => {
                                buffer.pop();
                            }
                            KeyCode::Esc => *editing = None,
                            KeyCode::Enter => {
                                match item {
                                    ConfigItem::ClientPort => {
                                        if let Ok(new_port) = buffer.parse::<u16>() {
                                            if new_port > 0 {
                                                settings_edit.client_port = new_port;
                                            }
                                        }
                                    }
                                    ConfigItem::GlobalDownloadLimit => {
                                        if let Ok(new_rate) = buffer.parse::<u64>() {
                                            settings_edit.global_download_limit_bps = new_rate;
                                            let bucket = app.global_dl_bucket.clone();
                                            tokio::spawn(async move {
                                                bucket.set_rate(new_rate as f64);
                                            });
                                        }
                                    }
                                    ConfigItem::GlobalUploadLimit => {
                                        if let Ok(new_rate) = buffer.parse::<u64>() {
                                            settings_edit.global_upload_limit_bps = new_rate;
                                            let bucket = app.global_ul_bucket.clone();
                                            tokio::spawn(async move {
                                                bucket.set_rate(new_rate as f64);
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                                *editing = None;
                            }
                            _ => {}
                        }
                    }
                }
            } else if let CrosstermEvent::Key(key) = event {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.client_configs = *settings_edit.clone();
                            app.app_state.mode = AppMode::Normal;
                        }
                        KeyCode::Enter => {
                            let selected_item = items[*selected_index];
                            match selected_item {
                                ConfigItem::GlobalDownloadLimit
                                | ConfigItem::GlobalUploadLimit
                                | ConfigItem::ClientPort => {
                                    *editing = Some((selected_item, String::new()));
                                }
                                ConfigItem::DefaultDownloadFolder | ConfigItem::WatchFolder => {
                                    let theme = Theme::default().add_default_title();
                                    match FileExplorer::with_theme(theme) {
                                        Ok(file_explorer) => {
                                            app.app_state.mode = AppMode::ConfigPathPicker {
                                                settings_edit: settings_edit.clone(),
                                                for_item: selected_item,
                                                file_explorer,
                                            };
                                        }
                                        Err(e) => tracing_event!(
                                            Level::ERROR,
                                            "Failed to create FileExplorer for config: {}",
                                            e
                                        ),
                                    }
                                }
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            *selected_index = selected_index.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if *selected_index < items.len() - 1 {
                                *selected_index += 1;
                            }
                        }
                        KeyCode::Char('r') => {
                            let default_settings = crate::config::Settings::default();
                            let selected_item = items[*selected_index];
                            match selected_item {
                                ConfigItem::ClientPort => {
                                    settings_edit.client_port = default_settings.client_port;
                                }
                                ConfigItem::DefaultDownloadFolder => {
                                    settings_edit.default_download_folder =
                                        default_settings.default_download_folder;
                                }
                                ConfigItem::WatchFolder => {
                                    settings_edit.watch_folder = default_settings.watch_folder;
                                }
                                ConfigItem::GlobalDownloadLimit => {
                                    settings_edit.global_download_limit_bps =
                                        default_settings.global_download_limit_bps;
                                }
                                ConfigItem::GlobalUploadLimit => {
                                    settings_edit.global_upload_limit_bps =
                                        default_settings.global_upload_limit_bps;
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            let item = items[*selected_index];
                            let increment = 10_000 * 8;
                            match item {
                                ConfigItem::GlobalDownloadLimit => {
                                    let new_rate = settings_edit
                                        .global_download_limit_bps
                                        .saturating_add(increment);
                                    settings_edit.global_download_limit_bps = new_rate;
                                    let bucket = app.global_dl_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                                ConfigItem::GlobalUploadLimit => {
                                    let new_rate = settings_edit
                                        .global_upload_limit_bps
                                        .saturating_add(increment);
                                    settings_edit.global_upload_limit_bps = new_rate;
                                    let bucket = app.global_ul_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            let item = items[*selected_index];
                            let decrement = 10_000 * 8;
                            match item {
                                ConfigItem::ClientPort => {}
                                ConfigItem::GlobalDownloadLimit => {
                                    let new_rate = settings_edit
                                        .global_download_limit_bps
                                        .saturating_sub(decrement);
                                    settings_edit.global_download_limit_bps = new_rate;
                                    let bucket = app.global_dl_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                                ConfigItem::GlobalUploadLimit => {
                                    let new_rate = settings_edit
                                        .global_upload_limit_bps
                                        .saturating_sub(decrement);
                                    settings_edit.global_upload_limit_bps = new_rate;
                                    let bucket = app.global_ul_bucket.clone();
                                    tokio::spawn(async move {
                                        bucket.set_rate(new_rate as f64);
                                    });
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        AppMode::ConfigPathPicker {
            settings_edit,
            for_item,
            file_explorer,
        } => {
            if let CrosstermEvent::Key(key) = event {
                let config_items = ConfigItem::iter().collect::<Vec<_>>();
                let return_to_config = |settings_edit, for_item| -> AppMode {
                    let selected_index = config_items
                        .iter()
                        .position(|&item| item == for_item)
                        .unwrap_or(0);
                    AppMode::Config {
                        settings_edit,
                        selected_index,
                        items: config_items,
                        editing: None,
                    }
                };
                match key.code {
                    KeyCode::Tab => {
                        let path = file_explorer.current().path().clone();
                        let dir_path = if path.is_dir() {
                            path
                        } else {
                            path.parent().unwrap_or(&path).to_path_buf()
                        };
                        match for_item {
                            ConfigItem::DefaultDownloadFolder => {
                                settings_edit.default_download_folder = Some(dir_path)
                            }
                            ConfigItem::WatchFolder => settings_edit.watch_folder = Some(dir_path),
                            _ => {}
                        }
                        app.app_state.mode = return_to_config(settings_edit.clone(), *for_item);
                    }
                    KeyCode::Esc => {
                        app.app_state.mode = return_to_config(settings_edit.clone(), *for_item)
                    }
                    _ => if file_explorer.handle(&event).is_err() {},
                }
            }
        }
        AppMode::DownloadPathPicker(file_explorer) => {
            if let CrosstermEvent::Key(key) = event {
                match key.code {
                    KeyCode::Tab => {
                        let mut download_path = file_explorer.current().path().clone();
                        if !download_path.is_dir() {
                            if let Some(parent) = download_path.parent() {
                                download_path = parent.to_path_buf();
                            }
                        }

                        if let Some(pending_path) = app.app_state.pending_torrent_path.take() {
                            if pending_path.extension().is_some_and(|e| e == "torrent") {
                                app.add_torrent_from_file(
                                    pending_path,
                                    download_path,
                                    false,
                                    TorrentControlState::Running,
                                )
                                .await;
                            } else {
                                // This handles the case where the path was from a .path or .magnet file
                                if let Ok(content) = std::fs::read_to_string(&pending_path) {
                                    if content.starts_with("magnet:") {
                                        app.add_magnet_torrent(
                                            "Fetching name...".to_string(),
                                            content.trim().to_string(),
                                            download_path,
                                            false,
                                            TorrentControlState::Running,
                                        )
                                        .await;
                                    } else {
                                        app.add_torrent_from_file(
                                            std::path::PathBuf::from(content.trim()),
                                            download_path,
                                            false,
                                            TorrentControlState::Running,
                                        )
                                        .await;
                                    }
                                }
                            }
                        } else if !app.app_state.pending_torrent_link.is_empty() {
                            app.add_magnet_torrent(
                                "Fetching name...".to_string(),
                                app.app_state.pending_torrent_link.clone(),
                                download_path,
                                false,
                                TorrentControlState::Running,
                            )
                            .await;
                            app.app_state.pending_torrent_link.clear();
                        }

                        app.app_state.mode = AppMode::Normal;
                        app.app_state.system_error = None;
                    }
                    KeyCode::Esc => {
                        app.app_state.mode = AppMode::Normal;
                        app.app_state.system_error = None;
                        app.app_state.pending_torrent_path = None;
                        app.app_state.pending_torrent_link.clear();
                    }
                    _ => {
                        if let Err(e) = file_explorer.handle(&event) {
                            tracing_event!(Level::ERROR, "File explorer error: {}", e);
                        }
                    }
                }
            }
        }
        AppMode::DeleteConfirm {
            info_hash,
            with_files,
        } => {
            if let CrosstermEvent::Key(key) = event {
                match key.code {
                    KeyCode::Enter => {
                        let command = if *with_files {
                            crate::torrent_manager::ManagerCommand::DeleteFile
                        } else {
                            crate::torrent_manager::ManagerCommand::Shutdown
                        };
                        if let Some(manager_tx) = app.torrent_manager_command_txs.get(info_hash) {
                            let manager_tx_clone = manager_tx.clone();
                            tokio::spawn(async move {
                                let _ = manager_tx_clone.send(command).await;
                            });
                        }
                        if let Some(torrent) = app.app_state.torrents.get_mut(info_hash) {
                            torrent.latest_state.torrent_control_state =
                                TorrentControlState::Deleting;
                        }
                        app.app_state.mode = AppMode::Normal;
                    }
                    KeyCode::Esc => app.app_state.mode = AppMode::Normal,
                    _ => {}
                }
            }
        }
    }
    app.app_state.ui_needs_redraw = true;
}

fn handle_navigation(app_state: &mut AppState, key_code: KeyCode) {
    // --- Get state needed for navigation ---
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let selected_torrent_has_peers =
        selected_torrent.is_some_and(|torrent| !torrent.latest_state.peers.is_empty());

    let selected_torrent_peer_count =
        selected_torrent.map_or(0, |torrent| torrent.latest_state.peers.len());

    // --- DYNAMICALLY CALCULATE VISIBLE COLUMNS ---
    // 1. Calculate layout to get list widths
    let ctx = LayoutContext::new(app_state.screen_area, app_state, 35);
    let layout_plan = calculate_layout(app_state.screen_area, &ctx);

    // 2. Compute visible Torrent Columns
    let t_cols = get_torrent_columns();
    let smart_t_cols: Vec<SmartCol> = t_cols.iter().map(|c| SmartCol {
        header: c.header,
        min_width: c.min_width,
        priority: c.priority,
        constraint: c.default_constraint,
    }).collect();
    let (_, visible_t_indices) = compute_smart_table_layout(&smart_t_cols, layout_plan.list.width, 1);
    let torrent_col_count = visible_t_indices.len();

    // 3. Compute visible Peer Columns
    let p_cols = get_peer_columns();
    let smart_p_cols: Vec<SmartCol> = p_cols.iter().map(|c| SmartCol {
        header: c.header,
        min_width: c.min_width,
        priority: c.priority,
        constraint: c.default_constraint,
    }).collect();
    // Use peers width if available, otherwise assume 0 (which results in 0 columns)
    let (_, visible_p_indices) = compute_smart_table_layout(&smart_p_cols, layout_plan.peers.width, 1);
    let peer_col_count = visible_p_indices.len();

    match key_code {
        // --- UP/DOWN/J/K Navigation (Rows) ---
        KeyCode::Up | KeyCode::Char('k') => {
            match app_state.selected_header {
                SelectedHeader::Torrent(_) => {
                    app_state.selected_torrent_index =
                        app_state.selected_torrent_index.saturating_sub(1);
                    app_state.selected_peer_index = 0;
                }
                SelectedHeader::Peer(_) => {
                    app_state.selected_peer_index = app_state.selected_peer_index.saturating_sub(1);
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match app_state.selected_header {
                SelectedHeader::Torrent(_) => {
                    if !app_state.torrent_list_order.is_empty() {
                        let new_index = app_state.selected_torrent_index.saturating_add(1);
                        if new_index < app_state.torrent_list_order.len() {
                            app_state.selected_torrent_index = new_index;
                        }
                    }
                    app_state.selected_peer_index = 0;
                }
                SelectedHeader::Peer(_) => {
                    if selected_torrent_peer_count > 0 {
                        let new_index = app_state.selected_peer_index.saturating_add(1);
                        if new_index < selected_torrent_peer_count {
                            app_state.selected_peer_index = new_index;
                        }
                    }
                }
            }
        }

        // --- LEFT/RIGHT/H/L Navigation (Columns) ---
        KeyCode::Left | KeyCode::Char('h') => {
            app_state.selected_header = match app_state.selected_header {
                SelectedHeader::Torrent(0) => {
                    // Wrap around to the last visible Peer column
                    if selected_torrent_has_peers && peer_col_count > 0 {
                        SelectedHeader::Peer(peer_col_count - 1)
                    } else {
                        SelectedHeader::Torrent(0)
                    }
                }
                SelectedHeader::Torrent(i) => SelectedHeader::Torrent(i - 1),
                
                SelectedHeader::Peer(0) => {
                    // Jump back to the last visible Torrent column
                    SelectedHeader::Torrent(torrent_col_count.saturating_sub(1))
                },
                
                SelectedHeader::Peer(i) => SelectedHeader::Peer(i - 1),
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app_state.selected_header = match app_state.selected_header {
                SelectedHeader::Torrent(i) => {
                    // If not at the last visible column, move right
                    if i < torrent_col_count.saturating_sub(1) {
                         SelectedHeader::Torrent(i + 1)
                    } else {
                        // At the last visible column, jump to Peer column 0 (if valid)
                        if selected_torrent_has_peers && peer_col_count > 0 {
                            SelectedHeader::Peer(0)
                        } else {
                            SelectedHeader::Torrent(i)
                        }
                    }
                }
                SelectedHeader::Peer(i) => {
                    // If not at the last visible peer column, move right
                    if i < peer_col_count.saturating_sub(1) {
                        SelectedHeader::Peer(i + 1)
                    } else {
                        // Wrap around to Torrent column 0
                        SelectedHeader::Torrent(0)
                    }
                }
            };
        }
        _ => {} 
    }
}

async fn handle_pasted_text(app: &mut App, pasted_text: &str) {
    if pasted_text.starts_with("magnet:") {
        // If a default download folder is configured, use it directly.
        if let Some(download_path) = app.client_configs.default_download_folder.clone() {
            app.add_magnet_torrent(
                "Fetching name...".to_string(),
                pasted_text.to_string(),
                download_path,
                false,
                TorrentControlState::Running,
            )
            .await;
        } else {
            app.app_state.pending_torrent_link = pasted_text.to_string();
            let theme = Theme::default()
                .add_default_title()
                .with_item_style(Style::default().fg(Color::DarkGray))
                .with_dir_style(Style::default());
            match FileExplorer::with_theme(theme) {
                Ok(mut file_explorer) => {
                    // Since no default path is set, try to find the most common path to start the picker in
                    let initial_path = app
                        .find_most_common_download_path()
                        .or_else(|| UserDirs::new().map(|ud| ud.home_dir().to_path_buf()));
                    if let Some(common_path) = initial_path {
                        file_explorer.set_cwd(common_path).ok();
                    }
                    app.app_state.mode = AppMode::DownloadPathPicker(file_explorer);
                }
                Err(e) => {
                    tracing_event!(Level::ERROR, "Failed to create FileExplorer: {}", e);
                }
            }
        }
    } else {
        let path = Path::new(pasted_text.trim());
        if path.is_file() && path.extension().is_some_and(|ext| ext == "torrent") {
            if let Some(download_path) = app.client_configs.default_download_folder.clone() {
                app.add_torrent_from_file(
                    path.to_path_buf(),
                    download_path,
                    false,
                    TorrentControlState::Running,
                )
                .await;
            } else {
                // Show the download path picker.
                app.app_state.pending_torrent_path = Some(path.to_path_buf());
                match FileExplorer::new() {
                    Ok(mut file_explorer) => {
                        let initial_path = app
                            .find_most_common_download_path()
                            .or_else(|| UserDirs::new().map(|ud| ud.home_dir().to_path_buf()));
                        if let Some(common_path) = initial_path {
                            file_explorer.set_cwd(common_path).ok();
                        }
                        app.app_state.mode = AppMode::DownloadPathPicker(file_explorer);
                    }
                    Err(e) => {
                        tracing_event!(Level::ERROR, "Failed to create FileExplorer: {}", e);
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, PeerInfo, SelectedHeader, TorrentDisplayState, TorrentMetrics};
    use crate::config::TorrentSortColumn;
    use ratatui::crossterm::event::KeyCode;

    /// Creates a mock TorrentMetrics with a specific number of peers.
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

    /// Creates a mock TorrentDisplayState for testing.
    fn create_mock_display_state(peer_count: usize) -> TorrentDisplayState {
        TorrentDisplayState {
            latest_state: create_mock_metrics(peer_count),
            ..Default::default()
        }
    }

    /// Creates a mock AppState for testing navigation.
    fn create_test_app_state() -> AppState {
        let mut app_state = AppState::default();

        let torrent_a = create_mock_display_state(2); // Has 2 peers
        let torrent_b = create_mock_display_state(0); // Has 0 peers

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

    // --- NAVIGATION TESTS ---

    #[test]
    fn test_nav_down_torrents() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0;
        app_state.selected_header = SelectedHeader::Torrent(0);

        handle_navigation(&mut app_state, KeyCode::Down);

        assert_eq!(app_state.selected_torrent_index, 1);
        assert_eq!(app_state.selected_peer_index, 0); // Should reset
    }

    #[test]
    fn test_nav_up_torrents() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 1;
        app_state.selected_header = SelectedHeader::Torrent(0);

        handle_navigation(&mut app_state, KeyCode::Up);

        assert_eq!(app_state.selected_torrent_index, 0);
        assert_eq!(app_state.selected_peer_index, 0); // Should reset
    }

    #[test]
    fn test_nav_down_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.selected_peer_index = 0;
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Down);

        assert_eq!(app_state.selected_torrent_index, 0); // Stays on same torrent
        assert_eq!(app_state.selected_peer_index, 1); // Moves down peer list
    }

    #[test]
    fn test_nav_right_to_peers_when_peers_exist() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // "hash_a" has peers
        app_state.selected_header = SelectedHeader::Torrent(TorrentSortColumn::COUNT - 1);

        handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.selected_header, SelectedHeader::Peer(0));
    }

    #[test]
    fn test_nav_right_to_peers_when_no_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 1; // "hash_b" has 0 peers
        app_state.selected_header = SelectedHeader::Torrent(TorrentSortColumn::COUNT - 1);

        handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(
            app_state.selected_header,
            SelectedHeader::Torrent(TorrentSortColumn::COUNT - 1)
        );
    }

    #[test]
    fn test_nav_left_from_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0;
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Left);

        assert_eq!(
            app_state.selected_header,
            SelectedHeader::Torrent(TorrentSortColumn::COUNT - 1)
        );
    }

    #[test]
    fn test_nav_up_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.selected_peer_index = 1;
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Up);

        assert_eq!(app_state.selected_torrent_index, 0); // Stays on same torrent
        assert_eq!(app_state.selected_peer_index, 0); // Moves up peer list
    }

    #[test]
    fn test_nav_up_at_top_of_list() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // At the top
        app_state.selected_header = SelectedHeader::Torrent(0);

        handle_navigation(&mut app_state, KeyCode::Up);

        // Should stay at 0, thanks to saturating_sub
        assert_eq!(app_state.selected_torrent_index, 0);
    }

    #[test]
    fn test_nav_down_at_bottom_of_list() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 1; // At the bottom (index 1 of 2)
        app_state.selected_header = SelectedHeader::Torrent(0);

        handle_navigation(&mut app_state, KeyCode::Down);

        // Should stay at 1, as it's the last index
        assert_eq!(app_state.selected_torrent_index, 1);
    }

    #[test]
    fn test_nav_up_peers_at_top_of_list() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.selected_peer_index = 0; // At the top
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Up);

        // Should stay at 0
        assert_eq!(app_state.selected_peer_index, 0);
    }

    #[test]
    fn test_nav_down_peers_at_bottom_of_list() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.selected_peer_index = 1; // At the bottom (index 1 of 2)
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Down);

        // Should stay at 1
        assert_eq!(app_state.selected_peer_index, 1);
    }
}
