# TUI Refactor Plan: Screen-Oriented Architecture With Shared Context and Safe Phased Migration

## Summary
Refactor `src/tui` into screen modules where each screen owns rendering and input mapping, while shared state/settings are provided through a read-only context. Keep domain logic in app core, move UI-only state into `UiState`, and route behavior through `UiAction -> reducer -> UiEffect` for testability and regression control.

This plan is incremental, parity-driven, and includes manual testing after each phase.

## Important Interface and Type Changes
- Add `ScreenId` enum for active/stacked screens.
- Add `ScreenContext<'a>`:
  - `ui: &'a UiState`
  - `app: &'a AppViewModel`
  - `settings: &'a Settings`
- Add `UiState` with shared + per-screen substates:
  - `UiSharedState` (selection, focus, search, redraw, animation clocks, etc.)
  - `NormalScreenState`
  - `BrowserScreenState`
  - `ConfigScreenState`
  - `HelpScreenState`
  - `DeleteConfirmScreenState`
  - `PowerScreenState`
- Add screen trait:
  - `fn draw(&self, f: &mut Frame, ctx: &ScreenContext)`
  - `fn map_input(&mut self, event: CrosstermEvent, ctx: &ScreenContext) -> Vec<UiAction>`
  - optional `fn on_enter(...)` / `fn on_exit(...)`
- Add `UiAction` enum for intent.
- Add `UiEffect` enum for side effects (manager commands, config writes, shutdown request, etc.).
- Add reducer API:
  - `fn reduce(ui: &mut UiState, action: UiAction) -> ReduceResult`
  - `ReduceResult { redraw: bool, effects: Vec<UiEffect> }`
- Add `AppViewModel` read-only projection for UI.
- Transition policy:
  - Root screen is `Normal`.
  - `Esc` on root `Normal` is no-op.
  - `Esc` with unsaved edits uses screen-specific policy:
    - search: clear + exit search
    - config/name edit: confirm discard
    - browser: preserve existing semantics unless explicitly changed

## Phase 0: Baseline and Parity Harness
### Steps
1. Add `tui/README.md` with current architecture, event flow, render flow.
2. Create a parity checklist document for manual behavior.
3. Add/normalize baseline tests for:
  - transitions and key handling currently in `events.rs`
  - layout invariants currently in `layout.rs`/`view.rs`
  - existing tree behavior stays intact
4. Record current transition table and state ownership matrix (current state).

### Automated Gate
- Existing tests pass.
- New baseline tests pass.
- No behavior change.

### Manual Testing
1. Start app and verify all current screens open/close.
2. Verify `Esc` behavior on each screen.
3. Verify search start/edit/exit.
4. Verify browser navigation and selection.
5. Verify config editing entry/exit.
6. Verify help toggle on current platform behavior.

## Phase 1: Screen Module Split (No Logic Change)
### Steps
1. Create `src/tui/screens/` with modules:
  - `normal.rs`, `welcome.rs`, `config.rs`, `browser.rs`, `help.rs`, `power.rs`, `delete_confirm.rs`
2. Move draw functions from `view.rs` into per-screen files.
3. Move event branches from `events.rs` into per-screen input mapping functions.
4. Keep data access unchanged for this phase (still reads from existing app state paths).
5. Keep central dispatch thin in `view.rs`/`events.rs`.

### Automated Gate
- No test regressions.
- No behavioral diff vs baseline tests.

### Manual Testing
1. Repeat full baseline checklist.
2. Confirm each screen still responds to same keys.
3. Confirm no visual regressions in major layouts.

## Phase 2: Introduce Shared Read Context + AppViewModel
### Steps
1. Add `ScreenContext` and `AppViewModel`.
2. Switch screen draw signatures to read-only `ScreenContext`.
3. Keep input mapping per screen; reducer not introduced yet.
4. Move animation clock ticking out of draw and into app loop; draw becomes read-only.
5. Add borrow-first `AppViewModel` rules to avoid per-frame full clones.

### Automated Gate
- Draw path compiles without `&mut AppState`.
- Render/layout tests pass.
- No new direct deep `crate::app::*` dependencies in screen modules except approved facade/view types.

### Manual Testing
1. Verify FPS/data-rate behavior unchanged.
2. Verify theme/effect animation still works.
3. Verify no stutter or noticeable latency increase.
4. Verify power-saving redraw behavior unchanged.

