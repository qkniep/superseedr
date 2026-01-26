// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::TorrentMetrics;
use crate::config::Settings;
use serde::Serialize;
use std::collections::HashMap;

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
        map_ser.serialize_entry(&hex::encode(k), v)?;
    }
    map_ser.end()
}

pub fn dump(output_data: AppOutputState, shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let base_path = crate::config::get_app_paths()
        .map(|(_, data_dir)| data_dir)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    let file_path = base_path.join("status_files").join("app_state.json");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TorrentMetrics;
    use crate::config::Settings;
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

        // We removed the negative assertion (!json.contains("[170,187")) because
        // the 'metrics.info_hash' field inside the object is expected to be a byte array.
    }
}
