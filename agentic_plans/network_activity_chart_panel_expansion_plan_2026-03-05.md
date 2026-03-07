# Expand Activity Chart Panel With Multi-View Modes + Persisted Torrent Overlay

## Summary
Add a new chart-view layer on top of the existing time-range graph so users can switch chart content between `Network`, `CPU`, `RAM`, `Disk`, `Tuning`, and `Torrent Overlay` while keeping existing `t/T` time-scale behavior.

Implement tiered history plus persistence for all new modes, including per-torrent overlay history tracked for every torrent currently in the list.

## Key Changes
1. UI state and controls
- Add `ChartPanelView` enum in app state: `Network`, `Cpu`, `Ram`, `Disk`, `Tuning`, `TorrentOverlay`.
- Keep `t/T` for time range; add `g/G` for chart view next/prev.
- Update help screen and footer command hints with new controls.

2. History model and persistence
- Introduce a generalized chart history persisted state (new binary file/module) that keeps tiered series for:
  - CPU%
  - RAM%
  - Disk read bps
  - Disk write bps
  - Tuning score (current)
  - Tuning baseline/best score
  - Per-torrent overlay series keyed by `info_hash` (net samples)
- Reuse the existing 1s/1m/15m/1h rollup pattern and retention caps to support 1m..1y in all chart views.
- Persist overlay history for all torrents currently present in `torrent_list_order`; prune history when torrent is removed from list.
- Keep existing network history persistence compatible; add migration/read-path so old persisted files still load without data loss for network mode.

3. Telemetry ingestion
- On each second tick, ingest CPU/RAM/disk/tuning samples into new chart rollups.
- On each second tick, ingest per-torrent speed samples for all active torrents (and zero samples for tracked but idle torrents to keep alignment).
- On late restore/startup, densify series and rehydrate in-memory short-window buffers from persisted tiers.

4. Chart rendering refactor
- Refactor current `draw_network_chart` into a view-dispatch renderer:
  - Shared window selection, x-axis labels, smoothing policy, and title framework.
  - Per-view dataset builders and y-axis label formatting.
- `Network` view remains current DL/UL + backoff markers behavior.
- `CPU` and `RAM` views render percentage series with fixed 0-100 y-bounds.
- `Disk` view renders read/write throughput series.
- `Tuning` view renders current tuning score + baseline/best reference series.
- `Torrent Overlay` view renders top 5 active torrents for selected window, and always includes the currently highlighted torrent if it is not already in that set:
  - Net-speed line per torrent.
  - Deterministic color assignment by info-hash; compact legend with truncation.

5. Public interface/type updates
- New app-level enum/type additions:
  - `ChartPanelView`
- New UI reducer actions/effects for chart-view cycling.
- New persisted schema/type for generalized activity history (and loader/saver API alongside existing persistence APIs).

## Test Plan
1. Reducer/keybinding tests
- `g/G` cycles chart views correctly and wraps.
- `t/T` still only cycles time range.

2. History/rollup tests
- Per-second ingestion creates expected tier points for CPU/RAM/disk/tuning and per-torrent series.
- Rollups aggregate correctly into 1m/15m/1h tiers.
- Densify/restore reconstructs aligned windows with zero-fill gaps.
- Torrent removal prunes corresponding persisted overlay history.

3. Persistence compatibility tests
- Existing network history file loads as before.
- New activity history file round-trips all series, including per-torrent keyed data.
- Corrupt/newer schema fallback behavior is safe (reset + warn, no panic).

4. Renderer tests
- Each chart view builds non-empty datasets from valid history and honors y-axis rules.
- Overlay mode uses top-5-plus-highlight selection and stable color mapping.

5. Manual acceptance scenarios
- User can switch between all six views and all existing time ranges.
- Long-range views (`7d`, `30d`, `1y`) show non-network data (not stretched short-window artifacts).
- Overlay retains history across restart for torrents still in list.

## Assumptions and Defaults
- Chart content switching replaces current single-purpose network chart behavior.
- `g/G` controls chart view; `t/T` remains time-scale control.
- Overlay mode defaults to top 5 active torrents plus highlighted torrent inclusion.
- Overlay supports full-range persisted history.
- Overlay history is retained while torrent remains in list; removed torrents are pruned.
