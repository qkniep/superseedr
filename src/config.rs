// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use figment::providers::{Env, Format};
use figment::{providers::Toml, Figment};

use tracing::{event as tracing_event, Level};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::app::FilePriority;
use crate::app::TorrentControlState;

use strum_macros::EnumCount;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, EnumIter, EnumCount)]
pub enum TorrentSortColumn {
    Name,
    #[default]
    Up,
    Down,
    Progress,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default, EnumIter, EnumCount)]
pub enum PeerSortColumn {
    Flags,
    Completed,
    Address,
    Client,
    Action,

    #[default]
    #[serde(alias = "TotalUL")]
    UL,

    #[serde(alias = "TotalDL")]
    DL,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub client_id: String,
    pub client_port: u16,
    pub torrents: Vec<TorrentSettings>,
    pub lifetime_downloaded: u64,
    pub lifetime_uploaded: u64,

    pub private_client: bool,

    // UI
    pub torrent_sort_column: TorrentSortColumn,
    pub torrent_sort_direction: SortDirection,
    pub peer_sort_column: PeerSortColumn,
    pub peer_sort_direction: SortDirection,

    // Disk
    pub watch_folder: Option<PathBuf>,
    pub default_download_folder: Option<PathBuf>,

    // Networking
    pub max_connected_peers: usize,
    pub bootstrap_nodes: Vec<String>,
    pub global_download_limit_bps: u64,
    pub global_upload_limit_bps: u64,

    // Performance
    pub max_concurrent_validations: usize,
    pub connection_attempt_permits: usize,
    pub resource_limit_override: Option<usize>,

    // Throttling / Choking
    pub upload_slots: usize,
    pub peer_upload_in_flight_limit: usize,

    // Timings
    pub tracker_fallback_interval_secs: u64,
    pub client_leeching_fallback_interval_secs: u64,
    pub output_status_interval: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_port: 6681,
            torrents: Vec::new(),
            watch_folder: None,
            default_download_folder: None,
            lifetime_downloaded: 0,
            lifetime_uploaded: 0,
            private_client: false,
            global_download_limit_bps: 0,
            global_upload_limit_bps: 0,
            torrent_sort_column: TorrentSortColumn::default(),
            torrent_sort_direction: SortDirection::default(),
            peer_sort_column: PeerSortColumn::default(),
            peer_sort_direction: SortDirection::default(),
            max_connected_peers: 2000,
            bootstrap_nodes: vec![
                "router.utorrent.com:6881".to_string(),
                "router.bittorrent.com:6881".to_string(),
                "dht.transmissionbt.com:6881".to_string(),
                "dht.libtorrent.org:25401".to_string(),
                "router.cococorp.de:6881".to_string(),
            ],
            max_concurrent_validations: 64,
            resource_limit_override: None,
            connection_attempt_permits: 50,
            upload_slots: 8,
            peer_upload_in_flight_limit: 4,
            tracker_fallback_interval_secs: 1800,
            client_leeching_fallback_interval_secs: 60,
            output_status_interval: 5,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
#[serde(default)]
pub struct TorrentSettings {
    pub torrent_or_magnet: String,
    pub name: String,
    pub validation_status: bool,
    pub download_path: Option<PathBuf>,
    pub container_name: Option<String>,
    pub torrent_control_state: TorrentControlState,

    #[serde(with = "string_usize_map")]
    pub file_priorities: HashMap<usize, FilePriority>,
}

mod string_usize_map {
    use crate::app::FilePriority;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::collections::HashMap;
    use std::str::FromStr;