## Phase 3: Extract UI State Into `UiState` (Slice by Slice)
### Status
- Completed:
  - `UiState` attached to `AppState`.
  - Search/selection/redraw/effects moved under `AppState.ui`.
  - `Config`, `DeleteConfirm`, and `FileBrowser` payloads moved from `AppMode` into `UiState` substates.
  - State ownership matrix documented in `src/tui/README.md`.
  - Invariant tests added for selection clamping and search filter/clamp behavior.

### Steps
1. Introduce `UiState` and attach to `App`.
2. Move UI-only fields from `AppState` first:
  - search flags/buffer
  - selection indices/focus/header
  - redraw and animation clocks
3. Move per-screen UI payloads out of `AppMode`/`FileBrowserMode` into screen substates.
4. Keep domain data in app core (`torrents`, peers, metrics, config values).
5. Add explicit state ownership matrix in docs and keep it updated.

### Automated Gate
- All moved fields compile and behave via `UiState`.
- Invariant tests pass:
  - selection clamping
  - search reset/filter behavior
  - browser cursor/expand/collapse behavior

### Manual Testing
1. Verify search still filters and exits correctly.
2. Verify selection remains stable after sorting/filtering.
3. Verify browser/tree interactions parity.
4. Verify config and delete-confirm flows parity.

## Phase 4: `UiAction` + Reducer + Effects Pipeline
### Status
- In progress, mostly complete for key screens:
  - Normal screen reducer/effect path now covers normal-screen hotkeys including paste routing via reducer/effects.
  - Config and delete-confirm screens are fully routed through reducer + effect execution.
  - Browser screen now routes search, filesystem navigation, confirm/escape, download edit/shortcuts, and preview-pane keys through reducer paths.
  - Normal and browser event handlers were split into staged dispatch helpers to keep per-screen entrypoints thin.
  - Root `tui/events.rs` was refactored into explicit pipeline stages (resize -> esc debounce -> mode dispatch).
  - Reducer-focused tests were added/updated for browser dialog/download/preview flows and existing normal/config/delete-confirm reducer coverage remains green.
  - Help screen now routes through dedicated `AppMode::Help` (no global help overlay hook).
  - Search handling is localized per screen (`normal`, `browser`) rather than global interception.

### Implementation Checkpoints (2026-02-15)
- `713fbd1` `tui: start normal-screen action reducer pipeline`
- `15df32a` `tui: route more normal keys through reducer actions`
- `0785cb7` `tui: migrate add and delete shortcuts to reducer actions`
- `50fb930` `tui: move config shortcut into reducer effect path`
- `788efae` `tui: migrate rate theme and pause shortcuts to reducer effects`
- `42ed8bd` `tui: migrate sort shortcut into reducer path`
- `e08ad99` `tui: add action reducer pipeline for config screen`
- `fb4e326` `tui: add action reducer pipeline for delete confirm screen`
- `8ab2ae7` `tui: add browser reducer path for search interceptor`
- `0ae864e` `tui: add browser reducer path for filesystem navigation`
- `0614845` `tui: route browser confirm and escape keys through dialog reducer`
- `605b48d` `tui: extract browser download key reducers`
- `a390816` `tui: route browser download key interception through reducer`
- `0d1f314` `tui: route browser preview pane keys through reducer`
- `ae3d43e` `tui: remove redundant browser helper wrappers`
- `9a60a7e` `tui: remove legacy browser preview helper`
- `583010b` `tui: split browser key handling into staged dispatch helpers`
- `b8a057f` `tui: split normal screen key handling into dispatch helpers`
- `a1c2812` `tui: stage root event pipeline into helper passes`
- `fa435e3` `tui: move help to AppMode and route paste through reducer effects`
- `6b4cde2` `tui: remove global help hook and scope help to normal entry`
- `c011a05` `tui: localize search handling to screen dispatch paths`

### Steps
1. Add `UiAction`, `UiEffect`, `ReduceResult`.
2. Implement reducer for `Normal` screen first.
3. Convert `Normal` key handling to:
  - `event -> UiAction` in screen
  - `UiAction -> reduce(ui)` for state changes
  - app loop executes `UiEffect` via facade
4. Add `AppFacade` methods for side effects.
5. Keep legacy branches for unmigrated screens temporarily.

### Automated Gate
- Reducer unit tests cover mode transitions, search editing, selection clamp, root `Esc`.
- Effect emission tests verify expected side effects for key actions.
- No regression in existing behavior tests.

