// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppCommand;
use crate::app::AppState;
use crate::app::FileBrowserMode;
use crate::app::{App, AppMode, SelectedHeader, TorrentControlState};
use crate::app::{BrowserPane, TorrentPreviewPayload};
use crate::torrent_manager::ManagerCommand;

use crate::tui::layout::calculate_layout;
use crate::tui::layout::compute_visible_peer_columns;
use crate::tui::layout::compute_visible_torrent_columns;
use crate::tui::layout::LayoutContext;
use crate::tui::screens::{browser, config, delete_confirm, normal, power, welcome};
use crate::tui::tree::RawNode;
use crate::tui::tree::TreeViewState;

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
                    if browser::handle_search_interceptor(
                        key.code,
                        &mut app.app_state.is_searching,
                        &mut app.app_state.search_query,
                    ) {
                        app.app_state.ui_needs_redraw = true;
                        return; // INTERCEPTED
                    }

                    if browser::handle_download_name_edit_guard(key.code, browser_mode) {
                        app.app_state.ui_needs_redraw = true;
                        return; // INTERCEPTED
                    }
                    if browser::handle_download_shortcuts(key.code, browser_mode) {
                        app.app_state.ui_needs_redraw = true;
                        return;
                    }

                    // 2. Mode-Specific Guard (Download Selection)
                    if let FileBrowserMode::DownloadLocSelection {
                        use_container,
                        focused_pane,
                        preview_tree,
                        preview_state,
                        ..
                    } = browser_mode
                    {
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
                            _ => {}
                        }

                        // Pane-Specific Navigation (Intercepting tree keys if focused on preview)
                        if let BrowserPane::TorrentPreview = focused_pane {
                            if let Some(list_height) = browser::calculate_preview_list_height(
                                app.app_state.screen_area,
                                app.app_state.is_searching,
                                focused_pane,
                                *use_container,
                            ) {
                                if !browser::apply_preview_navigation(
                                    key.code,
                                    preview_state,
                                    preview_tree,
                                    list_height,
                                ) && key.code == KeyCode::Char(' ')
                                {
                                    if let Some(t) = &preview_state.cursor_path {
                                        apply_priority_action(preview_tree, t, PriorityAction::Cycle);
                                    }
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

                    let has_preview_content = browser::has_preview_content(
                        browser_mode,
                        app.app_state.pending_torrent_path.is_some(),
                        !app.app_state.pending_torrent_link.is_empty(),
                        state.cursor_path.as_ref(),
                    );
                    let focused_pane = browser::focused_pane(browser_mode);
                    let list_height = browser::calculate_list_height(
                        app.app_state.screen_area,
                        has_preview_content,
                        app.app_state.is_searching,
                        &focused_pane,
                    );
                    match key.code {
                        key_code
                            if browser::handle_filesystem_navigation(
                                key_code,
                                state,
                                data,
                                browser_mode,
                                &mut app.app_state.is_searching,
                                &mut app.app_state.search_query,
                                list_height,
                                &app.app_command_tx,
                            ) => {}
                        KeyCode::Char('Y') => {
                            if let Some(mode) = browser::confirm_config_path_selection(state, browser_mode) {
                                tracing::info!(target: "superseedr", "Confirming Config Path Selection");
                                app.app_state.mode = mode;
                            } else {
                                match browser_mode {
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

                                    FileBrowserMode::File(_) => {
                                        if let Some(path) =
                                            browser::selected_torrent_file_for_confirm(state, browser_mode)
                                        {
                                            if path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .is_some_and(|name| name.ends_with(".torrent"))
                                            {
                                                let _ = app
                                                    .app_command_tx
                                                    .send(AppCommand::AddTorrentFromFile(path))
                                                    .await;
                                            }
                                            app.app_state.mode = AppMode::Normal;
                                        }
                                    }

                                    _ => {}
                                }
                            }
                            app.app_state.is_searching = false;
                            app.app_state.search_query.clear();
                        }

                        KeyCode::Esc => {
                            if let Some(mode) = browser::escape_to_config_mode(browser_mode) {
                                app.app_state.mode = mode;
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
