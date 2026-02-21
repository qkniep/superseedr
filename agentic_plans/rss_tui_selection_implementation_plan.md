# Superseedr TUI RSS Implementation Plan (Progress Update)

## Status Summary (as of current `rss` branch)
This document now tracks **implemented behavior** and **remaining UX iteration work**.

### Implemented Foundation
1. RSS mode exists (`r` from normal mode) and is functional end-to-end.
2. Primary RSS UI is unified into one responsive screen:
- `Links + Filters + Explorer` in one view.
- Separate `History` screen.
3. Responsive layout:
- Wide (`>= 140 cols`): Explorer left, right column split Links/Filters (50/50).
- Narrow (`< 140 cols`): Explorer / Filters / Links vertical stack.
4. Focus/navigation:
- Pane focus: `Tab` / `Shift+Tab`, plus `h/l` and `←/→`.
- Row movement: `j/k` and `↑/↓`.
- History: `H`.
5. Input UX:
- Search/edit text input now appears in a dedicated top input panel.
- Inline input/search text was removed from inside panes.
6. Feed/filter behavior:
- Add/delete/toggle links.
- Add/delete filters.
- Filter live preview shows full list, ranks matches first, greys non-matches.
- Empty filter draft still shows full list.
7. Explorer behavior:
- Match-priority sorting automatic when active query/filter context exists.
- Non-matches dimmed when prioritization is active.
- Downloaded rows badged.
8. Sync behavior:
- `S` triggers sync now.
- RSS enabled by default.
- `S` auto-enables RSS if disabled.
- RSS config changes auto-trigger sync (no manual sync required after add/edit).
9. Persistence split:
- Durable config in `settings.toml`.
- Runtime RSS state in `persistence/rss.toml`.
10. Worker/runtime:
- Feed polling + parse + aggregation + dedupe + auto-ingest path in place.
- Retry/backoff with jitter for feed fetch.

---

## Current Product Contract

### Primary workflow
1. Add RSS links in Links pane.
2. Explore aggregated preview items in Explorer.
3. Create/edit filters in Filters pane.
4. Auto-ingest occurs only for matched items.

### Explicitly removed from RSS UI
1. Manual one-off add from Explorer.
2. Copy selected Explorer link.

---

## Keybinds (Current)

### Global RSS mode
- `Esc` / `q`: exit RSS mode.
- `H`: open History screen.
- `S`: Sync Now.

### Unified screen navigation
- `Tab` / `Shift+Tab`: cycle pane focus.
- `h/l` or `←/→`: previous/next pane focus.
- `j/k` or `↑/↓`: move selection in focused pane.

### Focused pane actions
- Links pane:
- `a` add link
- `d` delete link
- `x` toggle link enabled
- Filters pane:
- `a` add filter
- `d` delete filter
- Explorer pane:
- `/` start search input
- `F` seed filter draft from selected Explorer title

### Input modes
- `Enter`: commit input
- `Esc`: cancel input/search
- typing + paste supported

---

## What Was Changed From Earlier Plan
1. Multi sub-screen navigation (`l/f/e`) was replaced by single unified pane model.
2. History moved to dedicated `H` screen access.
3. Manual add/copy link actions were removed from Explorer UI.
4. Filter matching moved to shared fuzzy matcher path instead of regex-only UX flow.
5. Text entry moved to dedicated top input panel.

---

## Remaining Work (High Priority UX Iteration)
1. Keep per-feed sync failures log-only for now (no dedicated RSS error panel yet).
2. Tune visual density/readability in Explorer (long rows, badge clarity, truncation strategy).
3. Improve footer/help brevity and progressive hints for current focus/mode.
4. Refine focus indicators and active-pane affordances for low-contrast themes.
5. Add narrow-terminal behavior polish (minimum pane heights, overflow messaging).

## Remaining Work (Engineering Cleanup)
1. Add broader integration tests for full RSS auto-sync lifecycle (feed fetch -> filter match -> auto-ingest history row).
2. Add render/snapshot tests for long-row truncation and tiny-terminal fallback messaging.
3. Prune any remaining obsolete keybind text in non-RSS docs.

---

## Validation Snapshot
Recent targeted suites passing on branch:
- `cargo test --offline tui::screens::rss`
- `cargo test --offline tui::screens::help::tests::help_esc_returns_to_normal`
