
# Frontend TUI Refactor Plan

## Goals
- Reduce coupling between `tui/*` and `crate::app::*`
- Replace “god modules” (`tui/view.rs`, `tui/events.rs`) with screen/components
- Move UI-only state out of `AppState`
- Make rendering mostly pure (immutable inputs)
- Enable unit tests via `UiAction` + reducer pattern
- Keep changes incremental (no big-bang rewrite)

## Guiding Principles
- **UI is an adapter**: it should translate input → intent, and state → pixels.
- **App core owns domain state**; UI owns selection/scroll/focus/search buffers.
- Prefer **thin interfaces** (`AppFacade` / `AppView`) over importing the full `App`.

---

## Phase 0 — Baseline & Safety Nets (0–1 PRs)
### Tasks
- Add `tui/README.md` describing current architecture and responsibilities.
- Add minimal smoke tests:
  - Build test (already implicit in CI)
  - Optional: a small unit test for `layout.rs` column selection (pure function)
- Add `#[deny(clippy::unwrap_used)]` only if feasible; otherwise don’t tighten linting yet.

### Acceptance Criteria
- No behavior change.
- A few low-effort tests exist for pure code paths.

---

## Phase 1 — Screen Modularization (2–4 PRs)
### Goal
Stop growing `view.rs` and `events.rs` by introducing screen modules.

### File/Module Changes
- Create:
  - `tui/screens/mod.rs`
  - `tui/screens/welcome.rs`
  - `tui/screens/normal.rs`
  - `tui/screens/config.rs`
  - `tui/screens/browser.rs`
  - `tui/screens/help.rs`
  - `tui/screens/power.rs`
  - `tui/screens/delete_confirm.rs`

### Design
Define a minimal screen interface:
- `fn draw(...)`
- `fn handle_event(...) -> EventResult`

Example (conceptual):
- `EventResult::{ConsumedRedraw, ConsumedNoRedraw, NotConsumed}`

### Migration Steps
- Move drawing code from `tui/view.rs` into screen modules, one screen at a time.
- Move event branches from `tui/events.rs` into corresponding screen modules.

### Acceptance Criteria
- `tui/view.rs` becomes mostly a dispatcher + shared widget helpers.
- `tui/events.rs` becomes mostly a dispatcher.
- No changes to `AppState` yet; still uses current data paths.
- Behavior matches current UI.

---

## Phase 2 — Extract UI-only State (3–6 PRs)
### Goal
Move selection/focus/search/tree view state out of `AppState`.

### Create
- `tui/ui_state.rs` (or `tui/state.rs` if available and appropriate)

### Move out of `AppState` (examples)
- Mode-related UI flags that exist solely for rendering or navigation
- Selection indices and scroll offsets
- Search input buffer + “search mode” state
- File browser: `TreeViewState`, expand/collapse, filter string, focus, etc.
- Animation/effects time state (phase time, last frame time)
- “ui_needs_redraw” (or keep as UI-owned flag)

### Keep in `AppState` (examples)
- Torrent list data, peers, metrics/histories, manager-derived data
- Domain config values (not “config editor cursor position”)

### Migration Steps
- Introduce `UiState` and plumb it through:
  - `App` holds `ui: UiState` + `app_state: AppState`
  - update `view` and `events` to use `ui` instead of storing UI state in `AppState`
- Update each screen to read/write `UiState` for UI concerns.

### Acceptance Criteria
- `crate::app::AppState` shrinks and becomes domain-oriented.
- UI behavior unchanged.
- New unit tests can be written against `UiState` methods without `AppState`.

---

## Phase 3 — Make Rendering Mostly Pure (2–4 PRs)
### Goal
Rendering should not require `&mut AppState` except for explicit UI animation clocks.

### Changes
- Update signatures from:
  - `draw(f, &mut AppState, &Settings)`
  - to: `draw(f, ui: &UiState, app: &AppViewModel, settings: &Settings)`
