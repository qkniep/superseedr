// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::FilePriority;
use crate::config::{Settings, TorrentSettings};
use crate::integrations::control::{ControlPriorityTarget, ControlRequest};
use crate::persistence::event_journal::{ControlOrigin, EventDetails};
use crate::torrent_file::parser::from_bytes;
use crate::torrent_identity::{decode_info_hash, info_hash_from_torrent_source};
use std::fs;

pub fn find_torrent_settings_index_by_info_hash(
    settings: &Settings,
    info_hash: &[u8],
) -> Option<usize> {
    settings.torrents.iter().position(|torrent| {
        info_hash_from_torrent_source(&torrent.torrent_or_magnet).as_deref() == Some(info_hash)
    })
}

pub fn describe_priority_target(target: &ControlPriorityTarget) -> String {
    match target {
        ControlPriorityTarget::FileIndex(index) => format!("index {}", index),
        ControlPriorityTarget::FilePath(path) => format!("path {}", path),
    }
}

pub fn online_control_success_message(request: &ControlRequest) -> String {
    match request {
        ControlRequest::Pause { info_hash_hex } => {
            format!("Queued pause request for torrent '{}'", info_hash_hex)
        }
        ControlRequest::Resume { info_hash_hex } => {
            format!("Queued resume request for torrent '{}'", info_hash_hex)
        }
        ControlRequest::Delete { info_hash_hex } => {
            format!(
                "Queued delete request for torrent '{}' (files deleted: no)",
                info_hash_hex
            )
        }
        ControlRequest::SetFilePriority {
            info_hash_hex,
            target,
            priority,
        } => format!(
            "Queued file priority request for torrent '{}' ({}) -> {:?}",
            info_hash_hex,
            describe_priority_target(target),
            priority
        ),
        ControlRequest::StatusNow
        | ControlRequest::StatusFollowStart { .. }
        | ControlRequest::StatusFollowStop => "Queued control request.".to_string(),
    }
}

pub fn control_event_details(request: &ControlRequest, origin: ControlOrigin) -> EventDetails {
    let (file_index, file_path) = match request.priority_target() {
        Some(ControlPriorityTarget::FileIndex(index)) => (Some(*index), None),
        Some(ControlPriorityTarget::FilePath(path)) => (None, Some(path.clone())),
        None => (None, None),
    };

    EventDetails::Control {
        origin,
        action: request.action_name().to_string(),
        target_info_hash_hex: request.target_info_hash_hex().map(str::to_string),
        file_index,
        file_path,
        priority: request
            .priority_value()
            .map(|priority| format!("{:?}", priority)),
    }
}

pub fn load_torrent_file_list_for_settings(
    torrent_settings: &TorrentSettings,
) -> Result<Vec<(Vec<String>, u64)>, String> {
    if torrent_settings.torrent_or_magnet.starts_with("magnet:") {
        return Err(
            "This torrent does not have a persisted .torrent source for file path lookup"
                .to_string(),
        );
    }

    let bytes = fs::read(&torrent_settings.torrent_or_magnet).map_err(|error| {
        format!(
            "Failed to read torrent metadata from '{}': {}",
            torrent_settings.torrent_or_magnet, error
        )
    })?;
    let torrent = from_bytes(&bytes).map_err(|error| {
        format!(
            "Failed to parse torrent metadata from '{}': {:?}",
            torrent_settings.torrent_or_magnet, error
        )
    })?;
    Ok(torrent.file_list())
}

pub fn resolve_priority_file_index(
    torrent_settings: &TorrentSettings,
    target: &ControlPriorityTarget,
) -> Result<usize, String> {
    let file_list = load_torrent_file_list_for_settings(torrent_settings)?;
    match target {
        ControlPriorityTarget::FileIndex(index) => {
            if *index < file_list.len() {
                Ok(*index)
            } else {
                Err(format!(
                    "File index {} is out of range for torrent '{}' ({} files)",
                    index,
                    torrent_settings.name,
                    file_list.len()
                ))
            }
        }
        ControlPriorityTarget::FilePath(path) => {
            let normalized_target = path.replace('\\', "/");
            file_list
                .into_iter()
                .enumerate()
                .find_map(|(index, (parts, _))| {
                    (parts.join("/") == normalized_target).then_some(index)
                })
                .ok_or_else(|| {
                    format!(
                        "No file matching '{}' was found in torrent '{}'",
                        path, torrent_settings.name
                    )
                })
        }
    }
}

