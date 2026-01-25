// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Serialize;
use std::collections::HashMap;
use crate::config::Settings;
use crate::app::TorrentMetrics;

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
