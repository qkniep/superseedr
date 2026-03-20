// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::TorrentMetrics;
use crate::config::Settings;
use serde::ser::SerializeStruct;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crate::torrent_identity::info_hash_from_torrent_source;

#[derive(Serialize)]
pub struct AppOutputState {
    pub run_time: u64,
    pub cpu_usage: f32,
    pub ram_usage_percent: f32,
    pub total_download_bps: u64,
    pub total_upload_bps: u64,
    #[serde(serialize_with = "serialize_torrents_hex")]
    pub torrents: HashMap<Vec<u8>, TorrentMetrics>,
    pub settings: Settings,
}

pub fn serialize_torrents_hex<S>(
    map: &HashMap<Vec<u8>, TorrentMetrics>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_ser = s.serialize_map(Some(map.len()))?;
    for (k, v) in map {
        map_ser.serialize_entry(&hex::encode(k), &StatusTorrentMetrics::new(v))?;
    }
    map_ser.end()
}

struct StatusTorrentMetrics<'a> {
    metrics: &'a TorrentMetrics,
}

impl<'a> StatusTorrentMetrics<'a> {
    fn new(metrics: &'a TorrentMetrics) -> Self {
        Self { metrics }
    }
}

impl Serialize for StatusTorrentMetrics<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("TorrentMetrics", 22)?;
        state.serialize_field("info_hash_hex", &hex::encode(&self.metrics.info_hash))?;
        state.serialize_field("torrent_control_state", &self.metrics.torrent_control_state)?;
        state.serialize_field("info_hash", &self.metrics.info_hash)?;
        state.serialize_field("torrent_or_magnet", &self.metrics.torrent_or_magnet)?;
        state.serialize_field("torrent_name", &self.metrics.torrent_name)?;
        state.serialize_field("download_path", &self.metrics.download_path)?;
        state.serialize_field("container_name", &self.metrics.container_name)?;
        state.serialize_field("is_multi_file", &self.metrics.is_multi_file)?;
        state.serialize_field("file_count", &self.metrics.file_count)?;
        state.serialize_field("file_priorities", &self.metrics.file_priorities)?;
        state.serialize_field("data_available", &self.metrics.data_available)?;
        state.serialize_field("is_complete", &self.metrics.is_complete)?;
        state.serialize_field(
            "number_of_successfully_connected_peers",
            &self.metrics.number_of_successfully_connected_peers,
        )?;
        state.serialize_field(
            "number_of_pieces_total",
            &self.metrics.number_of_pieces_total,
        )?;
        state.serialize_field(
            "number_of_pieces_completed",
            &self.metrics.number_of_pieces_completed,
        )?;
        state.serialize_field("download_speed_bps", &self.metrics.download_speed_bps)?;
        state.serialize_field("upload_speed_bps", &self.metrics.upload_speed_bps)?;
        state.serialize_field(
            "bytes_downloaded_this_tick",
            &self.metrics.bytes_downloaded_this_tick,
        )?;
        state.serialize_field(
            "bytes_uploaded_this_tick",
            &self.metrics.bytes_uploaded_this_tick,
        )?;
        state.serialize_field(
            "session_total_downloaded",
            &self.metrics.session_total_downloaded,
        )?;
        state.serialize_field(
            "session_total_uploaded",
            &self.metrics.session_total_uploaded,
        )?;
        state.serialize_field("eta", &self.metrics.eta)?;
        state.serialize_field("activity_message", &self.metrics.activity_message)?;
        state.serialize_field("next_announce_in", &self.metrics.next_announce_in)?;
        state.serialize_field("total_size", &self.metrics.total_size)?;
        state.serialize_field("bytes_written", &self.metrics.bytes_written)?;
        state.serialize_field("blocks_in_this_tick", &self.metrics.blocks_in_this_tick)?;
        state.serialize_field("blocks_out_this_tick", &self.metrics.blocks_out_this_tick)?;
        state.end()
    }
}