### Manual Testing
1. Focus on normal-screen hotkeys (`/`, arrows, sorting, theme, quit key, etc.).
2. Verify root `Esc` is no-op.
3. Verify side effects still trigger (pause/resume/config update/shutdown request).
4. Verify errors and warnings still surface correctly.

## Phase 5: Migrate Remaining Screens to Action/Reducer Model
### Steps
1. Migrate `browser`, `config`, `help`, `power`, `delete_confirm`, `welcome`.
2. Add transition table enforcement in reducer:
  - `Back`, `Open`, `CloseOverlay`, `Confirm`, `Cancel`
3. Add screen-specific unsaved-edit policies.
4. Remove legacy event branches only when each screen reaches parity.

### Automated Gate
- Transition matrix tests pass for all screens.
- Per-screen action mapping tests pass.
- No dead code warnings from retired legacy branches.

### Manual Testing
1. Execute full transition table manually.
2. Verify unsaved edit policies:
  - search clears on `Esc`
  - config/name edit prompts discard
3. Verify browser `Esc` semantics match chosen policy.
4. Verify all overlays return to correct previous screen.

## Phase 6: Layout and Theme/Effects Cleanup + Boundary Hardening
### Status
- Completed.
- Layout and effects are now separated by concern:
  - `src/tui/layout/browser.rs` owns browser layout planning.
  - `src/tui/layout/normal.rs` owns normal-screen layout planning.
  - `src/tui/layout/common.rs` holds shared table/column layout helpers.
  - `src/tui/effects.rs` owns theme post-processing and effect-activity speed helpers.
- Boundary hardening updates:
  - `events.rs` remains staged (resize -> debounce -> mode dispatch).
  - `normal` and `browser` screen handlers remain thin staged dispatchers.
  - `view.rs` is now a thin draw dispatcher that calls layout planners/effects modules.
- Architecture docs updated in `src/tui/README.md` with invariants and extension guide.

### Implementation Checkpoints (2026-02-15)
- `9f3688c` `tui: split layout planners and extract theme effects module`
- `926ee89` `tui: harden layout boundaries and finalize phase6 docs`

### Post-Phase Validation (2026-02-15)
- Checklist-mapped parity regression sweep passed:
  - `cargo test -q --no-run`
  - `cargo test -q tui::events::tests`
  - `cargo test -q tui::screens::normal::tests`
  - `cargo test -q tui::screens::browser::tests`
  - `cargo test -q tui::screens::config::tests`
  - `cargo test -q tui::screens::delete_confirm::tests`
  - `cargo test -q tui::events::tests::test_nav_down_torrents`
  - `cargo test -q app::tests::should_only_draw_dirty_in_power_saving_mode`
- API-surface cleanup audit completed:
  - No remaining legacy layout re-export call sites found.
  - Layout usage is now direct per module (`layout::normal`, `layout::browser`, `layout::common`).

### Steps
1. Split layout into `tui/layout/common.rs` + per-screen planners.
2. Keep layout pure: `plan(area, ctx) -> LayoutPlan`.
3. Move theme effects function out of `view.rs` to dedicated theme/effects module.
4. Remove remaining deep coupling from `tui/screens/*`.
5. Finalize docs: architecture, invariants, extension guide for new screens.

### Automated Gate
- Layout unit tests pass per screen breakpoints.
- Theme/effect tests/smoke checks pass.
- `view.rs` and `events.rs` are thin dispatch layers (or consolidated dispatcher).

### Manual Testing
1. Resize terminal across breakpoints and validate each screen layout.
2. Verify theme switching/effects at multiple data rates.
3. Verify power-saving behavior and redraw gating.
4. Perform end-to-end user flow: startup -> browse -> config -> normal -> shutdown.

## Cross-Phase Regression Controls
- One functional slice per PR.
- Mandatory before/after parity checklist for touched behavior.
- Keep legacy path until migrated path is test-covered and parity-verified.
- If parity fails, rollback only current slice, not entire refactor.
- No formatter/lint rewrite-only churn mixed into behavior PRs.

## Test Cases and Scenarios (Minimum Required)
- Transition tests:
  - `Esc` from each non-root screen returns expected screen
  - root `Normal + Esc` is no-op
  - overlay stack push/pop correctness
- Search tests:
  - enter search, edit query, backspace, `Esc`, `Enter`
- Selection tests:
  - clamp after filter/sort/update
- Browser tests:
  - tree expand/collapse/cursor/filter and pane switching
- Config tests:
  - edit, cancel, confirm discard, save/apply effects
- Effect tests:
  - expected `UiEffect` emitted for each command key path
