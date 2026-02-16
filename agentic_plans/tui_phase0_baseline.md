# TUI Phase 0 Baseline: Transition Table and State Ownership Matrix

This baseline is a reference for parity checks during the refactor.

## Transition Table (Current Behavior)
| From | Trigger | To | Notes |
|---|---|---|---|
| `Welcome` | `Esc` | `Normal` | Exit splash/welcome screen |
| `Normal` | `/` | `Normal` | Enters in-place search (`is_searching = true`) |
| `Normal` | `z` | `PowerSaving` | Power-saving mode |
| `Normal` | `c` | `Config` | Opens settings editor |
| `Normal` | `a` | `FileBrowser` | Add torrent flow (`File` browser mode) |
| `Normal` | `d`/`D` | `DeleteConfirm` | Delete dialog (metadata only / with files) |
| `Normal` | `Esc` | `Normal` | Clears `system_error`; stays in Normal |
| `PowerSaving` | `z` | `Normal` | Exits power-saving mode |
| `Config` | `Esc` or `Q` | `Normal` | Sends `UpdateConfig` and exits |
| `Config` | `Enter` on path item | `FileBrowser` | `ConfigPathSelection` browser mode |
| `FileBrowser` (`ConfigPathSelection`) | `Esc` | `Config` | Returns with current edit state |
| `FileBrowser` (other modes) | `Esc` | `Normal` | Clears pending browser/search context |
| `FileBrowser` | `Y` | `Normal` or `Config` | Context-dependent confirmation path |
| `DeleteConfirm` | `Enter` | `Normal` | Sends delete/shutdown command to manager |
| `DeleteConfirm` | `Esc` | `Normal` | Cancel |

## Overlay Behavior (Current)
- Help is currently an overlay (`show_help`) not a dedicated screen mode.
- Windows: `m` press toggles overlay.
- Non-Windows: `m` press opens; `m` release or `Esc` closes.

## State Ownership Matrix (Current, Pre-Refactor)

### App Domain-Owned (Should stay in app core)
- Torrent map/order and manager-derived data:
  - `torrents`, `torrent_list_order`, manager channels/receivers
- Metrics/history/runtime and resource data:
  - rate histories, CPU/RAM, disk telemetry, limits, run time, tuning fields
- App lifecycle and warnings/errors:
  - `should_quit`, `system_warning`, `system_error`, update availability
- Settings/config values:
  - `client_configs`, persisted config fields

### UI-Owned But Currently in `AppState` (Target for `UiState`)
- View and navigation:
  - `screen_area`, `selected_header`, `selected_torrent_index`, `selected_peer_index`
- UI controls and search:
  - `show_help`, `is_searching`, `search_query`, `anonymize_torrent_names`
- Redraw and visual effect clocks:
  - `ui_needs_redraw`, `theme`, `effects_phase_time`, `effects_last_wall_time`, `effects_speed_multiplier`
- UI display preferences:
  - `graph_mode`, `data_rate`

### UI Screen-Local But Currently in App Mode Enums
- `AppMode::Config { selected_index, editing, settings_edit, ... }`
- `AppMode::FileBrowser { state, data, browser_mode }`
- `FileBrowserMode::DownloadLocSelection { focused_pane, preview_state, cursor_pos, ... }`
- `FileBrowserMode::ConfigPathSelection { selected_index, items, current_settings, ... }`

These enum payloads are current coupling hotspots and are expected migration targets during the refactor.