pub fn dump(output_data: AppOutputState, shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let file_path = status_file_path().unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("status_files")
            .join("app_state.json")
    });
    let mut shutdown_rx = shutdown_tx.subscribe();

    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::debug!("Status dump aborted due to application shutdown");
            }
            result = tokio::task::spawn_blocking(move || {
                if let Some(parent) = file_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                serde_json::to_string_pretty(&output_data)
                    .map(|json| std::fs::write(file_path, json))
            }) => {
                if let Ok(Err(e)) = result {
                    tracing::error!("Failed to write status dump: {:?}", e);
                }
            }
        }
    });
}

pub fn status_file_path() -> io::Result<PathBuf> {
    let base_path = crate::config::get_app_paths()
        .map(|(_, data_dir)| data_dir)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not resolve app data directory",
            )
        })?;
    Ok(base_path.join("status_files").join("app_state.json"))
}

pub fn offline_output_state(settings: &Settings) -> AppOutputState {
    let torrents = settings
        .torrents
        .iter()
        .filter_map(torrent_metrics_from_settings)
        .map(|metrics| (metrics.info_hash.clone(), metrics))
        .collect();

    AppOutputState {
        run_time: 0,
        cpu_usage: 0.0,
        ram_usage_percent: 0.0,
        total_download_bps: 0,
        total_upload_bps: 0,
        torrents,
        settings: settings.clone(),
    }
}

pub fn offline_output_json(settings: &Settings) -> io::Result<String> {
    serde_json::to_string_pretty(&offline_output_state(settings)).map_err(io::Error::other)
}

fn torrent_metrics_from_settings(
    torrent_settings: &crate::config::TorrentSettings,
) -> Option<TorrentMetrics> {
    let info_hash = info_hash_from_torrent_source(&torrent_settings.torrent_or_magnet)?;

    Some(TorrentMetrics {
        torrent_control_state: torrent_settings.torrent_control_state.clone(),
        info_hash,
        torrent_or_magnet: torrent_settings.torrent_or_magnet.clone(),
        torrent_name: torrent_settings.name.clone(),
        download_path: torrent_settings.download_path.clone(),
        container_name: torrent_settings.container_name.clone(),
        file_priorities: torrent_settings.file_priorities.clone(),
        is_complete: torrent_settings.validation_status,
        activity_message: "Offline settings snapshot".to_string(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TorrentMetrics;
    use crate::config::{Settings, TorrentSettings};
    use std::collections::HashMap;

    #[test]
    fn test_serialize_torrents_hex_keys() {
        let mut torrents = HashMap::new();

        // Create a fake info hash (5 bytes for simplicity)
        // 0xAA = 170, 0xBB = 187, etc.
        let info_hash = vec![0xAA, 0xBB, 0xCC, 0x12, 0x34];
        let info_hash_key = info_hash.clone();

        let metrics = TorrentMetrics {
            info_hash, // This field will still serialize to [170, 187, ...]
            torrent_name: "Test Torrent".to_string(),
            ..Default::default()
        };

        torrents.insert(info_hash_key, metrics);

        let settings = Settings {
            client_port: 8080,
            ..Default::default()
        };

        let output = AppOutputState {
            run_time: 100,
            cpu_usage: 5.5,
            ram_usage_percent: 10.0,
            total_download_bps: 1024,
            total_upload_bps: 512,
            torrents,
            settings,
        };

        let json = serde_json::to_string(&output).expect("Serialization failed");

        // The key in the JSON map MUST be the hex string "aabbcc1234"
        assert!(
            json.contains("\"aabbcc1234\":"),
            "JSON should contain hex-encoded key"
        );
        assert!(
            json.contains("\"info_hash_hex\":\"aabbcc1234\""),
            "JSON should contain info_hash_hex in the torrent payload"
        );

        // We removed the negative assertion (!json.contains("[170,187")) because
        // the 'metrics.info_hash' field inside the object is expected to be a byte array.
    }

    #[test]
    fn offline_output_json_builds_snapshot_from_settings() {
        let settings = Settings {
            torrents: vec![TorrentSettings {
                torrent_or_magnet: "magnet:?xt=urn:btih:1111111111111111111111111111111111111111"
                    .to_string(),
                name: "Sample Alpha".to_string(),
                validation_status: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        let json = offline_output_json(&settings).expect("serialize offline output");

        assert!(json.contains("\"settings\""));
        assert!(json.contains("\"1111111111111111111111111111111111111111\""));
        assert!(json.contains("Offline settings snapshot"));
    }
}
