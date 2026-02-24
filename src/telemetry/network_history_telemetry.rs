// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::persistence::network_history::{
    enforce_retention_caps, NetworkHistoryPersistedState, NetworkHistoryPoint,
};
use std::collections::{BTreeMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct NetworkHistoryTelemetry;

impl NetworkHistoryTelemetry {
    pub fn on_second_tick(app_state: &mut AppState) {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let download_bps = app_state.avg_download_history.last().copied().unwrap_or(0);
        let upload_bps = app_state.avg_upload_history.last().copied().unwrap_or(0);
        let backoff_ms_max = app_state.disk_backoff_history_ms.back().copied().unwrap_or(0);
        if app_state.network_history_rollups.ingest_second_sample(
            &mut app_state.network_history_state,
            now_unix,
            download_bps,
            upload_bps,
            backoff_ms_max,
        ) {
            app_state.network_history_dirty = true;
        }
    }

    pub fn apply_loaded_state(app_state: &mut AppState, state: NetworkHistoryPersistedState) {
        let was_dirty = app_state.network_history_dirty;
        let merged = merge_state_for_late_restore(&app_state.network_history_state, state);

        app_state.avg_download_history = merged
            .tiers
            .second_1s
            .iter()
            .map(|p| p.download_bps)
            .collect();
        app_state.avg_upload_history = merged
            .tiers
            .second_1s
            .iter()
            .map(|p| p.upload_bps)
            .collect();
        app_state.disk_backoff_history_ms = VecDeque::from(
            merged
                .tiers
                .second_1s
                .iter()
                .map(|p| p.backoff_ms_max)
                .collect::<Vec<_>>(),
        );

        app_state.minute_avg_dl_history = merged
            .tiers
            .minute_1m
            .iter()
            .map(|p| p.download_bps)
            .collect();
        app_state.minute_avg_ul_history = merged
            .tiers
            .minute_1m
            .iter()
            .map(|p| p.upload_bps)
            .collect();
        app_state.minute_disk_backoff_history_ms = VecDeque::from(
            merged
                .tiers
                .minute_1m
                .iter()
                .map(|p| p.backoff_ms_max)
                .collect::<Vec<_>>(),
        );

        app_state.network_history_state = merged;
        // Preserve dirty state if live samples were already pending flush.
        app_state.network_history_dirty = was_dirty;
    }
}

fn merge_tier_points(loaded: Vec<NetworkHistoryPoint>, live: Vec<NetworkHistoryPoint>) -> Vec<NetworkHistoryPoint> {
    let mut by_ts = BTreeMap::<u64, NetworkHistoryPoint>::new();
    for point in loaded {
        by_ts.insert(point.ts_unix, point);
    }
    // Live points win for identical timestamps.
    for point in live {
        by_ts.insert(point.ts_unix, point);
    }
    by_ts.into_values().collect()
}

fn merge_state_for_late_restore(
    live_state: &NetworkHistoryPersistedState,
    loaded_state: NetworkHistoryPersistedState,
) -> NetworkHistoryPersistedState {
    let mut merged = NetworkHistoryPersistedState {
        schema_version: loaded_state.schema_version.max(live_state.schema_version),
        updated_at_unix: loaded_state.updated_at_unix.max(live_state.updated_at_unix),
        tiers: crate::persistence::network_history::NetworkHistoryTiers {
            second_1s: merge_tier_points(
                loaded_state.tiers.second_1s,
                live_state.tiers.second_1s.clone(),
            ),
            minute_1m: merge_tier_points(
                loaded_state.tiers.minute_1m,
                live_state.tiers.minute_1m.clone(),
            ),
            minute_15m: merge_tier_points(
                loaded_state.tiers.minute_15m,
                live_state.tiers.minute_15m.clone(),
            ),
            hour_1h: merge_tier_points(
                loaded_state.tiers.hour_1h,
                live_state.tiers.hour_1h.clone(),
            ),
        },
    };

    enforce_retention_caps(&mut merged);
    merged
}

#[cfg(test)]
mod tests {
    use super::{merge_state_for_late_restore, merge_tier_points, NetworkHistoryTelemetry};
    use crate::app::AppState;
    use crate::persistence::network_history::{NetworkHistoryPersistedState, NetworkHistoryPoint};
    use std::collections::VecDeque;

    #[test]
    fn merge_tier_points_prefers_live_on_timestamp_collision() {
        let loaded = vec![NetworkHistoryPoint {
            ts_unix: 10,
            download_bps: 100,
            upload_bps: 10,
            backoff_ms_max: 1,
        }];
        let live = vec![NetworkHistoryPoint {
            ts_unix: 10,
            download_bps: 200,
            upload_bps: 20,
            backoff_ms_max: 2,
        }];

        let merged = merge_tier_points(loaded, live);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].download_bps, 200);
        assert_eq!(merged[0].upload_bps, 20);
        assert_eq!(merged[0].backoff_ms_max, 2);
    }

    #[test]
    fn apply_loaded_state_merges_late_data_and_preserves_dirty() {
        let mut app_state = AppState {
            avg_download_history: vec![100],
            avg_upload_history: vec![10],
            disk_backoff_history_ms: VecDeque::from(vec![1]),
            network_history_dirty: true,
            ..Default::default()
        };
        app_state.network_history_state.tiers.second_1s.push(NetworkHistoryPoint {
            ts_unix: 2,
            download_bps: 100,
            upload_bps: 10,
            backoff_ms_max: 1,
        });

        let mut loaded = NetworkHistoryPersistedState::default();
        loaded.tiers.second_1s.push(NetworkHistoryPoint {
            ts_unix: 1,
            download_bps: 200,
            upload_bps: 20,
            backoff_ms_max: 2,
        });
        loaded.tiers.second_1s.push(NetworkHistoryPoint {
            ts_unix: 2,
            download_bps: 150,
            upload_bps: 15,
            backoff_ms_max: 3,
        });

        NetworkHistoryTelemetry::apply_loaded_state(&mut app_state, loaded);

        // ts=2 should come from live value (100), not loaded overlap (150).
        assert_eq!(app_state.avg_download_history, vec![200, 100]);
        assert_eq!(app_state.avg_upload_history, vec![20, 10]);
        assert_eq!(app_state.disk_backoff_history_ms, VecDeque::from(vec![2, 1]));
        assert!(app_state.network_history_dirty);
    }

    #[test]
    fn merge_state_for_late_restore_preserves_live_point_on_overlap() {
        let mut live = NetworkHistoryPersistedState::default();
        live.tiers.second_1s.push(NetworkHistoryPoint {
            ts_unix: 5,
            download_bps: 500,
            upload_bps: 50,
            backoff_ms_max: 5,
        });
        let mut loaded = NetworkHistoryPersistedState::default();
        loaded.tiers.second_1s.push(NetworkHistoryPoint {
            ts_unix: 5,
            download_bps: 300,
            upload_bps: 30,
            backoff_ms_max: 3,
        });

        let merged = merge_state_for_late_restore(&live, loaded);
        assert_eq!(merged.tiers.second_1s.len(), 1);
        assert_eq!(merged.tiers.second_1s[0].download_bps, 500);
    }
}
