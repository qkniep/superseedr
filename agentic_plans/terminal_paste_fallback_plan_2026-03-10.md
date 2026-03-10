# Terminal Paste Fallback Plan (Normal Screen, Clean Baseline)

## Summary
- Add a Normal-screen paste-burst fallback so terminals that do not emit `CrosstermEvent::Paste` still route pasted magnet links and `.torrent` paths through the existing paste flow.
- Remove the debug instrumentation and probing changes before landing the fallback.
- Remove the Windows clipboard dependency after the fallback is in place.
- Treat bracketed paste as best-effort: enable it, prefer real `Paste(...)` events when they arrive, and keep the burst fallback available because there is no reliable up-front capability detection on the affected Windows path.

## Key Changes
- Add a small `src/tui/paste_burst.rs` state machine that buffers rapid plain-char input and flushes it either as synthetic paste text or replayed key events.
- Store burst state in `AppState.ui` and flush it from the main app loop on its own deadline.
- Intercept Normal-screen plain-char input in `src/tui/events.rs` before screen dispatch, while keeping explicit `Paste(...)` events as the preferred path.
- Keep `src/tui/screens/normal.rs` as the single place that classifies and handles pasted magnet links and `.torrent` file paths.
- Handle Windows terminal quirks by treating `KeyEventKind::Repeat` as part of burst capture and ignoring `KeyEventKind::Release` for burst classification so the first pasted character is not replayed as a shortcut.

## Detection Notes
- `superseedr` currently does not try to auto-detect "true bracketed paste support" and selectively disable the fallback for the session.
- The comparison `codex` codebase follows the same broad model: it enables bracketed paste best-effort, handles real paste events immediately, and keeps paste-burst logic available behind configuration rather than runtime capability detection.
- `supports_keyboard_enhancement()`-style checks are not sufficient for this problem; they do not reliably answer whether pasted clipboard data will arrive as `Paste(...)` on the Windows setups we debugged.
- Because of that, the current `superseedr` implementation keeps the fallback active and accepts the small Normal-screen plain-key delay as the tradeoff for reliable Windows paste handling.

## Validation
- `cargo fmt`
- `cargo check`
- targeted tests covering `tui::paste_burst`, `tui::events`, and `tui::screens::normal`
- manual Windows Terminal `Ctrl+V` check with a magnet link
- manual confirmation that the Windows paste path no longer opens the file browser on the leading pasted character