- Layout tests:
  - narrow/short/wide breakpoints per screen
- Theme/effects tests:
  - effect enable/disable and no-mutation draw contract

## Assumptions and Defaults
- Keep current user-visible behavior unless explicitly listed as changed.
- Root `Esc` remains no-op.
- Unsaved-edit `Esc` behavior is screen-specific as defined above.
- `AppViewModel` is borrow-first; avoid full per-frame clones.
- Side effects never run inside reducer; reducer is deterministic and testable.
- Screen modules own input mapping and drawing; shared reducer/effects enforce consistency.

## Final Manual Regression Checklist (Full UI)
Run this checklist in one session after all automated tests pass.

### Setup
1. Launch TUI in a terminal that can be resized.
2. Ensure at least 2 torrents are visible (or mocked) so list/peer navigation is meaningful.
3. Ensure one `.torrent` file path and one magnet link are available for add-flow checks.

### Core Screen Entry/Exit
1. `Welcome -> Normal` transition works.
2. `Normal -> Config` via `c`, and `Config -> Normal` via `Esc` and `Q`.
3. `Normal -> DeleteConfirm` via `d`/`D`; `Esc` cancels, `Enter` confirms, both return to `Normal`.
4. `Normal -> PowerSaving` via `z`; `z` returns to `Normal`.
5. `Normal -> Help` via `m`; help exits back to `Normal` with platform-specific close key:
   - Windows: `m` press
   - Non-Windows: `m` release or `Esc`
6. Verify `m` does not open help from non-normal screens (Config/Browser/DeleteConfirm/PowerSaving).

### Esc Behavior and Debounce Risk Checks
1. In `Normal` (not searching), press `Esc`: no mode change.
2. In `Normal` while searching, press `Esc`: exits search and clears query.
3. In `Config`, press `Esc`: returns to `Normal` and applies expected config behavior.
4. In `DeleteConfirm`, press `Esc`: cancel and return to `Normal`.
5. In browser `ConfigPathSelection`, `Esc` returns to `Config`.
6. In other browser modes, `Esc` returns to `Normal`.
7. Press `Esc` rapidly in each screen above and verify no incorrect cross-screen jumps.

### Normal Screen Behavior
1. Navigation: arrows and `hjkl` move selection/header as expected.
2. Sorting: `s` toggles selected column sort and direction.
3. Search: `/`, typing, backspace, `Enter`, `Esc` all behave correctly.
4. Pause/resume: `p` toggles selected torrent state.
5. Theme: `<` and `>` switch themes immediately.
6. Data rate: `[`/`]` and `{`/`}` adjust rate without UI glitches.
7. Anonymize: `x` toggles displayed names.
8. Quit intent: `Q` triggers quit flow.

### Paste/Add Flows
1. Non-Windows bracketed paste and Windows `v` paste both add valid magnet links.
2. Paste valid `.torrent` path and verify add behavior (default path vs no default path cases).
3. Paste invalid text and verify user-facing error message appears.
4. `a` opens file browser add flow and remains functional.

### Browser Screen Behavior
1. File nav: `Enter`/`Right` into dir, `Backspace`/`Left`/`u` to parent.
2. Browser search: `/`, typing, backspace, `Enter`, `Esc`.
3. Confirm key `Y` performs expected action per browser mode.
4. Download-location mode:
   - `Tab` switches pane focus.
   - `x` toggles container usage.
   - `r` enters rename; edit keys work; `Enter` commits rename; `Esc` cancels rename.
   - Preview pane nav keys move correctly and `Space` cycles priority.

### Config Screen Behavior
1. Up/down navigation across config items.
2. Edit entry/commit/cancel flows function.
3. Rate increase/decrease effects are applied.
4. Path-selection handoff to browser and back to config works.

### Rendering/Layout/Effects
1. Resize terminal: narrow, medium, wide, very short heights; verify all screens remain usable.
2. Verify normal layout regions (list/details/peers/chart/stats/footer) stay aligned.
3. Verify browser layout adapts with/without preview and with search bar.
4. Verify theme effects still animate and do not corrupt text readability.
5. Verify PowerSaving redraw behavior still avoids unnecessary redraws.

### End-to-End Flow
1. Startup -> Normal -> Add torrent (magnet or file) -> Browser destination selection -> confirm.
2. Return to Normal, sort/filter/search, pause/resume, open/close help.
3. Open Config, change a value, return and confirm behavior.
4. Delete flow (cancel then confirm) works.
5. Shutdown flow completes cleanly.
