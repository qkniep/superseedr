// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::{App, AppMode};

use crate::tui::screens::{browser, config, delete_confirm, help, normal, power, rss, welcome};

use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::Rect;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
static GLOBAL_ESC_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

pub async fn handle_event(event: CrosstermEvent, app: &mut App) {
    if handle_resize_event(&event, app) {
        return;
    }

    if should_quit_on_ctrl_c(&event, app) {
        return;
    }

    if should_debounce_escape(&event) {
        return;
    }

    if matches!(app.app_state.mode, AppMode::FileBrowser) {
        browser::handle_event(event, app).await;
        app.app_state.ui.needs_redraw = true;
        return;
    }

    dispatch_mode_event(event, app).await;
    app.app_state.ui.needs_redraw = true;
}

fn should_quit_on_ctrl_c(event: &CrosstermEvent, app: &mut App) -> bool {
    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            app.app_state.should_quit = true;
            app.app_state.ui.needs_redraw = true;
            return true;
        }
    }
    false
}

fn handle_resize_event(event: &CrosstermEvent, app: &mut App) -> bool {
    if let CrosstermEvent::Resize(w, h) = event {
        app.app_state.screen_area = Rect::new(0, 0, *w, *h);
        app.app_state.ui.needs_redraw = true;
        return true;
    }
    false
}

fn should_debounce_escape(event: &CrosstermEvent) -> bool {
    if let CrosstermEvent::Key(key) = event {
        if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let last = GLOBAL_ESC_TIMESTAMP.load(Ordering::Relaxed);
            if now.saturating_sub(last) < 200 {
                return true;
            }

            GLOBAL_ESC_TIMESTAMP.store(now, Ordering::Relaxed);
        }
    }
    false
}

