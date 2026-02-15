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
- In progress:
  - Normal screen has initial `UiAction`/`UiEffect`/`ReduceResult` scaffold.
  - Reducer path currently handles: `Esc`, `/`, arrow/vim navigation, `x`, `z`, `Q`, `t`, `T`.
  - Reducer unit tests cover search start, error clear, navigation, anonymize toggle, power-saving transition, quit flag, and graph mode cycling.

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