pub fn apply_offline_control_request(
    settings: &mut Settings,
    request: &ControlRequest,
) -> Result<String, String> {
    match request {
        ControlRequest::Pause { info_hash_hex } => {
            let info_hash = decode_info_hash(info_hash_hex)?;
            let Some(index) = find_torrent_settings_index_by_info_hash(settings, &info_hash) else {
                return Err(format!("Torrent '{}' was not found", info_hash_hex));
            };
            settings.torrents[index].torrent_control_state =
                crate::app::TorrentControlState::Paused;
            Ok(format!("Paused torrent '{}'", info_hash_hex))
        }
        ControlRequest::Resume { info_hash_hex } => {
            let info_hash = decode_info_hash(info_hash_hex)?;
            let Some(index) = find_torrent_settings_index_by_info_hash(settings, &info_hash) else {
                return Err(format!("Torrent '{}' was not found", info_hash_hex));
            };
            settings.torrents[index].torrent_control_state =
                crate::app::TorrentControlState::Running;
            Ok(format!("Resumed torrent '{}'", info_hash_hex))
        }
        ControlRequest::Delete { info_hash_hex } => {
            let info_hash = decode_info_hash(info_hash_hex)?;
            let initial_len = settings.torrents.len();
            settings.torrents.retain(|torrent| {
                info_hash_from_torrent_source(&torrent.torrent_or_magnet).as_deref()
                    != Some(info_hash.as_slice())
            });
            if settings.torrents.len() == initial_len {
                return Err(format!("Torrent '{}' was not found", info_hash_hex));
            }
            Ok(format!(
                "Removed torrent '{}' (files deleted: no)",
                info_hash_hex
            ))
        }
        ControlRequest::SetFilePriority {
            info_hash_hex,
            target,
            priority,
        } => {
            let info_hash = decode_info_hash(info_hash_hex)?;
            let Some(index) = find_torrent_settings_index_by_info_hash(settings, &info_hash) else {
                return Err(format!("Torrent '{}' was not found", info_hash_hex));
            };
            let file_index = resolve_priority_file_index(&settings.torrents[index], target)?;
            if matches!(priority, FilePriority::Normal) {
                settings.torrents[index].file_priorities.remove(&file_index);
            } else {
                settings.torrents[index]
                    .file_priorities
                    .insert(file_index, *priority);
            }
            Ok(format!(
                "Set file priority for torrent '{}' at index {} to {:?}",
                info_hash_hex, file_index, priority
            ))
        }
        ControlRequest::StatusNow
        | ControlRequest::StatusFollowStart { .. }
        | ControlRequest::StatusFollowStop => {
            Err("Status commands require a running superseedr instance".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_offline_control_request, find_torrent_settings_index_by_info_hash};
    use crate::config::{Settings, TorrentSettings};
    use crate::integrations::control::{ControlPriorityTarget, ControlRequest};

    #[test]
    fn offline_hybrid_magnet_lookup_prefers_btih_identity() {
        let magnet = concat!(
            "magnet:?xt=urn:btih:1111111111111111111111111111111111111111",
            "&xt=urn:btmh:1220aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        let settings = Settings {
            torrents: vec![TorrentSettings {
                torrent_or_magnet: magnet.to_string(),
                name: "Sample Hybrid".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            find_torrent_settings_index_by_info_hash(&settings, &[0x11; 20]),
            Some(0)
        );
    }

    #[test]
    fn offline_delete_targets_hybrid_magnet_by_btih() {
        let magnet = concat!(
            "magnet:?xt=urn:btih:1111111111111111111111111111111111111111",
            "&xt=urn:btmh:1220aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        let mut settings = Settings {
            torrents: vec![TorrentSettings {
                torrent_or_magnet: magnet.to_string(),
                name: "Sample Hybrid".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let result = apply_offline_control_request(
            &mut settings,
            &ControlRequest::Delete {
                info_hash_hex: "1111111111111111111111111111111111111111".to_string(),
            },
        );

        assert!(result.is_ok());
        assert!(settings.torrents.is_empty());
    }

    #[test]
    fn priority_file_path_resolution_still_requires_torrent_metadata() {
        let mut settings = Settings {
            torrents: vec![TorrentSettings {
                torrent_or_magnet: "magnet:?xt=urn:btih:1111111111111111111111111111111111111111"
                    .to_string(),
                name: "Magnet".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let result = apply_offline_control_request(
            &mut settings,
            &ControlRequest::SetFilePriority {
                info_hash_hex: "1111111111111111111111111111111111111111".to_string(),
                target: ControlPriorityTarget::FilePath("folder/item.bin".to_string()),
                priority: crate::app::FilePriority::High,
            },
        );

        assert!(result.is_err());
    }
}
