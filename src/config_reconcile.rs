// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::FilePriority;
use crate::config::{configured_watch_paths, Settings, TorrentSettings};
use crate::torrent_identity::info_hash_from_torrent_source;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveTorrentRuntimeConfig {
    pub torrent_data_path: PathBuf,
    pub file_priorities: HashMap<usize, FilePriority>,
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchPathChanges {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TorrentConfigChange {
    Added {
        info_hash: Vec<u8>,
        current: TorrentSettings,
    },
    Removed {
        info_hash: Vec<u8>,
        previous: TorrentSettings,
    },
    Updated {
        info_hash: Vec<u8>,
        previous: TorrentSettings,
        current: TorrentSettings,
        runtime_config: Option<EffectiveTorrentRuntimeConfig>,
        control_state_changed: bool,
    },
}

pub fn effective_torrent_runtime_config(
    settings: &Settings,
    torrent: &TorrentSettings,
) -> Option<EffectiveTorrentRuntimeConfig> {
    let torrent_data_path = torrent
        .download_path
        .clone()
        .or_else(|| settings.default_download_folder.clone())?;

    Some(EffectiveTorrentRuntimeConfig {
        torrent_data_path,
        file_priorities: torrent.file_priorities.clone(),
        container_name: torrent.container_name.clone(),
    })
}

pub fn configured_watch_path_changes(
    old_settings: &Settings,
    new_settings: &Settings,
) -> WatchPathChanges {
    let old_paths = configured_watch_paths(old_settings);
    let new_paths = configured_watch_paths(new_settings);

    let removed = old_paths
        .iter()
        .filter(|old_path| !new_paths.iter().any(|new_path| new_path == *old_path))
        .cloned()
        .collect();
    let added = new_paths
        .iter()
        .filter(|new_path| !old_paths.iter().any(|old_path| old_path == *new_path))
        .cloned()
        .collect();

    WatchPathChanges { added, removed }
}

fn torrents_by_info_hash(settings: &Settings) -> HashMap<Vec<u8>, TorrentSettings> {
    settings
        .torrents
        .iter()
        .filter_map(|torrent| {
            info_hash_from_torrent_source(&torrent.torrent_or_magnet)
                .map(|info_hash| (info_hash, torrent.clone()))
        })
        .collect()
}

pub fn diff_torrent_configs(
    old_settings: &Settings,
    new_settings: &Settings,
) -> Vec<TorrentConfigChange> {
    let old_by_hash = torrents_by_info_hash(old_settings);
    let new_by_hash = torrents_by_info_hash(new_settings);
    let mut changes = Vec::new();

    for (info_hash, previous) in &old_by_hash {
        if !new_by_hash.contains_key(info_hash) {
            changes.push(TorrentConfigChange::Removed {
                info_hash: info_hash.clone(),
                previous: previous.clone(),
            });
        }
    }

    for (info_hash, current) in &new_by_hash {
        let Some(previous) = old_by_hash.get(info_hash) else {
            changes.push(TorrentConfigChange::Added {
                info_hash: info_hash.clone(),
                current: current.clone(),
            });
            continue;
        };

        let runtime_settings_changed = previous.download_path != current.download_path
            || previous.file_priorities != current.file_priorities
            || previous.container_name != current.container_name;
        let control_state_changed = previous.torrent_control_state != current.torrent_control_state;

        if runtime_settings_changed || control_state_changed || previous != current {
            changes.push(TorrentConfigChange::Updated {
                info_hash: info_hash.clone(),
                previous: previous.clone(),
                current: current.clone(),
                runtime_config: runtime_settings_changed
                    .then(|| effective_torrent_runtime_config(new_settings, current))
                    .flatten(),
                control_state_changed,
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::{
        configured_watch_path_changes, diff_torrent_configs, effective_torrent_runtime_config,
        TorrentConfigChange,
    };
    use crate::app::{FilePriority, TorrentControlState};
    use crate::config::{Settings, TorrentSettings};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn effective_runtime_config_uses_default_download_folder_when_torrent_override_is_missing() {
        let settings = Settings {
            default_download_folder: Some(PathBuf::from("/downloads")),
            ..Default::default()
        };
        let torrent = TorrentSettings {
            file_priorities: HashMap::from([(1, FilePriority::High)]),
            container_name: Some("sample".to_string()),
            ..Default::default()
        };

        let config = effective_torrent_runtime_config(&settings, &torrent).expect("runtime config");
        assert_eq!(config.torrent_data_path, PathBuf::from("/downloads"));
        assert_eq!(config.file_priorities.get(&1), Some(&FilePriority::High));
        assert_eq!(config.container_name.as_deref(), Some("sample"));
    }

    #[test]
    fn diff_torrent_configs_materializes_runtime_update_for_default_folder_torrent() {
        let old_settings = Settings {
            default_download_folder: Some(PathBuf::from("/downloads")),
            torrents: vec![TorrentSettings {
                torrent_or_magnet: "magnet:?xt=urn:btih:1111111111111111111111111111111111111111"
                    .to_string(),
                name: "Sample".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let new_settings = Settings {
            default_download_folder: Some(PathBuf::from("/downloads")),
            torrents: vec![TorrentSettings {
                torrent_or_magnet: "magnet:?xt=urn:btih:1111111111111111111111111111111111111111"
                    .to_string(),
                name: "Sample".to_string(),
                file_priorities: HashMap::from([(2, FilePriority::Skip)]),
                container_name: Some("sample".to_string()),
                torrent_control_state: TorrentControlState::Paused,
                ..Default::default()
            }],
            ..Default::default()
        };

        let changes = diff_torrent_configs(&old_settings, &new_settings);
        assert_eq!(changes.len(), 1);
        match &changes[0] {
            TorrentConfigChange::Updated {
                runtime_config,
                control_state_changed,
                ..
            } => {
                let runtime_config = runtime_config.as_ref().expect("runtime config");
                assert_eq!(
                    runtime_config.torrent_data_path,
                    PathBuf::from("/downloads")
                );
                assert_eq!(
                    runtime_config.file_priorities.get(&2),
                    Some(&FilePriority::Skip)
                );
                assert_eq!(runtime_config.container_name.as_deref(), Some("sample"));
                assert!(*control_state_changed);
            }
            other => panic!("expected updated change, got {other:?}"),
        }
    }

    #[test]
    fn configured_watch_path_changes_tracks_additions_and_removals() {
        let old_settings = Settings {
            watch_folder: Some(PathBuf::from("/watch-a")),
            ..Default::default()
        };
        let new_settings = Settings {
            watch_folder: Some(PathBuf::from("/watch-b")),
            ..Default::default()
        };

        let changes = configured_watch_path_changes(&old_settings, &new_settings);
        assert!(changes.removed.contains(&PathBuf::from("/watch-a")));
        assert!(changes.added.contains(&PathBuf::from("/watch-b")));
    }
}
