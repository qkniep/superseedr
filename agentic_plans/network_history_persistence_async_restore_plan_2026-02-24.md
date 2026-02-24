# Network History Persistence Plan (Async Restore + Dirty Writes)

## Summary
Implement simple file-based persistence for global network time-series history using dynamic rollups, without introducing SQLite. Startup should not block on loading history; persisted data is restored asynchronously after app boot. Writes are periodic and conditional (dirty-check), with a forced flush on shutdown.

## Scope
- In scope:
  - Global network chart history persistence only.
  - Multi-resolution rollup storage for long retention.
  - Async read after startup.
  - Async writes every 15 seconds, skipped when unchanged.
  - Guaranteed flush at clean shutdown.
- Out of scope:
  - Peer accounting/reputation persistence.
  - Per-torrent peer/block stream persistence.
  - SQLite integration.
  - Per-torrent telemetry persistence implementation (only architecture boundary is planned now).

## Decisions Locked
1. Persistence backend: TOML file in app data dir (`persistence/network_history.toml`).
2. Retention profile:
   - `1s` tier: keep 1 hour
   - `1m` tier: keep 48 hours
   - `15m` tier: keep 30 days
   - `1h` tier: keep 365 days
3. Runtime writes:
   - Timer: every 15 seconds
   - Condition: persist only if new data was ingested since last successful save
   - Shutdown: always flush regardless of dirty flag
4. Startup load:
   - Non-blocking app startup
   - History file loaded in background (`spawn_blocking`)
   - App state hydrated when load completes
5. Telemetry ownership boundary:
   - Keep global history persistence logic out of `ManagerTelemetry`.
   - Keep `ManagerTelemetry` focused on per-torrent snapshot emit/dedupe policy.
   - Reserve a separate per-torrent history telemetry path for future persistence.

## Data Model
Add `src/persistence/network_history.rs` with:
- `NetworkHistoryPersistedState`
  - `schema_version: u32`
  - `updated_at_unix: u64`
  - tiered data series for:
    - download bps
    - upload bps
    - backoff max (ms)
- Timestamped points per tier to support rollup correctness and future migration.

Add versioning and tolerant parsing:
- Missing file => default empty state
- Corrupt/invalid file => warning + default empty state
- Future schema => migration entrypoint via `schema_version`

## App Integration
### New/extended state
- Extend `PersistPayload` in `src/app.rs` with `network_history_state`.
- Add in-memory rollup holder in `AppState` (aggregator + tier buffers).
- Add `network_history_dirty: bool` in `AppState`.

### Telemetry component direction
- Current phase:
  - Global history is aggregated from app-level telemetry after per-second updates.
- Future phase:
  - Add a distinct `TorrentHistoryTelemetry` component for per-torrent persisted series keyed by info-hash.
- Separation principle:
  - `ManagerTelemetry`: manager snapshot emission decisions only.
  - Global/per-torrent history components: rollups, retention, and persistence-facing state.

### Startup flow
1. `App::new` initializes with empty/live history.
2. Spawn background task to load persisted network history via `spawn_blocking`.
3. On completion, send internal app command/event (e.g. `AppCommand::NetworkHistoryLoaded(...)`).
4. Handler hydrates chart buffers safely:
   - `avg_download_history`, `avg_upload_history`, `disk_backoff_history_ms`
   - `minute_avg_dl_history`, `minute_avg_ul_history`, `minute_disk_backoff_history_ms`
5. Merge strategy must preserve any already-collected live samples.

### Tick/update flow
- Each second, ingest latest live sample into rollup aggregator.
- Build higher tiers from lower tiers:
  - 60 x `1s` -> `1m`
  - 15 x `1m` -> `15m`
  - 4 x `15m` -> `1h`
- Enforce retention caps immediately after append.
- Set `network_history_dirty = true` only when samples/rollups are appended.

### Persistence flow
- Keep existing persistence worker pattern (watch channel + `spawn_blocking`).
- Save path remains atomic (`.tmp` then `rename`).
- Add 15s timer in main run loop:
  - If `network_history_dirty == true`, call `save_state_to_disk()`.
  - On successful write, clear dirty flag.
  - If write fails, keep dirty flag set for retry.

### Shutdown flow
1. Call `save_state_to_disk()` unconditionally.
2. Call existing `flush_persistence_writer().await` to join writer task.
3. Exit only after queued persistence completes.

## Performance Expectations
- CPU: negligible per-second append and periodic rollups.
- Memory: bounded by retention caps.
- Disk I/O: moderate and controlled by dirty-check; no write when idle.
- Startup: immediate UI availability; background hydration completes shortly after.

## Test Plan
1. Round-trip serialization/deserialization of populated multi-tier state.
2. Missing/corrupt file fallback returns default state.
3. Retention pruning keeps exact cap sizes per tier.
4. Rollup math validation for `1s -> 1m -> 15m -> 1h`.
5. Async post-start restore hydrates state after app begins running.
6. Merge safety: live samples collected before restore are not lost.
7. Dirty-check behavior:
   - no new data => no write triggered on 15s tick
   - new data => write triggered
8. Shutdown flush persists latest snapshot even when timer did not fire recently.

## Acceptance Criteria
- Network chart history survives restart and appears shortly after launch.
- App startup is not blocked by history file I/O.
- No periodic writes occur when there is no new telemetry.
- Last session history is retained on clean shutdown.
- No SQLite dependency introduced.