async fn dispatch_mode_event(event: CrosstermEvent, app: &mut App) {
    match app.app_state.mode {
        AppMode::Help => {
            help::handle_event(event, &mut app.app_state);
        }
        AppMode::Welcome => {
            welcome::handle_event(event, &mut app.app_state);
        }
        AppMode::Normal => normal::handle_event(event, app).await,
        AppMode::PowerSaving => power::handle_event(event, &mut app.app_state),
        AppMode::Config => {
            config::handle_event(
                event,
                config::ConfigHandleContext {
                    mode: &mut app.app_state.mode,
                    settings_edit: &mut app.app_state.ui.config.settings_edit,
                    selected_index: &mut app.app_state.ui.config.selected_index,
                    items: app.app_state.ui.config.items.as_mut_slice(),
                    editing: &mut app.app_state.ui.config.editing,
                    app_command_tx: &app.app_command_tx,
                    global_dl_bucket: &app.global_dl_bucket,
                    global_ul_bucket: &app.global_ul_bucket,
                },
            );
        }
        AppMode::DeleteConfirm => {
            let _ = delete_confirm::handle_event(event, app);
        }
        AppMode::Rss => {
            rss::handle_event(
                event,
                &mut app.app_state,
                &app.client_configs,
                &app.app_command_tx,
            );
        }
        AppMode::FileBrowser => {}
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
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        app_state.ui.selected_torrent_index = 0;
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        normal::handle_navigation(&mut app_state, KeyCode::Down);

        assert_eq!(app_state.ui.selected_torrent_index, 1);
        assert_eq!(app_state.ui.selected_peer_index, 0); // Should reset
    }

    #[test]
    fn test_nav_up_torrents() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 1;
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        normal::handle_navigation(&mut app_state, KeyCode::Up);

        assert_eq!(app_state.ui.selected_torrent_index, 0);
        assert_eq!(app_state.ui.selected_peer_index, 0); // Should reset
    }

    #[test]
    fn test_nav_down_peers() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.ui.selected_peer_index = 0;
        app_state.ui.selected_header = SelectedHeader::Peer(0);

        normal::handle_navigation(&mut app_state, KeyCode::Down);

        assert_eq!(app_state.ui.selected_torrent_index, 0); // Stays on same torrent
        assert_eq!(app_state.ui.selected_peer_index, 1); // Moves down peer list
    }

    #[test]
    fn test_nav_right_to_peers_when_peers_exist() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // "hash_a" has peers
        app_state.ui.selected_header = SelectedHeader::Torrent(99);

        normal::handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.ui.selected_header, SelectedHeader::Peer(0));
    }

    #[test]
    fn test_nav_right_to_peers_when_no_peers() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 1; // "hash_b" has 0 peers
        app_state.ui.selected_header = SelectedHeader::Torrent(99);

        normal::handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.ui.selected_header, SelectedHeader::Torrent(0));
    }

    #[test]
    fn test_nav_left_from_peers() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0;
        app_state.ui.selected_header = SelectedHeader::Peer(0);

        normal::handle_navigation(&mut app_state, KeyCode::Left);

        assert_eq!(app_state.ui.selected_header, SelectedHeader::Torrent(0));
    }

    #[test]
    fn test_nav_up_peers() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.ui.selected_peer_index = 1;
        app_state.ui.selected_header = SelectedHeader::Peer(0);

        normal::handle_navigation(&mut app_state, KeyCode::Up);

        assert_eq!(app_state.ui.selected_torrent_index, 0); // Stays on same torrent
        assert_eq!(app_state.ui.selected_peer_index, 0); // Moves up peer list
    }

    #[test]
    fn test_nav_up_at_top_of_list() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // At the top
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        normal::handle_navigation(&mut app_state, KeyCode::Up);

        // Should stay at 0, thanks to saturating_sub
        assert_eq!(app_state.ui.selected_torrent_index, 0);
    }

    #[test]
    fn test_nav_down_at_bottom_of_list() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 1; // At the bottom (index 1 of 2)
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        normal::handle_navigation(&mut app_state, KeyCode::Down);

        // Should stay at 1, as it's the last index
        assert_eq!(app_state.ui.selected_torrent_index, 1);
    }

    #[test]
    fn test_nav_up_peers_at_top_of_list() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.ui.selected_peer_index = 0; // At the top
        app_state.ui.selected_header = SelectedHeader::Peer(0);

        normal::handle_navigation(&mut app_state, KeyCode::Up);

        // Should stay at 0
        assert_eq!(app_state.ui.selected_peer_index, 0);
    }

    #[test]
    fn test_nav_down_peers_at_bottom_of_list() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0; // "hash_a" has 2 peers
        app_state.ui.selected_peer_index = 1; // At the bottom (index 1 of 2)
        app_state.ui.selected_header = SelectedHeader::Peer(0);

        normal::handle_navigation(&mut app_state, KeyCode::Down);

        // Should stay at 1
        assert_eq!(app_state.ui.selected_peer_index, 1);
    }

    #[test]
    fn test_nav_right_jumps_to_peers_when_only_name_column_visible() {
        let mut app_state = create_test_app_state();
        app_state.ui.selected_torrent_index = 0;
        app_state.ui.selected_header = SelectedHeader::Torrent(0);

        if let Some(torrent) = app_state.torrents.get_mut("hash_a".as_bytes()) {
            torrent.latest_state.activity_message = "Seeding".to_string();
            torrent.latest_state.number_of_pieces_total = 100;
            torrent.latest_state.number_of_pieces_completed = 100;
        }

        for torrent in app_state.torrents.values_mut() {
            torrent.smoothed_download_speed_bps = 0;
            torrent.smoothed_upload_speed_bps = 0;
        }

        normal::handle_navigation(&mut app_state, KeyCode::Right);

        assert_eq!(app_state.ui.selected_header, SelectedHeader::Peer(0));
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

        let changed = browser::apply_priority_cycle(&mut nodes, &PathBuf::from("root"));

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

        let changed = browser::apply_priority_cycle(&mut nodes, &PathBuf::from("missing"));

        assert!(!changed);
        assert_eq!(nodes[0].payload.priority, FilePriority::Normal);
    }

    #[test]
    fn test_escape_debounce_ignores_non_escape_keys() {
        GLOBAL_ESC_TIMESTAMP.store(0, Ordering::Relaxed);
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!should_debounce_escape(&event));
    }

    #[test]
    fn test_escape_debounce_blocks_rapid_second_escape() {
        GLOBAL_ESC_TIMESTAMP.store(0, Ordering::Relaxed);
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!should_debounce_escape(&event));
        assert!(should_debounce_escape(&event));
    }
}
