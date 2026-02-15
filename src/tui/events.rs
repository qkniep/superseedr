// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::app::AppState;
use crate::app::FileBrowserMode;
use crate::app::{App, AppMode, ConfigItem, SelectedHeader, TorrentControlState};
use crate::app::{BrowserPane, TorrentPreviewPayload};
use crate::torrent_manager::ManagerCommand;

use crate::tui::formatters::centered_rect;
use crate::tui::layout::calculate_file_browser_layout;
use crate::tui::layout::calculate_layout;
use crate::tui::layout::compute_visible_peer_columns;
use crate::tui::layout::compute_visible_torrent_columns;
use crate::tui::layout::LayoutContext;
use crate::tui::screens::{config, delete_confirm, normal, power, welcome};
use crate::tui::tree::RawNode;
use crate::tui::tree::TreeViewState;
use crate::tui::tree::{TreeAction, TreeFilter, TreeMathHelper};

use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind};
use ratatui::prelude::Rect;

use std::collections::HashMap;
use std::path::Path;
use tracing::{event as tracing_event, Level};

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
static GLOBAL_ESC_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
enum PriorityAction {
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
                return;
            }

            GLOBAL_ESC_TIMESTAMP.store(now, Ordering::Relaxed);
        }
    }

    if let CrosstermEvent::Key(key) = event {
        if matches!(app.app_state.mode, AppMode::Normal)
            && app.app_state.is_searching
            && key.kind == KeyEventKind::Press
        {
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
            if key.code == KeyCode::Char('m')
                && key.kind == KeyEventKind::Press
                && matches!(app.app_state.mode, AppMode::Normal)
            {
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
                    || (key.code == KeyCode::Char('m')
                        && key.kind == KeyEventKind::Release
                        && matches!(app.app_state.mode, AppMode::Normal))
                {
                    app.app_state.show_help = false;
                    help_key_handled = true;
                }
            } else if key.code == KeyCode::Char('m')
                && key.kind == KeyEventKind::Press
                && matches!(app.app_state.mode, AppMode::Normal)
            {
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
            welcome::handle_event(event, &mut app.app_state);
        }
        AppMode::Normal => normal::handle_event(event, app).await,
        AppMode::PowerSaving => power::handle_event(event, &mut app.app_state),
        AppMode::Config {
            settings_edit,
            selected_index,
            items,
            editing,
        } => {
            if let config::ConfigOutcome::ToNormal = config::handle_event(
                event,
                settings_edit,
                selected_index,
                items.as_mut_slice(),
                editing,
                &app.app_command_tx,
                &app.global_dl_bucket,
                &app.global_ul_bucket,
            ) {
                app.app_state.mode = AppMode::Normal;
            }
        }

        AppMode::FileBrowser {
            state,
            data,
            browser_mode,
        } => {
            if let CrosstermEvent::Key(key) = event {
                if key.kind == KeyEventKind::Press {
                    // 1. Search Interceptor
                    if app.app_state.is_searching {
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
                            match key.code {
                                KeyCode::Enter => {
                                    *is_editing_name = false;
                                }
                                KeyCode::Esc => {
                                    *container_name = original_name_backup.clone();
                                    *is_editing_name = false;
                                    *cursor_pos = container_name.len();
                                }
                                KeyCode::Left => {
                                    *cursor_pos = cursor_pos.saturating_sub(1);
                                }
                                KeyCode::Right => {
                                    if *cursor_pos < container_name.len() {
                                        *cursor_pos += 1;
                                    }
                                }
                                KeyCode::Backspace => {
                                    if *cursor_pos > 0 {
                                        container_name.remove(*cursor_pos - 1);
                                        *cursor_pos -= 1;
                                    }
                                }
                                KeyCode::Delete => {
                                    if *cursor_pos < container_name.len() {
                                        container_name.remove(*cursor_pos);
                                    }
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
                                if !app.app_state.pending_torrent_link.is_empty() {
                                    if let (Some(info_hash), _) = crate::app::parse_hybrid_hashes(
                                        &app.app_state.pending_torrent_link,
                                    ) {
                                        // 1. Grab reference to channel
                                        if let Some(manager_tx) =
                                            app.torrent_manager_command_txs.get(&info_hash)
                                        {
                                            let tx = manager_tx.clone();
                                            // 2. Send Kill Command asynchronously
                                            tokio::spawn(async move {
                                                if let Err(e) =
                                                    tx.send(ManagerCommand::DeleteFile).await
                                                {
                                                    tracing::error!("Failed to send DeleteFile to cancelled manager: {}", e);
                                                }
                                            });
                                        }

                                        // 3. Remove from UI immediately (Manager will kill itself upon receipt of DeleteFile)
                                        app.torrent_manager_command_txs.remove(&info_hash);
                                        app.app_state.torrents.remove(&info_hash);
                                        app.app_state
                                            .torrent_list_order
                                            .retain(|h| h != &info_hash);
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
                                app.app_state.ui_needs_redraw = true;
                                return;
                            }
                            KeyCode::Char('r') if *use_container => {
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
                                app.app_state.ui_needs_redraw = true;
                                return;
                            }
                            _ => {}
                        }

                        // Pane-Specific Navigation (Intercepting tree keys if focused on preview)
                        if let BrowserPane::TorrentPreview = focused_pane {
                            // 1. Re-calculate area logic from view.rs
                            let screen = app.app_state.screen_area;
                            let area = if screen.width < 60 {
                                screen
                            } else {
                                centered_rect(90, 80, screen)
                            };

                            // 2. Run the layout calculator
                            let layout = calculate_file_browser_layout(
                                area,
                                true, // DownloadLocSelection always has preview content
                                app.app_state.is_searching,
                                focused_pane,
                            );

                            if let Some(preview_rect) = layout.preview {
                                // 3. Calculate inner list height
                                let inner_height = preview_rect.height.saturating_sub(2); // Remove borders
                                                                                          // If using container, we have 2 header rows. Else 1.
                                let header_rows = if *use_container { 2 } else { 1 };
                                let list_height = inner_height.saturating_sub(header_rows) as usize;

                                let filter = TreeFilter::default();

                                match key.code {
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        TreeMathHelper::apply_action(
                                            preview_state,
                                            preview_tree,
                                            TreeAction::Up,
                                            filter,
                                            list_height,
                                        );
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        TreeMathHelper::apply_action(
                                            preview_state,
                                            preview_tree,
                                            TreeAction::Down,
                                            filter,
                                            list_height,
                                        );
                                    }
                                    KeyCode::Left | KeyCode::Char('h') => {
                                        TreeMathHelper::apply_action(
                                            preview_state,
                                            preview_tree,
                                            TreeAction::Left,
                                            filter,
                                            list_height,
                                        );
                                    }
                                    KeyCode::Right | KeyCode::Char('l') => {
                                        TreeMathHelper::apply_action(
                                            preview_state,
                                            preview_tree,
                                            TreeAction::Right,
                                            filter,
                                            list_height,
                                        );
                                    }
                                    KeyCode::Char(' ') => {
                                        if let Some(t) = &preview_state.cursor_path {
                                            apply_priority_action(
                                                preview_tree,
                                                t,
                                                PriorityAction::Cycle,
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            app.app_state.ui_needs_redraw = true;
                            // If focused on TorrentPreview, we don't want FileSystem navigation to trigger
                            // except for confirming the whole thing via SHIFT+Y.
                            if key.code != KeyCode::Char('Y') {
                                return;
                            }
                        }
                    }

                    // 1. Determine Preview Status
                    let has_preview_content = match browser_mode {
                        FileBrowserMode::DownloadLocSelection { .. } => {
                            app.app_state.pending_torrent_path.is_some()
                                || !app.app_state.pending_torrent_link.is_empty()
                        }
                        FileBrowserMode::File(_) => state
                            .cursor_path
                            .as_ref()
                            .is_some_and(|p| p.extension().is_some_and(|ext| ext == "torrent")),
                        _ => false,
                    };

                    // 2. Determine Focused Pane
                    let default_pane = BrowserPane::FileSystem;
                    let focused_pane =
                        if let FileBrowserMode::DownloadLocSelection { focused_pane, .. } =
                            browser_mode
                        {
                            focused_pane
                        } else {
                            &default_pane
                        };

                    // 3. Determine Area (Matching view.rs logic exactly)
                    let screen = app.app_state.screen_area;
                    let area = if has_preview_content {
                        if screen.width < 60 {
                            screen
                        } else {
                            centered_rect(90, 80, screen)
                        }
                    } else if screen.width < 40 {
                        screen
                    } else {
                        centered_rect(75, 80, screen)
                    };

                    // 4. Calculate Layout
                    let layout = calculate_file_browser_layout(
                        area,
                        has_preview_content,
                        app.app_state.is_searching,
                        focused_pane,
                    );

                    // 5. Get EXACT List Height (Height - 2 for Borders)
                    let list_height = layout.list.height.saturating_sub(2) as usize;

                    let filter = match browser_mode {
                        FileBrowserMode::Directory
                        | FileBrowserMode::DownloadLocSelection { .. }
                        | FileBrowserMode::ConfigPathSelection { .. } => {
                            TreeFilter::from_text(&app.app_state.search_query)
                        }
                        FileBrowserMode::File(extensions) => {
                            let exts = extensions.clone();
                            TreeFilter::new(&app.app_state.search_query, move |node| {
                                node.is_dir || exts.iter().any(|ext| node.name.ends_with(ext))
                            })
                        }
                    };

                    match key.code {
                        KeyCode::Char('/') => {
                            app.app_state.is_searching = true;
                            app.app_state.search_query.clear();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            TreeMathHelper::apply_action(
                                state,
                                data,
                                TreeAction::Up,
                                filter,
                                list_height,
                            );
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            TreeMathHelper::apply_action(
                                state,
                                data,
                                TreeAction::Down,
                                filter,
                                list_height,
                            );
                        }
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                            if let Some(path) = state.cursor_path.clone() {
                                if path.is_dir() {
                                    // Entering a directory starts a fresh listing context.
                                    app.app_state.is_searching = false;
                                    app.app_state.search_query.clear();
                                    let _ =
                                        app.app_command_tx.try_send(AppCommand::FetchFileTree {
                                            path,
                                            browser_mode: browser_mode.clone(),
                                            highlight_path: None,
                                        });
                                }
                            }
                        }
                        KeyCode::Backspace
                        | KeyCode::Left
                        | KeyCode::Char('h')
                        | KeyCode::Char('u') => {
                            let child_to_highlight = state.current_path.clone();
                            if let Some(parent) = state.current_path.parent() {
                                let _ = app.app_command_tx.try_send(AppCommand::FetchFileTree {
                                    path: parent.to_path_buf(),
                                    browser_mode: browser_mode.clone(),
                                    highlight_path: Some(child_to_highlight),
                                });
                            }
                        }

                        KeyCode::Char('Y') => {
                            match browser_mode {
                                FileBrowserMode::ConfigPathSelection {
                                    target_item,
                                    current_settings,
                                    selected_index,
                                    items,
                                } => {
                                    tracing::info!(target: "superseedr", "Confirming Config Path Selection");
                                    let mut new_settings = current_settings.clone();
                                    let selected_path = state.current_path.clone();

                                    match target_item {
                                        ConfigItem::DefaultDownloadFolder => {
                                            new_settings.default_download_folder =
                                                Some(selected_path)
                                        }
                                        ConfigItem::WatchFolder => {
                                            new_settings.watch_folder = Some(selected_path)
                                        }
                                        _ => {}
                                    }

                                    app.app_state.mode = AppMode::Config {
                                        settings_edit: new_settings,
                                        selected_index: *selected_index,
                                        items: items.clone(),
                                        editing: None,
                                    };
                                }

                                FileBrowserMode::DownloadLocSelection {
                                    container_name,
                                    use_container,
                                    preview_tree,
                                    ..
                                } => {
                                    let base_path = state.current_path.clone();
                                    let container_name_to_use = if *use_container {
                                        Some(container_name.clone())
                                    } else {
                                        Some(String::new())
                                    };

                                    let mut file_priorities = HashMap::new();
                                    for node in preview_tree {
                                        node.collect_priorities(&mut file_priorities);
                                    }

                                    if let Some(pending_path) =
                                        app.app_state.pending_torrent_path.take()
                                    {
                                        app.add_torrent_from_file(
                                            pending_path,
                                            Some(base_path),
                                            false,
                                            TorrentControlState::Running,
                                            file_priorities,
                                            container_name_to_use.clone(),
                                        )
                                        .await;
                                    } else if !app.app_state.pending_torrent_link.is_empty() {
                                        app.add_magnet_torrent(
                                            "Fetching name...".to_string(),
                                            app.app_state.pending_torrent_link.clone(),
                                            Some(base_path),
                                            false,
                                            TorrentControlState::Running,
                                            file_priorities,
                                            container_name_to_use,
                                        )
                                        .await;
                                        app.app_state.pending_torrent_link.clear();
                                    } else {
                                        tracing::warn!(target: "superseedr", "SHIFT+Y pressed but no pending content was found");
                                    }

                                    app.app_state.mode = AppMode::Normal;
                                }

                                FileBrowserMode::File(extensions) => {
                                    if let Some(path) = state.cursor_path.clone() {
                                        let name =
                                            path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                        if extensions.iter().any(|ext| name.ends_with(ext)) {
                                            if name.ends_with(".torrent") {
                                                let _ = app
                                                    .app_command_tx
                                                    .send(AppCommand::AddTorrentFromFile(path))
                                                    .await;
                                            }
                                            app.app_state.mode = AppMode::Normal;
                                        }
                                    }
                                }

                                _ => {}
                            }
                            app.app_state.is_searching = false;
                            app.app_state.search_query.clear();
                        }

                        KeyCode::Esc => {
                            if let FileBrowserMode::ConfigPathSelection {
                                current_settings,
                                selected_index,
                                items,
                                ..
                            } = browser_mode
                            {
                                app.app_state.mode = AppMode::Config {
                                    settings_edit: current_settings.clone(),
                                    selected_index: *selected_index,
                                    items: items.clone(),
                                    editing: None,
                                };
                                app.app_state.ui_needs_redraw = true;
                                return;
                            }

                            if let FileBrowserMode::DownloadLocSelection { .. } = browser_mode {
                                if !app.app_state.pending_torrent_link.is_empty() {
                                    // 1. Calculate the hash to find the entry
                                    if let (Some(info_hash), _) = crate::app::parse_hybrid_hashes(
                                        &app.app_state.pending_torrent_link,
                                    ) {
                                        // 2. Shut down the manager
                                        if let Some(manager_tx) =
                                            app.torrent_manager_command_txs.remove(&info_hash)
                                        {
                                            let _ = manager_tx.try_send(ManagerCommand::DeleteFile);
                                        }
                                        // 3. Remove from UI state
                                        app.app_state.torrents.remove(&info_hash);
                                        app.app_state
                                            .torrent_list_order
                                            .retain(|h| h != &info_hash);
                                    }
                                }
                            }

                            app.app_state.is_searching = false;
                            app.app_state.search_query.clear();
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
            let info_hash = info_hash.clone();
            let with_files = *with_files;
            if delete_confirm::handle_event(event, app, info_hash, with_files) {
                app.app_state.mode = AppMode::Normal;
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

pub(crate) fn handle_navigation(app_state: &mut AppState, key_code: KeyCode) {
    let selected_torrent = app_state
        .torrent_list_order
        .get(app_state.selected_torrent_index)
        .and_then(|info_hash| app_state.torrents.get(info_hash));

    let selected_torrent_has_peers =
        selected_torrent.is_some_and(|torrent| !torrent.latest_state.peers.is_empty());

    let selected_torrent_peer_count =
        selected_torrent.map_or(0, |torrent| torrent.latest_state.peers.len());

    let layout_ctx = LayoutContext::new(app_state.screen_area, app_state, 35);
    let layout_plan = calculate_layout(app_state.screen_area, &layout_ctx);
    let (_, visible_torrent_columns) =
        compute_visible_torrent_columns(app_state, layout_plan.list.width);
    let (_, visible_peer_columns) = compute_visible_peer_columns(layout_plan.peers.width);
    let torrent_col_count = visible_torrent_columns.len();
    let peer_col_count = visible_peer_columns.len();

    app_state.selected_header = match app_state.selected_header {
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

pub(crate) async fn handle_pasted_text(app: &mut App, pasted_text: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        AppState, FilePriority, PeerInfo, SelectedHeader, TorrentDisplayState, TorrentMetrics,
        TorrentPreviewPayload,
    };
    use crate::tui::tree::RawNode;
    use ratatui::crossterm::event::KeyCode;
    use std::path::PathBuf;

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
        app_state.selected_header = SelectedHeader::Torrent(99);

        handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.selected_header, SelectedHeader::Peer(0));
    }

    #[test]
    fn test_nav_right_to_peers_when_no_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 1; // "hash_b" has 0 peers
        app_state.selected_header = SelectedHeader::Torrent(99);

        handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.selected_header, SelectedHeader::Torrent(0));
    }

    #[test]
    fn test_nav_left_from_peers() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0;
        app_state.selected_header = SelectedHeader::Peer(0);

        handle_navigation(&mut app_state, KeyCode::Left);

        assert_eq!(app_state.selected_header, SelectedHeader::Torrent(0));
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

    #[test]
    fn test_nav_right_jumps_to_peers_when_only_name_column_visible() {
        let mut app_state = create_test_app_state();
        app_state.selected_torrent_index = 0;
        app_state.selected_header = SelectedHeader::Torrent(0);

        if let Some(torrent) = app_state.torrents.get_mut("hash_a".as_bytes()) {
            torrent.latest_state.activity_message = "Seeding".to_string();
            torrent.latest_state.number_of_pieces_total = 100;
            torrent.latest_state.number_of_pieces_completed = 100;
        }

        for torrent in app_state.torrents.values_mut() {
            torrent.smoothed_download_speed_bps = 0;
            torrent.smoothed_upload_speed_bps = 0;
        }

        handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.selected_header, SelectedHeader::Peer(0));
    }

    #[test]
    fn test_apply_priority_action_cycles_target_and_children() {
        let mut nodes = vec![RawNode {
            name: "root".to_string(),
            full_path: PathBuf::from("root"),
            is_dir: true,
            payload: TorrentPreviewPayload::default(),
            children: vec![RawNode {
                name: "leaf.bin".to_string(),
                full_path: PathBuf::from("root/leaf.bin"),
                is_dir: false,
                payload: TorrentPreviewPayload::default(),
                children: vec![],
            }],
        }];

        let changed = apply_priority_action(
            &mut nodes,
            &PathBuf::from("root"),
            PriorityAction::Cycle,
        );

        assert!(changed);
        assert_eq!(nodes[0].payload.priority, FilePriority::Skip);
        assert_eq!(nodes[0].children[0].payload.priority, FilePriority::Skip);
    }

    #[test]
    fn test_apply_priority_action_returns_false_for_missing_path() {
        let mut nodes = vec![RawNode {
            name: "root".to_string(),
            full_path: PathBuf::from("root"),
            is_dir: true,
            payload: TorrentPreviewPayload::default(),
            children: vec![],
        }];

        let changed = apply_priority_action(
            &mut nodes,
            &PathBuf::from("missing"),
            PriorityAction::Cycle,
        );

        assert!(!changed);
        assert_eq!(nodes[0].payload.priority, FilePriority::Normal);
    }
}
