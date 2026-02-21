// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::{get_app_paths, FeedSyncError, RssHistoryEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{event as tracing_event, Level};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RssPersistedState {
    pub history: Vec<RssHistoryEntry>,
    pub last_sync_at: Option<String>,
    pub feed_errors: HashMap<String, FeedSyncError>,
}

#[allow(dead_code)]
pub fn rss_state_file_path() -> io::Result<PathBuf> {
    let (_, data_dir) = get_app_paths().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not resolve app data directory for RSS persistence",
        )
    })?;

    Ok(data_dir.join("persistence").join("rss.toml"))
}

#[allow(dead_code)]
pub fn load_rss_state() -> RssPersistedState {
    match rss_state_file_path() {
        Ok(path) => load_rss_state_from_path(&path),
        Err(e) => {
            tracing_event!(
                Level::WARN,
                "Failed to get RSS persistence path. Using empty state: {}",
                e
            );
            RssPersistedState::default()
        }
    }
}

#[allow(dead_code)]
pub fn save_rss_state(state: &RssPersistedState) -> io::Result<()> {
    let path = rss_state_file_path()?;
    save_rss_state_to_path(state, &path)
}

fn load_rss_state_from_path(path: &Path) -> RssPersistedState {
    if !path.exists() {
        return RssPersistedState::default();
    }

    match fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<RssPersistedState>(&content) {
            Ok(state) => state,
            Err(e) => {
                tracing_event!(
                    Level::WARN,
                    "Failed to parse RSS persistence file {:?}. Resetting RSS state: {}",
                    path,
                    e
                );
                RssPersistedState::default()
            }
        },
        Err(e) => {
            tracing_event!(
                Level::WARN,
                "Failed to read RSS persistence file {:?}. Using empty state: {}",
                path,
                e
            );
            RssPersistedState::default()
        }
    }
}

fn save_rss_state_to_path(state: &RssPersistedState, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(state).map_err(io::Error::other)?;
    let tmp_path = path.with_extension("toml.tmp");

    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RssAddedVia;
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("rss.toml");

        let state = load_rss_state_from_path(&path);
        assert_eq!(state, RssPersistedState::default());
    }

    #[test]
    fn load_invalid_file_returns_default() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("rss.toml");
        fs::write(&path, "not = [valid").expect("write malformed toml");

        let state = load_rss_state_from_path(&path);
        assert_eq!(state, RssPersistedState::default());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("rss.toml");

        let mut feed_errors = HashMap::new();
        feed_errors.insert(
            "https://example.com/rss".to_string(),
            FeedSyncError {
                message: "timeout".to_string(),
                occurred_at_iso: "2026-02-17T12:00:00Z".to_string(),
            },
        );

        let state = RssPersistedState {
            history: vec![RssHistoryEntry {
                dedupe_key: "guid:123".to_string(),
                info_hash: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
                guid: Some("123".to_string()),
                link: Some("https://example.com/item.torrent".to_string()),
                title: "SampleAlpha ISO".to_string(),
                source: Some("Example Feed".to_string()),
                date_iso: "2026-02-17T10:00:00Z".to_string(),
                added_via: RssAddedVia::Manual,
            }],
            last_sync_at: Some("2026-02-17T12:00:00Z".to_string()),
            feed_errors,
        };

        save_rss_state_to_path(&state, &path).expect("save rss state");
        let loaded = load_rss_state_from_path(&path);

        assert_eq!(loaded, state);
    }
}
