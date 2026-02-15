// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::{App, TorrentControlState};
use crate::torrent_manager::ManagerCommand;
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode};

pub fn handle_event(event: CrosstermEvent, app: &mut App, info_hash: Vec<u8>, with_files: bool) -> bool {
    if let CrosstermEvent::Key(key) = event {
        match key.code {
            KeyCode::Enter => {
                let command = if with_files {
                    ManagerCommand::DeleteFile
                } else {
                    ManagerCommand::Shutdown
                };
                if let Some(manager_tx) = app.torrent_manager_command_txs.get(&info_hash) {
                    let manager_tx_clone = manager_tx.clone();
                    tokio::spawn(async move {
                        let _ = manager_tx_clone.send(command).await;
                    });
                }
                if let Some(torrent) = app.app_state.torrents.get_mut(&info_hash) {
                    torrent.latest_state.torrent_control_state = TorrentControlState::Deleting;
                }
                return true;
            }
            KeyCode::Esc => return true,
            _ => {}
        }
    }

    false
}
