# TUI Phase 0 Manual Parity Checklist

Run this checklist before/after each refactor slice. Record pass/fail notes.

## Setup
1. Build and run the app in TUI mode.
2. Ensure at least one torrent row is visible for list interactions.
3. Ensure terminal resizing is possible during the run.

## Core Navigation
1. Verify arrow keys and `hjkl` navigate torrent/peer tables.
2. Verify selection remains in bounds at top/bottom edges.
3. Verify sorting (`s`) on selected header works and toggles direction.

## Search Behavior
1. From normal mode press `/`, type characters, and verify list filtering.
2. Press `Backspace` and verify filter updates.
3. Press `Enter` to exit search but keep current query behavior.
4. Press `Esc` during search and verify search clears and exits.

## Screen and Overlay Transitions
1. `z` enters `PowerSaving`; `z` exits to `Normal`.
2. `c` opens `Config`; `Esc` or `Q` returns to `Normal`.
3. `d`/`D` opens `DeleteConfirm`; `Esc` cancels; `Enter` confirms and returns.
4. Help overlay:
   - Windows: `m` toggles.
   - Non-Windows: `m` press opens; `m` release or `Esc` closes.
5. `Esc` in `Normal` does not change mode (only clears error banner if present).

## File Browser Flows
1. Press `a` to open add-torrent browser.
2. Navigate directories (`Enter`/`Right`) and parent (`Backspace`/`Left`/`u`).
3. Use `/` search within browser and verify filtering.
4. In download-location mode:
   - `Tab` switches pane focus.
   - `x` toggles container usage.
   - `r` enters name edit; `Esc` cancels edit; `Enter` commits edit.
5. `Esc` exits browser:
   - Back to `Config` for `ConfigPathSelection`.
   - Back to `Normal` for other browser modes.

## Theme and Display
1. Use `<` and `>` to change theme; verify immediate update.
2. Use `[`/`]` (`{`/`}`) to change data rate; verify UI remains responsive.
3. Resize terminal to narrow and wide sizes; verify layout remains usable.

## Quit/Safety
1. `Q` triggers quit flow.
2. No unexpected mode transitions occur during rapid `Esc` presses.
