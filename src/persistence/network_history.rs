// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::get_app_paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{event as tracing_event, Level};

pub const NETWORK_HISTORY_SCHEMA_VERSION: u32 = 1;
pub const SECOND_1S_CAP: usize = 60 * 60; // 1 hour
pub const MINUTE_1M_CAP: usize = 48 * 60; // 48 hours
pub const MINUTE_15M_CAP: usize = 30 * 24 * 4; // 30 days
pub const HOUR_1H_CAP: usize = 365 * 24; // 365 days

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct NetworkHistoryPoint {
    pub ts_unix: u64,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub backoff_ms_max: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct NetworkHistoryTiers {
    pub second_1s: Vec<NetworkHistoryPoint>,
    pub minute_1m: Vec<NetworkHistoryPoint>,
    pub minute_15m: Vec<NetworkHistoryPoint>,
    pub hour_1h: Vec<NetworkHistoryPoint>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkHistoryPersistedState {
    pub schema_version: u32,
    pub updated_at_unix: u64,
    pub tiers: NetworkHistoryTiers,
}

impl Default for NetworkHistoryPersistedState {
    fn default() -> Self {
        Self {
            schema_version: NETWORK_HISTORY_SCHEMA_VERSION,
            updated_at_unix: 0,
            tiers: NetworkHistoryTiers::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RollupAccumulator {
    count: u32,
    dl_sum: u128,
    ul_sum: u128,
    backoff_max: u64,
}

impl RollupAccumulator {
    fn push(&mut self, point: &NetworkHistoryPoint) {
        self.count += 1;
        self.dl_sum += point.download_bps as u128;
        self.ul_sum += point.upload_bps as u128;
        self.backoff_max = self.backoff_max.max(point.backoff_ms_max);
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default)]
pub struct NetworkHistoryRollupState {
    second_to_minute: RollupAccumulator,
    minute_to_15m: RollupAccumulator,
    m15_to_hour: RollupAccumulator,
}

impl NetworkHistoryRollupState {
    pub fn ingest_second_sample(
        &mut self,
        state: &mut NetworkHistoryPersistedState,
        ts_unix: u64,
        download_bps: u64,
        upload_bps: u64,
        backoff_ms_max: u64,
    ) -> bool {
        let second_point = NetworkHistoryPoint {
            ts_unix,
            download_bps,
            upload_bps,
            backoff_ms_max,
        };
        state.tiers.second_1s.push(second_point.clone());
        cap_vec(&mut state.tiers.second_1s, SECOND_1S_CAP);

        self.second_to_minute.push(&second_point);
        if self.second_to_minute.count >= 60 {
            let minute_point = make_rollup_point(&self.second_to_minute, ts_unix);
            self.second_to_minute.clear();

            state.tiers.minute_1m.push(minute_point.clone());
            cap_vec(&mut state.tiers.minute_1m, MINUTE_1M_CAP);

            self.minute_to_15m.push(&minute_point);
            if self.minute_to_15m.count >= 15 {
                let m15_point = make_rollup_point(&self.minute_to_15m, ts_unix);
                self.minute_to_15m.clear();

                state.tiers.minute_15m.push(m15_point.clone());
                cap_vec(&mut state.tiers.minute_15m, MINUTE_15M_CAP);

                self.m15_to_hour.push(&m15_point);
                if self.m15_to_hour.count >= 4 {
                    let hour_point = make_rollup_point(&self.m15_to_hour, ts_unix);
                    self.m15_to_hour.clear();

                    state.tiers.hour_1h.push(hour_point);
                    cap_vec(&mut state.tiers.hour_1h, HOUR_1H_CAP);
                }
            }
        }

        true
    }
}

fn make_rollup_point(acc: &RollupAccumulator, ts_unix: u64) -> NetworkHistoryPoint {
    if acc.count == 0 {
        return NetworkHistoryPoint {
            ts_unix,
            ..Default::default()
        };
    }
    NetworkHistoryPoint {
        ts_unix,
        download_bps: (acc.dl_sum / acc.count as u128) as u64,
        upload_bps: (acc.ul_sum / acc.count as u128) as u64,
        backoff_ms_max: acc.backoff_max,
    }
}

fn cap_vec<T>(vec: &mut Vec<T>, cap: usize) {
    if vec.len() > cap {
        let overflow = vec.len() - cap;
        vec.drain(0..overflow);
    }
}

pub fn enforce_retention_caps(state: &mut NetworkHistoryPersistedState) {
    cap_vec(&mut state.tiers.second_1s, SECOND_1S_CAP);
    cap_vec(&mut state.tiers.minute_1m, MINUTE_1M_CAP);
    cap_vec(&mut state.tiers.minute_15m, MINUTE_15M_CAP);
    cap_vec(&mut state.tiers.hour_1h, HOUR_1H_CAP);
}

#[allow(dead_code)]
pub fn network_history_state_file_path() -> io::Result<PathBuf> {
    let (_, data_dir) = get_app_paths().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not resolve app data directory for network history persistence",
        )
    })?;

    Ok(data_dir.join("persistence").join("network_history.toml"))
}

#[allow(dead_code)]
pub fn load_network_history_state() -> NetworkHistoryPersistedState {
    match network_history_state_file_path() {
        Ok(path) => load_network_history_state_from_path(&path),
        Err(e) => {
            tracing_event!(
                Level::WARN,
                "Failed to get network history persistence path. Using empty state: {}",
                e
            );
            NetworkHistoryPersistedState::default()
        }
    }
}

#[allow(dead_code)]
pub fn save_network_history_state(state: &NetworkHistoryPersistedState) -> io::Result<()> {
    let path = network_history_state_file_path()?;
    save_network_history_state_to_path(state, &path)
}

fn load_network_history_state_from_path(path: &Path) -> NetworkHistoryPersistedState {
    if !path.exists() {
        return NetworkHistoryPersistedState::default();
    }

    match fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<NetworkHistoryPersistedState>(&content) {
            Ok(mut state) => {
                enforce_retention_caps(&mut state);
                state
            }
            Err(e) => {
                tracing_event!(
                    Level::WARN,
                    "Failed to parse network history persistence file {:?}. Resetting state: {}",
                    path,
                    e
                );
                NetworkHistoryPersistedState::default()
            }
        },
        Err(e) => {
            tracing_event!(
                Level::WARN,
                "Failed to read network history persistence file {:?}. Using empty state: {}",
                path,
                e
            );
            NetworkHistoryPersistedState::default()
        }
    }
}

fn save_network_history_state_to_path(
    state: &NetworkHistoryPersistedState,
    path: &Path,
) -> io::Result<()> {
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
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("network_history.toml");

        let state = load_network_history_state_from_path(&path);
        assert_eq!(state, NetworkHistoryPersistedState::default());
    }

    #[test]
    fn load_invalid_file_returns_default() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("network_history.toml");
        fs::write(&path, "not = [valid").expect("write malformed toml");

        let state = load_network_history_state_from_path(&path);
        assert_eq!(state, NetworkHistoryPersistedState::default());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("network_history.toml");

        let state = NetworkHistoryPersistedState {
            schema_version: NETWORK_HISTORY_SCHEMA_VERSION,
            updated_at_unix: 1_771_860_000,
            tiers: NetworkHistoryTiers {
                second_1s: vec![NetworkHistoryPoint {
                    ts_unix: 1_771_860_000,
                    download_bps: 1024,
                    upload_bps: 256,
                    backoff_ms_max: 0,
                }],
                minute_1m: vec![],
                minute_15m: vec![],
                hour_1h: vec![],
            },
        };

        save_network_history_state_to_path(&state, &path).expect("save network history state");
        let loaded = load_network_history_state_from_path(&path);

        assert_eq!(loaded, state);
    }

    #[test]
    fn retention_caps_trim_oldest_points() {
        let mut state = NetworkHistoryPersistedState::default();

        state.tiers.second_1s = (0..(SECOND_1S_CAP + 10))
            .map(|i| NetworkHistoryPoint {
                ts_unix: i as u64,
                ..Default::default()
            })
            .collect();
        state.tiers.minute_1m = (0..(MINUTE_1M_CAP + 10))
            .map(|i| NetworkHistoryPoint {
                ts_unix: i as u64,
                ..Default::default()
            })
            .collect();
        state.tiers.minute_15m = (0..(MINUTE_15M_CAP + 10))
            .map(|i| NetworkHistoryPoint {
                ts_unix: i as u64,
                ..Default::default()
            })
            .collect();
        state.tiers.hour_1h = (0..(HOUR_1H_CAP + 10))
            .map(|i| NetworkHistoryPoint {
                ts_unix: i as u64,
                ..Default::default()
            })
            .collect();

        enforce_retention_caps(&mut state);

        assert_eq!(state.tiers.second_1s.len(), SECOND_1S_CAP);
        assert_eq!(state.tiers.minute_1m.len(), MINUTE_1M_CAP);
        assert_eq!(state.tiers.minute_15m.len(), MINUTE_15M_CAP);
        assert_eq!(state.tiers.hour_1h.len(), HOUR_1H_CAP);
        assert_eq!(state.tiers.second_1s.first().map(|p| p.ts_unix), Some(10));
    }

    #[test]
    fn rollup_pipeline_emits_expected_aggregates() {
        let mut state = NetworkHistoryPersistedState::default();
        let mut rollups = NetworkHistoryRollupState::default();

        // 3600 seconds => 60 minute points => 4 x 15m points => 1 hour point
        for i in 1..=3600_u64 {
            let dl = i;
            let ul = i * 2;
            let backoff = i % 100;
            assert!(rollups.ingest_second_sample(&mut state, i, dl, ul, backoff));
        }

        assert_eq!(state.tiers.second_1s.len(), 3600);
        assert_eq!(state.tiers.minute_1m.len(), 60);
        assert_eq!(state.tiers.minute_15m.len(), 4);
        assert_eq!(state.tiers.hour_1h.len(), 1);

        let minute_1 = &state.tiers.minute_1m[0];
        // average of 1..=60
        assert_eq!(minute_1.download_bps, 30);
        assert_eq!(minute_1.upload_bps, 61);
        assert_eq!(minute_1.backoff_ms_max, 60);

        let hour = &state.tiers.hour_1h[0];
        // average of 1..=3600
        assert_eq!(hour.download_bps, 1800);
        assert_eq!(hour.upload_bps, 3601);
        assert_eq!(hour.backoff_ms_max, 99);
    }
}