- Create `AppViewModel` (or `AppView`) that provides read-only data needed by UI.

### Migration Steps
- Build `AppViewModel` as a struct of references or cloned slices:
  - torrents list (already sorted/filtered)
  - peers for selected torrent
  - summaries, stats, config snapshot, etc.
- Compute view model in the app loop once per frame or per change.
- Make “effects_phase_time” and other animation clocks live in `UiState`.

### Acceptance Criteria
- `tui/screens/*::draw` takes immutable state.
- Side effects in rendering eliminated (or isolated to `UiState` clock updates outside draw).

---

## Phase 4 — Introduce `UiAction` + Reducer (4–8 PRs)
### Goal
Decouple key bindings from behavior and make input handling testable.

### Create
- `tui/actions.rs`:
  - `enum UiAction { ... }`
- `tui/reducer.rs`:
  - `fn reduce(ui: &mut UiState, app: &mut dyn AppFacade, action: UiAction) -> ReduceResult`

### Create an `AppFacade`
A narrow interface exposed to UI:
- Torrent controls: start/stop/pause/resume/remove
- Sorting/filtering triggers (or UI sets filter and app recomputes view model)
- Config apply/save actions
- Shutdown request

### Migration Steps
- Convert screen input handlers:
  - `Event -> UiAction` mapping (keymap)
  - then `reduce(...)`
- Start with one screen (Normal) and migrate others gradually.
- Keep legacy paths temporarily where needed (bridge actions to existing `app` methods).

### Acceptance Criteria
- Unit tests exist for `reduce()`:
  - search input editing
  - selection clamping after filtering
  - mode transitions
- Keymap changes no longer require touching app logic.

---

## Phase 5 — Theme & Effects Cleanup (1–3 PRs)
### Goal
Make theme implementation coherent and reduce `view.rs` responsibility.

### Changes
- Move `apply_theme_effects_to_frame` out of `tui/view.rs` into:
  - `tui/theme_fx.rs` or `tui/theme.rs`
- Split large theme definitions:
  - `tui/theme/mod.rs`
  - `tui/theme/palettes.rs`
  - `tui/theme/effects.rs`
  - `tui/theme/registry.rs`

### Optional Performance Tweaks
- Skip effects pass if disabled (already mostly there)
- Consider applying effects only to affected regions (later optimization)

### Acceptance Criteria
- `tui/view.rs` no longer contains theme effect logic.
- Theme changes are localized and easier to review.

---

## Phase 6 — Tighten Boundaries & Remove Debt (ongoing)
### Tasks
- Remove remaining direct `use crate::app::*` imports from deep inside widgets:
  - screens should depend on `UiState` + `AppViewModel` only
- Consolidate shared widgets into `tui/widgets/*`
- Reduce duplication in layout computations
- Document invariants:
  - selection indices and scroll offsets rules
  - mode transition rules
  - view model rebuild triggers

### Acceptance Criteria
- `tui/` compiles with minimal knowledge of app internals.
- New screens/features can be added without touching giant match statements.
- Tests cover key UI flows.

---

## Suggested PR Breakdown (practical order)
1. Add screens folder + move `welcome` draw/event.
2. Move `normal` draw/event.
3. Move remaining screens draw/event.
4. Introduce `UiState`, migrate search/selection first.
5. Migrate browser tree UI state next.
6. Make draw immutable via `AppViewModel`.
7. Add `UiAction` + reducer for Normal screen.
8. Migrate remaining screens to actions.
9. Theme/effects extraction.

---

## “Definition of Done”
- `tui/view.rs` is a thin dispatcher + shared helpers.
- `tui/events.rs` is a thin dispatcher or gone entirely (replaced by screen handlers).
- UI-only state is not in `AppState`.
- Rendering is pure (immutable app inputs), with explicit UI clock updates.
- Input is testable through `UiAction` + `reduce()`.
- Theme effects and theme data are clearly separated and not mixed into view rendering logic.
