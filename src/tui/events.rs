// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::app::{App, AppMode, ConfigItem, SelectedHeader, TorrentControlState};
use crate::app::{BrowserPane, FilePriority, TorrentPreviewPayload};
use crate::app::AppCommand;
use crate::app::FileBrowserMode;
use crate::torrent_manager::ManagerCommand;

use strum::IntoEnumIterator;

use crate::config::SortDirection;

use crate::tui::layout::calculate_layout;
use crate::tui::layout::compute_smart_table_layout;
use crate::tui::layout::get_peer_columns;
use crate::tui::layout::get_torrent_columns;
use crate::tui::layout::LayoutContext;
use crate::tui::layout::SmartCol;
use crate::tui::tree::{TreeMathHelper, TreeAction, TreeFilter};
use crate::tui::tree::RawNode;
use crate::tui::tree::TreeViewState;
use crate::tui::formatters::centered_rect;

use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::prelude::Rect;
use ratatui::style::{Color, Style};

use std::path::Path;
use std::collections::HashMap;
use tracing::{event as tracing_event, Level};

use directories::UserDirs;


#[cfg(windows)]
use clipboard::{ClipboardContext, ClipboardProvider};

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
static GLOBAL_ESC_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
enum PriorityAction {
    Set(FilePriority),
    Cycle,
}