    pub fn serialize<S>(
        map: &HashMap<usize, FilePriority>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 1. Convert usize keys to Strings for TOML compatibility
        let string_map: HashMap<String, FilePriority> =
            map.iter().map(|(k, v)| (k.to_string(), *v)).collect();

        // 2. Simply serialize the new map.
        // Do NOT call serializer.serialize_map() manually before this.
        serde::Serialize::serialize(&string_map, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<usize, FilePriority>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 1. Load the TOML map as Strings first
        let string_map: HashMap<String, FilePriority> = HashMap::deserialize(deserializer)?;
        let mut result = HashMap::new();

        // 2. Convert strings back to usize
        for (k, v) in string_map {
            let k_usize = usize::from_str(&k).map_err(serde::de::Error::custom)?;
            result.insert(k_usize, v);
        }
        Ok(result)
    }
}

/// This is now the single source of truth for app directories.
pub fn get_app_paths() -> Option<(PathBuf, PathBuf)> {
    if let Some(proj_dirs) = ProjectDirs::from("com", "github", "jagalite.superseedr") {
        let config_dir = proj_dirs.config_dir().to_path_buf();
        let data_dir = proj_dirs.data_local_dir().to_path_buf();

        // Ensure directories exist
        fs::create_dir_all(&config_dir).ok()?;
        fs::create_dir_all(&data_dir).ok()?;

        Some((config_dir, data_dir))
    } else {
        None
    }
}

pub fn get_watch_path() -> Option<(PathBuf, PathBuf)> {
    if let Some((_, base_path)) = get_app_paths() {
        let watch_path = base_path.join("watch_files");
        let processed_path = base_path.join("processed_files");
        Some((watch_path, processed_path))
    } else {
        None
    }
}

pub fn create_watch_directories() -> io::Result<()> {
    if let Some((watch_path, processed_path)) = get_watch_path() {
        fs::create_dir_all(&watch_path)?;
        fs::create_dir_all(&processed_path)?;
    }

    Ok(())
}

pub fn load_settings() -> Settings {
    if let Some((config_dir, _)) = get_app_paths() {
        let config_file_path = config_dir.join("settings.toml");

        match Figment::new()
            .merge(Toml::file(&config_file_path))
            .merge(Env::prefixed("SUPERSEEDR_"))
            .extract::<Settings>()
        {
            Ok(s) => return s,
            Err(e) => {
                tracing_event!(
                    Level::ERROR,
                    "Failed to load settings at {:?}: {}",
                    config_file_path,
                    e
                );
            }
        }
    }
    Settings::default()
}

pub fn save_settings(settings: &Settings) -> io::Result<()> {
    if let Some((config_dir, _)) = get_app_paths() {
        let config_file_path = config_dir.join("settings.toml");

        // 1. Create a backup directory
        let backup_dir = config_dir.join("backups_settings_files");
        fs::create_dir_all(&backup_dir)?;

        // 2. Create a timestamped filename (e.g., settings_20240112_1906.toml)
        let now = chrono::Local::now();
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let backup_path = backup_dir.join(format!("settings_{}.toml", timestamp));

        // 3. Serialize the current settings
        let content = toml::to_string_pretty(settings).map_err(io::Error::other)?;

        // 4. Write the main file and the backup
        let temp_file_path = config_dir.join("settings.toml.tmp");
        fs::write(&temp_file_path, &content)?;
        fs::rename(&temp_file_path, &config_file_path)?;

        // Only keep a reasonable number of backups (e.g., last 10) to save space
        fs::write(backup_path, content)?;
        cleanup_old_backups(&backup_dir, 64)?;
    }
    Ok(())
}

fn cleanup_old_backups(backup_dir: &PathBuf, limit: usize) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(backup_dir)?
        .filter_map(|res| res.ok())
        .map(|e| e.path())
        // Filter: Only include files that look like your backups
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("settings_") && s.ends_with(".toml"))
                .unwrap_or(false)
        })
        .collect();

    if entries.len() > limit {
        // Since names are timestamped, alphabetical sort is chronological sort
        entries.sort();
        for path in entries.iter().take(entries.len() - limit) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*; // Import everything from the parent module (your config.rs code)
    use figment::providers::{Format, Toml};
    use figment::Figment;
    use std::path::PathBuf;

    #[test]
    fn test_full_settings_parsing() {
        let toml_str = r#"
            client_id = "test-client-id-123"
            client_port = 12345
            lifetime_downloaded = 1000
            lifetime_uploaded = 2000

            torrent_sort_column = "Name"
            torrent_sort_direction = "Descending"
            peer_sort_column = "Address"
            peer_sort_direction = "Ascending"

            watch_folder = "/path/to/watch"
            default_download_folder = "/path/to/download"

            max_connected_peers = 500
            global_download_limit_bps = 102400
            global_upload_limit_bps = 51200

            max_concurrent_validations = 32
            connection_attempt_permits = 25
            resource_limit_override = 1024

            upload_slots = 10
            peer_upload_in_flight_limit = 2

            tracker_fallback_interval_secs = 3600
            client_leeching_fallback_interval_secs = 120

            bootstrap_nodes = [
                "node1.com:1234",
                "node2.com:5678"
            ]

            [[torrents]]
            torrent_or_magnet = "magnet:?xt=urn:btih:..."
            name = "My Test Torrent"
            validation_status = true
            download_path = "/downloads/my_test_torrent"
            # torrent_control_state is omitted, will use default (Stopped)

            [[torrents]]
            torrent_or_magnet = "magnet:?xt=urn:btih:other"
            name = "Another Torrent"
            validation_status = false
            download_path = "/downloads/another"
            torrent_control_state = "Paused"
        "#;

        // Parse the string using Figment, just like load_settings would
        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse full TOML string");

        // Assert values
        assert_eq!(settings.client_id, "test-client-id-123");
        assert_eq!(settings.client_port, 12345);
        assert_eq!(settings.lifetime_downloaded, 1000);
        assert_eq!(settings.global_upload_limit_bps, 51200);
        assert_eq!(settings.torrent_sort_column, TorrentSortColumn::Name);
        assert_eq!(settings.torrent_sort_direction, SortDirection::Descending);
        assert_eq!(settings.peer_sort_column, PeerSortColumn::Address);
        assert_eq!(settings.watch_folder, Some(PathBuf::from("/path/to/watch")));
        assert_eq!(settings.resource_limit_override, Some(1024));
        assert_eq!(
            settings.bootstrap_nodes,
            vec!["node1.com:1234", "node2.com:5678"]
        );

        // Assert torrents
        assert_eq!(settings.torrents.len(), 2);
        assert_eq!(settings.torrents[0].name, "My Test Torrent");
        assert!(settings.torrents[0].validation_status);
        assert_eq!(
            settings.torrents[0].download_path,
            Some(PathBuf::from("/downloads/my_test_torrent"))
        );
        // Check that omitting the field used the default
        assert_eq!(settings.torrents[1].name, "Another Torrent");
        assert_eq!(
            settings.torrents[1].torrent_control_state,
            TorrentControlState::Paused
        );
    }

    #[test]
    fn test_partial_settings_override() {
        let toml_str = r#"
            # Only override a few values
            client_port = 9999
            global_upload_limit_bps = 50000

            [[torrents]]
            name = "Partial Torrent"
            download_path = "/partial/path"
            # Other fields like torrent_or_magnet will be default (empty string)
        "#;

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse partial TOML string");

        let default_settings = Settings::default();

        // Assert changed values
        assert_eq!(settings.client_port, 9999);
        assert_eq!(settings.global_upload_limit_bps, 50000);

        // Assert unchanged (default) values
        assert_eq!(settings.client_id, default_settings.client_id); // ""
        assert_eq!(
            settings.max_connected_peers,
            default_settings.max_connected_peers
        ); // 2000
        assert_eq!(
            settings.torrent_sort_column,
            default_settings.torrent_sort_column
        ); // Up

        // Assert partial torrent
        assert_eq!(settings.torrents.len(), 1);
        assert_eq!(settings.torrents[0].name, "Partial Torrent");
        assert_eq!(
            settings.torrents[0].download_path,
            Some(PathBuf::from("/partial/path"))
        );
        assert_eq!(settings.torrents[0].torrent_or_magnet, ""); // Default for String
        assert!(!settings.torrents[0].validation_status); // Default for bool
        assert_eq!(
            settings.torrents[0].torrent_control_state,
            TorrentControlState::default()
        );
    }

    #[test]
    fn test_default_settings() {
        // An empty string should result in all default values
        let toml_str = "";

        let settings: Settings = Figment::new()
            .merge(Toml::string(toml_str))
            .extract()
            .expect("Failed to parse empty string");

        let default_settings = Settings::default();

        // Assert a few key default values
        assert_eq!(settings.client_id, default_settings.client_id);
        assert_eq!(settings.client_port, 6681);
        assert_eq!(settings.lifetime_downloaded, 0);
        assert_eq!(settings.global_upload_limit_bps, 0);
        assert_eq!(settings.torrent_sort_column, TorrentSortColumn::Up);
        assert_eq!(settings.peer_sort_direction, SortDirection::Ascending);
        assert!(settings.watch_folder.is_none());
        assert_eq!(settings.max_connected_peers, 2000);
        assert_eq!(settings.bootstrap_nodes, default_settings.bootstrap_nodes);
        assert!(settings.torrents.is_empty());
    }

    #[test]
    fn test_invalid_torrent_state_parsing() {
        let toml_str = r#"
            [[torrents]]
            name = "Invalid Torrent"
            download_path = "/invalid/path"
            torrent_control_state = "UNKNOWN" # This is not a valid variant
        "#;

        // Try to parse the string
        let result: Result<Settings, figment::Error> =
            Figment::new().merge(Toml::string(toml_str)).extract();

        // We expect this to fail
        assert!(
            result.is_err(),
            "Parsing should fail with an invalid enum variant"
        );

        // Optional: Check if the error message contains the problematic variant
        // This makes the test more robust.
        if let Err(e) = result {
            let error_string = e.to_string();
            assert!(
                error_string.contains("UNKNOWN"),
                "Error message should mention the invalid variant 'UNKNOWN'"
            );
            assert!(
                error_string.contains("torrent_control_state"),
                "Error message should mention the field 'torrent_control_state'"
            );
        }
    }
}