pub async fn handle_event(event: CrosstermEvent, app: &mut App) {
    if let CrosstermEvent::Resize(w, h) = &event {
        app.app_state.screen_area = Rect::new(0, 0, *w, *h);
        app.app_state.ui_needs_redraw = true;
        return;
    }

    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let last = GLOBAL_ESC_TIMESTAMP.load(Ordering::Relaxed);

            // If the last Esc was less than 200ms ago, ignore this one
            if now.saturating_sub(last) < 200 {
                tracing::info!("GLOBAL DEBOUNCE: Ignoring rapid Esc key press");
                return; 
            }

            // Otherwise, update the timestamp and let the event proceed
            GLOBAL_ESC_TIMESTAMP.store(now, Ordering::Relaxed);
        }
    }

    if let CrosstermEvent::Key(key) = event {
        if matches!(app.app_state.mode, AppMode::Normal) && app.app_state.is_searching && key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Esc => {
                    app.app_state.is_searching = false;
                    app.app_state.search_query.clear();
                    app.sort_and_filter_torrent_list();
                    app.app_state.selected_torrent_index = 0;
                }
                KeyCode::Enter => {
                    app.app_state.is_searching = false;
                }
                KeyCode::Backspace => {
                    app.app_state.search_query.pop();
                    app.sort_and_filter_torrent_list();
                    app.app_state.selected_torrent_index = 0;
                }
                KeyCode::Char(c) => {
                    app.app_state.search_query.push(c);
                    app.sort_and_filter_torrent_list();
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
            if key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Press && matches!(app.app_state.mode, AppMode::Normal) {
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
                    || (key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Release && matches!(app.app_state.mode, AppMode::Normal))
                {
                    app.app_state.show_help = false;
                    help_key_handled = true;
                }
            } else if key.code == KeyCode::Char('m') && key.kind == KeyEventKind::Press && matches!(app.app_state.mode, AppMode::Normal) {
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
                            match app.app_state.selected_header {
                                SelectedHeader::Torrent(i) => {
                                    let cols = get_torrent_columns();

                                    if let Some(def) = cols.get(i) {
                                        if let Some(column) = def.sort_enum {
                                            if app.app_state.torrent_sort.0 == column {
                                                app.app_state.torrent_sort.1 =
                                                    if app.app_state.torrent_sort.1
                                                        == SortDirection::Ascending
                                                    {
                                                        SortDirection::Descending
                                                    } else {
                                                        SortDirection::Ascending
                                                    };
                                            } else {
                                                app.app_state.torrent_sort.0 = column;
                                                app.app_state.torrent_sort.1 =
                                                    SortDirection::Descending;
                                            }
                                            app.sort_and_filter_torrent_list();
                                        }
                                    }
                                }
                                SelectedHeader::Peer(i) => {
                                    let cols = get_peer_columns();

                                    if let Some(def) = cols.get(i) {
                                        if let Some(column) = def.sort_enum {
                                            if app.app_state.peer_sort.0 == column {
                                                app.app_state.peer_sort.1 =
                                                    if app.app_state.peer_sort.1
                                                        == SortDirection::Ascending
                                                    {
                                                        SortDirection::Descending
                                                    } else {
                                                        SortDirection::Ascending
                                                    };
                                            } else {
                                                app.app_state.peer_sort.0 = column;
                                                app.app_state.peer_sort.1 =
                                                    SortDirection::Descending;
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
                            let _ = app.app_command_tx.try_send(AppCommand::UpdateConfig(*settings_edit.clone()));
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
                                    let initial_path = if selected_item == ConfigItem::WatchFolder {
                                        settings_edit.watch_folder.clone()
                                    } else {
                                        settings_edit.default_download_folder.clone()
                                    }.unwrap_or_else(|| {
                                        UserDirs::new()
                                            .and_then(|ud| ud.download_dir().map(|p| p.to_path_buf()))
                                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    });

                                    // Send command to switch mode, PASSING the current state
                                    let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                                        path: initial_path,
                                        // Carry the state into the browser
                                        browser_mode: FileBrowserMode::ConfigPathSelection {
                                            target_item: selected_item,
                                            current_settings: settings_edit.clone(), // Clone the edits so far
                                            selected_index: *selected_index,
                                            items: items.clone(),
                                        },
                                        highlight_path: None,
                                    });
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

AppMode::FileBrowser { state, data, browser_mode } => {
    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press {

            // 1. Search Interceptor
            if app.app_state.is_searching {
                tracing::debug!(target: "superseedr", "Key captured by search buffer");
                match key.code {
                    KeyCode::Esc => {
                        app.app_state.is_searching = false;
                        app.app_state.search_query.clear();
                    }
                    KeyCode::Enter => {
                        app.app_state.is_searching = false;
                    }
                    KeyCode::Backspace => {
                        app.app_state.search_query.pop();
                    }
                    KeyCode::Char(c) => {
                        app.app_state.search_query.push(c);
                    }
                    _ => {} 
                }
                app.app_state.ui_needs_redraw = true;
                return; // INTERCEPTED
            }

            // 2. Mode-Specific Guard (Download Selection)
            if let FileBrowserMode::DownloadLocSelection { 
                container_name, 
                use_container, 
                is_editing_name, 
                focused_pane,
                preview_tree,
                preview_state,
                cursor_pos,
                original_name_backup,
                .. 
            } = browser_mode 
            {
                // Input Guard for Renaming the Container
                if *is_editing_name {
                    tracing::debug!(target: "superseedr", "Key captured by rename editor");
                    match key.code {
                        KeyCode::Enter => { *is_editing_name = false; }
                        KeyCode::Esc => {
                            *container_name = original_name_backup.clone(); 
                            *is_editing_name = false;
                            *cursor_pos = container_name.len();
                        }
                        KeyCode::Left => { *cursor_pos = cursor_pos.saturating_sub(1); }
                        KeyCode::Right => { if *cursor_pos < container_name.len() { *cursor_pos += 1; } }
                        KeyCode::Backspace => {
                            if *cursor_pos > 0 {
                                container_name.remove(*cursor_pos - 1);
                                *cursor_pos -= 1;
                            }
                        }
                        KeyCode::Delete => {
                            if *cursor_pos < container_name.len() { container_name.remove(*cursor_pos); }
                        }
                        KeyCode::Char(c) => {
                            container_name.insert(*cursor_pos, c);
                            *cursor_pos += 1;
                        }
                        _ => {}
                    }
                    app.app_state.ui_needs_redraw = true;
                    return; // INTERCEPTED
                }

                // Global Actions (within Download Selection)
                match key.code {
                    KeyCode::Esc => {
                        tracing::info!(target: "superseedr", "ESC pressed: Cancelling download selection");
                        if !app.app_state.pending_torrent_link.is_empty() {
                            if let (Some(info_hash), _) = crate::app::parse_hybrid_hashes(&app.app_state.pending_torrent_link) {
                                // 1. Grab reference to channel
                                if let Some(manager_tx) = app.torrent_manager_command_txs.get(&info_hash) {
                                    let tx = manager_tx.clone();
                                    // 2. Send Kill Command asynchronously
                                    tokio::spawn(async move {
                                        if let Err(e) = tx.send(ManagerCommand::DeleteFile).await {
                                            tracing::error!("Failed to send DeleteFile to cancelled manager: {}", e);
                                        }
                                    });
                                }
                                
                                // 3. Remove from UI immediately (Manager will kill itself upon receipt of DeleteFile)
                                app.torrent_manager_command_txs.remove(&info_hash);
                                app.app_state.torrents.remove(&info_hash);
                                app.app_state.torrent_list_order.retain(|h| h != &info_hash);
                            }
                        }

                        app.app_state.mode = AppMode::Normal;
                        app.app_state.pending_torrent_path = None;
                        app.app_state.pending_torrent_link.clear();
                        app.app_state.ui_needs_redraw = true;
                        return;
                    }
                    KeyCode::Char('x') => {
                        *use_container = !*use_container;
                        tracing::info!(target: "superseedr", "Toggle container folder: {}", *use_container);
                        app.app_state.ui_needs_redraw = true;
                        return;
                    }
                    KeyCode::Char('r') if *use_container => {
                        tracing::info!(target: "superseedr", "Triggering rename of container folder");
                        *is_editing_name = true;
                        *original_name_backup = container_name.clone(); 
                        *cursor_pos = container_name.len(); 
                        *focused_pane = BrowserPane::TorrentPreview;
                        app.app_state.ui_needs_redraw = true;
                        return;
                    }
                    KeyCode::Tab => {
                        *focused_pane = match focused_pane {
                            BrowserPane::FileSystem => BrowserPane::TorrentPreview,
                            BrowserPane::TorrentPreview => BrowserPane::FileSystem,
                        };
                        tracing::info!(target: "superseedr", "Tab pressed: Switched focus to {:?}", focused_pane);
                        app.app_state.ui_needs_redraw = true;
                        return;
                    }
                    _ => {} 
                }

                // Pane-Specific Navigation (Intercepting tree keys if focused on preview)
                if let BrowserPane::TorrentPreview = focused_pane {
                    let area = centered_rect(90, 80, app.app_state.screen_area);
                    let list_height = area.height.saturating_sub(4) as usize; 
                    let filter = TreeFilter::default();

                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { TreeMathHelper::apply_action(preview_state, preview_tree, TreeAction::Up, filter, list_height); }
                        KeyCode::Down | KeyCode::Char('j') => { TreeMathHelper::apply_action(preview_state, preview_tree, TreeAction::Down, filter, list_height); }
                        KeyCode::Left | KeyCode::Char('h') => { TreeMathHelper::apply_action(preview_state, preview_tree, TreeAction::Left, filter, list_height); }
                        KeyCode::Right | KeyCode::Char('l') => { TreeMathHelper::apply_action(preview_state, preview_tree, TreeAction::Right, filter, list_height); }
                        KeyCode::Char(' ') => { if let Some(t) = &preview_state.cursor_path { apply_priority_action(preview_tree, t, PriorityAction::Cycle); } }
                        _ => { 
                            tracing::debug!(target: "superseedr", "Key {:?} unhandled in TorrentPreview pane", key.code);
                        }
                    }
                    app.app_state.ui_needs_redraw = true;
                    // If focused on TorrentPreview, we don't want FileSystem navigation to trigger
                    // except for confirming the whole thing via SHIFT+C.
                    if key.code != KeyCode::Char('C') {
                        return;
                    }
                }
            }

            // 3. General Browser Navigation Setup
            let area = crate::tui::formatters::centered_rect(75, 80, app.app_state.screen_area);
            let max_height = area.height.saturating_sub(2) as usize;

            let filter = match browser_mode {
                FileBrowserMode::Directory 
                | FileBrowserMode::DownloadLocSelection { .. } 
                | FileBrowserMode::ConfigPathSelection { .. } => {
                     TreeFilter::from_text(&app.app_state.search_query)
                },
                FileBrowserMode::File(extensions) => {
                    let exts = extensions.clone();
                    TreeFilter::new(&app.app_state.search_query, move |node| {
                        node.is_dir || exts.iter().any(|ext| node.name.ends_with(ext))
                    })
                }
            };

            // 4. Main Key Match
            match key.code {
                KeyCode::Char('/') => {
                    app.app_state.is_searching = true;
                    app.app_state.search_query.clear();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    TreeMathHelper::apply_action(state, data, TreeAction::Up, filter, max_height);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    TreeMathHelper::apply_action(state, data, TreeAction::Down, filter, max_height);
                }
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(path) = state.cursor_path.clone() {
                        if path.is_dir() {
                            tracing::info!(target: "superseedr", "Entering directory: {:?}", path);
                            let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                                path,
                                browser_mode: browser_mode.clone(),
                                highlight_path: None,
                            });
                        }
                    }
                }
                KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                    let child_to_highlight = state.current_path.clone();
                    if let Some(parent) = state.current_path.parent() {
                        tracing::info!(target: "superseedr", "Navigating to parent: {:?}", parent);
                        let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                            path: parent.to_path_buf(),
                            browser_mode: browser_mode.clone(),
                            highlight_path: Some(child_to_highlight),
                        });
                    }
                }

                // --- THE CRITICAL CONFIRMATION ACTION ---
                KeyCode::Char('C') => {
                    tracing::info!(target: "superseedr", "SHIFT+C triggered for confirmation");
                    match browser_mode {
                        FileBrowserMode::ConfigPathSelection { target_item, current_settings, selected_index, items } => {
                            tracing::info!(target: "superseedr", "Confirming Config Path Selection");
                            let mut new_settings = current_settings.clone();
                            let selected_path = state.cursor_path.clone().unwrap_or_else(|| state.current_path.clone());

                            match target_item {
                                ConfigItem::DefaultDownloadFolder => new_settings.default_download_folder = Some(selected_path),
                                ConfigItem::WatchFolder => new_settings.watch_folder = Some(selected_path),
                                _ => {}
                            }

                            app.app_state.mode = AppMode::Config {
                                settings_edit: new_settings,
                                selected_index: *selected_index,
                                items: items.clone(),
                                editing: None,
                            };
                        }

                        FileBrowserMode::DownloadLocSelection { container_name, use_container, preview_tree, .. } => {
                            tracing::info!(target: "superseedr", "Confirming Download Location Selection");
                            let mut base_path = state.current_path.clone();
                            let final_path = if *use_container {
                                Some(base_path.join(container_name))
                            } else {
                                Some(base_path)
                            };

                            tracing::info!(target: "superseedr", "Final resolved path: {:?}", final_path);
                            let mut file_priorities = HashMap::new();
                            for node in preview_tree {
                                node.collect_priorities(&mut file_priorities);
                            }

                            if let Some(pending_path) = app.app_state.pending_torrent_path.take() {
                                tracing::info!(target: "superseedr", "Executing: Add torrent from file");
                                app.add_torrent_from_file(
                                    pending_path,
                                    final_path,
                                    false,
                                    TorrentControlState::Running,
                                    file_priorities,
                                ).await;
                            } else if !app.app_state.pending_torrent_link.is_empty() {
                                tracing::info!(target: "superseedr", "Executing: Add magnet link");
                                app.add_magnet_torrent(
                                    "Fetching name...".to_string(),
                                    app.app_state.pending_torrent_link.clone(),
                                    final_path,
                                    false,
                                    TorrentControlState::Running,
                                    file_priorities,
                                ).await;
                                app.app_state.pending_torrent_link.clear();
                            } else {
                                tracing::warn!(target: "superseedr", "SHIFT+C pressed but no pending content was found");
                            }

                            app.app_state.mode = AppMode::Normal;
                        }

                        FileBrowserMode::File(extensions) => {
                            tracing::info!(target: "superseedr", "Confirming File Selection");
                            if let Some(path) = state.cursor_path.clone() {
                                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                if extensions.iter().any(|ext| name.ends_with(ext)) {
                                    if name.ends_with(".torrent") {
                                        let _ = app.app_command_tx.send(AppCommand::AddTorrentFromFile(path)).await;
                                    }
                                    app.app_state.mode = AppMode::Normal;
                                }
                            }
                        }
                        
                        _ => {
                            tracing::info!(target: "superseedr", "SHIFT+C pressed in generic directory mode: closing browser");
                            app.app_state.mode = AppMode::Normal;
                        }
                    }
                    app.app_state.ui_needs_redraw = true;
                }

                KeyCode::Esc => {
                    tracing::info!(target: "superseedr", "ESC pressed: closing browser");
                    
                    if let FileBrowserMode::DownloadLocSelection { .. } = browser_mode {
                        if !app.app_state.pending_torrent_link.is_empty() {
                            // 1. Calculate the hash to find the entry
                            if let (Some(info_hash), _) = crate::app::parse_hybrid_hashes(&app.app_state.pending_torrent_link) {
                                // 2. Shut down the manager
                                if let Some(manager_tx) = app.torrent_manager_command_txs.remove(&info_hash) {
                                    let _ = manager_tx.try_send(ManagerCommand::DeleteFile);
                                }
                                // 3. Remove from UI state
                                app.app_state.torrents.remove(&info_hash);
                                app.app_state.torrent_list_order.retain(|h| h != &info_hash);
                            }
                        }
                    }

                    app.app_state.mode = AppMode::Normal;
                    app.app_state.pending_torrent_path = None;
                    app.app_state.pending_torrent_link.clear();
                    app.app_state.ui_needs_redraw = true;
                }
                _ => {}
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

fn apply_priority_action(
    nodes: &mut [RawNode<TorrentPreviewPayload>],
    target_path: &Path,
    action: PriorityAction,
) -> bool {
    for node in nodes {
        // CHANGED: We explicitly pass a mutable reference (&mut |...|)
        let found = node.find_and_act(target_path, &mut |target_node| {
            // 1. Determine the new priority
            let new_priority = match action {
                PriorityAction::Set(p) => p,
                PriorityAction::Cycle => target_node.payload.priority.next(),
            };

            // 2. Apply this priority to the target node AND all its children
            target_node.apply_recursive(&|n| {
                n.payload.priority = new_priority;
            });
        });

        if found {
            return true;
        }
    }
    false
}

fn handle_navigation(app_state: &mut AppState, key_code: KeyCode) {
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let selected_torrent_has_peers =
        selected_torrent.is_some_and(|torrent| !torrent.latest_state.peers.is_empty());

    let selected_torrent_peer_count =
        selected_torrent.map_or(0, |torrent| torrent.latest_state.peers.len());

    let ctx = LayoutContext::new(app_state.screen_area, app_state, 35);
    let layout_plan = calculate_layout(app_state.screen_area, &ctx);

    let t_cols = get_torrent_columns();
    let smart_t_cols: Vec<SmartCol> = t_cols
        .iter()
        .map(|c| SmartCol {
            min_width: c.min_width,
            priority: c.priority,
            constraint: c.default_constraint,
        })
        .collect();
    let (_, visible_t_indices) =
        compute_smart_table_layout(&smart_t_cols, layout_plan.list.width, 1);
    let torrent_col_count = visible_t_indices.len();

    let p_cols = get_peer_columns();
    let smart_p_cols: Vec<SmartCol> = p_cols
        .iter()
        .map(|c| SmartCol {
            min_width: c.min_width,
            priority: c.priority,
            constraint: c.default_constraint,
        })
        .collect();
    let (_, visible_p_indices) =
        compute_smart_table_layout(&smart_p_cols, layout_plan.peers.width, 1);
    let peer_col_count = visible_p_indices.len();

    match key_code {
        // --- UP/DOWN/J/K Navigation (Rows) ---
        KeyCode::Up | KeyCode::Char('k') => match app_state.selected_header {
            SelectedHeader::Torrent(_) => {
                app_state.selected_torrent_index =
                    app_state.selected_torrent_index.saturating_sub(1);
                app_state.selected_peer_index = 0;
            }
            SelectedHeader::Peer(_) => {
                app_state.selected_peer_index = app_state.selected_peer_index.saturating_sub(1);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match app_state.selected_header {
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
        },

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
                }

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
    let pasted_text = pasted_text.trim();
    
    if pasted_text.starts_with("magnet:") {
        tracing::info!(target: "magnet_flow", "Magnet link detected in clipboard");
        
        let download_path = app.client_configs.default_download_folder.clone();

        app.add_magnet_torrent(
            "Fetching name...".to_string(),
            pasted_text.to_string(),
            download_path.clone(),
            false,
            TorrentControlState::Running,
            HashMap::new(),
        )
        .await;

        if download_path.is_none() {
            app.app_state.pending_torrent_link = pasted_text.to_string();
            let initial_path = app.get_initial_destination_path();
            let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                path: initial_path,
                browser_mode: FileBrowserMode::DownloadLocSelection {
                    torrent_files: vec![],
                    container_name: "Magnet Download".to_string(),
                    use_container: true,
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
                )
                .await;
            } else {
                // FIX 3: Redirect to AddTorrentFromFile command
                // This lets app.rs handle metadata parsing and tree building
                let _ = app.app_command_tx.try_send(AppCommand::AddTorrentFromFile(path.to_path_buf()));
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
    use strum::EnumCount;

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
        let mut app_state = AppState {
            screen_area: ratatui::layout::Rect::new(0, 0, 200, 100),
            ..Default::default()
        };

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
