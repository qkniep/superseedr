# Torrent Remove/Delete Lifecycle Plan

## Status Snapshot (2026-03-02)
This plan captures a release-deferred cleanup for torrent removal semantics. The current behavior is close enough to ship, but there is a lifecycle mismatch around torrent removal, transient `Deleting` UI state, and persistence across app shutdown/restart.

## Problem Summary

### Reported symptom
Sometimes a user deletes a torrent and sees it remain stuck in red (`Deleting`). After restart, the torrent may still be present and get loaded again.

### What the code currently does
1. The delete confirm UI marks the torrent as `TorrentControlState::Deleting` immediately.
2. The UI then sends one of two manager commands:
   - `ManagerCommand::DeleteFile` for "remove and delete files"
   - `ManagerCommand::Shutdown` for "remove from client, keep files"
3. The app only removes a torrent from:
   - `app_state.torrents`
   - `client_configs.torrents`
   - persisted settings
   after receiving `ManagerEvent::DeletionComplete`.

### Root causes
1. The user-facing command model is semantically inconsistent:
   - `DeleteFile` means "remove torrent and delete files"
   - `Shutdown` is being reused to mean "remove torrent and keep files"
2. App shutdown persists settings before shutdown cleanup completes.
3. During app shutdown, `ManagerEvent::DeletionComplete` is counted for progress, but it is not routed through the normal torrent-removal cleanup path.

## Why It Reappears On Restart
Startup reloads torrents from persisted `client_configs.torrents`. If a torrent is still present in config when the app exits, it will be reconstructed on the next boot. A transient `Deleting` UI state does not remove the torrent from config by itself.

## Goal
Make torrent removal lifecycle explicit and unified:
1. "Remove torrent, keep files" and "remove torrent, delete files" must use one command family.
2. Both paths must end in the same completion event and app-side cleanup path.
3. Shutdown-time completion events must update persisted state before exit.

## Scope

### In scope
1. Manager command model for torrent removal.
2. Delete confirm UI wiring.
3. App-side cleanup path for `ManagerEvent::DeletionComplete`.
4. Shutdown sequencing so completed removals are reflected in persisted settings.
5. Regression tests for remove/delete/restart behavior.

### Out of scope
1. Broader torrent lifecycle refactors unrelated to removal.
2. Changes to file deletion safety rules.
3. New UI affordances or copy changes beyond what is required for the command rename.

## Recommended Design

### 1. Replace the user-facing dual command model with one explicit removal command
Add a new manager command:

- `ManagerCommand::Remove { delete_files: bool }`

Behavior:
1. `delete_files: true`
   - perform current delete flow
   - delete managed torrent files/directories when safe
   - emit `ManagerEvent::DeletionComplete`
   - exit the manager loop
2. `delete_files: false`
   - perform current shutdown/removal flow
   - do not delete data files
   - still emit `ManagerEvent::DeletionComplete`
   - exit the manager loop

### 2. Keep `ManagerCommand::Shutdown` for app shutdown only
`ManagerCommand::Shutdown` should continue to mean:
- stop torrent activity
- send stop announces
- shut down the manager because the entire app is exiting

It must not remain a user-facing "remove torrent but keep files" command.

### 3. Make the UI fully mechanical
Delete confirm should map as follows:
1. `d` -> `ManagerCommand::Remove { delete_files: false }`
2. `D` -> `ManagerCommand::Remove { delete_files: true }`

The UI may continue to set `TorrentControlState::Deleting` immediately for feedback, but that state must remain transient and must not be relied on for actual removal.

### 4. Preserve one app-side cleanup path
The only code that should remove a torrent from app/config state should remain the existing completion-based path triggered by `ManagerEvent::DeletionComplete`.

That path should continue to:
1. remove the torrent from `app_state.torrents`
2. remove manager channels/watchers
3. remove the entry from `client_configs.torrents`
4. persist updated state

No app-side special case should be added for "remove while keeping files."

### 5. Fix shutdown persistence ordering
Current shutdown sequence persists settings before manager shutdown completion has been fully applied.

Required change:
1. when shutdown begins, send `ManagerCommand::Shutdown` to all managers
2. while waiting for managers to complete, route any `ManagerEvent::DeletionComplete` through the same cleanup logic used during normal runtime
3. only after shutdown cleanup has been processed, persist final settings/state
4. then flush persistence writer and exit

This ensures:
1. a torrent removed just before app exit is not written back into config
2. shutdown progress accounting still works

## Implementation Notes

### `src/torrent_manager/mod.rs`
1. Add `ManagerCommand::Remove { delete_files: bool }`.
2. Keep `ManagerCommand::Shutdown` for whole-app shutdown semantics.
3. Remove `DeleteFile` if no longer needed after migration.

### `src/torrent_manager/manager.rs`
1. Handle `ManagerCommand::Remove { delete_files }` in the main manager loop.
2. Route:
   - `delete_files: true` -> current `Action::Delete`
   - `delete_files: false` -> current `Action::Shutdown`
3. After applying the action, continue to break out of the manager loop once the action has been initiated, as today.

### `src/tui/screens/delete_confirm.rs`
1. Replace the current command mapping:
   - `with_files: true` -> `ManagerCommand::Remove { delete_files: true }`
   - `with_files: false` -> `ManagerCommand::Remove { delete_files: false }`
2. Keep `MarkDeleting` unless it causes undesirable flicker.

### `src/app.rs`
1. Extract the existing `ManagerEvent::DeletionComplete` cleanup body into a helper so both:
   - normal runtime event handling
   - shutdown wait loop
   can reuse it.
2. In shutdown wait loop, when `ManagerEvent::DeletionComplete` arrives:
   - apply cleanup helper
   - increment shutdown completion counter
3. Move or repeat final `save_state_to_disk()` after manager shutdown completions have been processed.

## Backward Compatibility / Migration
This is an internal command refactor only.

No config migration is required because persisted torrent data should continue to use the existing `TorrentSettings` shape.

Optional hardening:
1. avoid persisting `TorrentControlState::Deleting`
2. map it to `Paused` or omit it entirely if a torrent survives to persistence unexpectedly

This is not required for the main fix, but it is a useful defense-in-depth guard.

## Risks
1. Any tests that currently assume `ManagerCommand::Shutdown` is the user-facing "safe remove" path will need to be updated.
2. Shutdown behavior must continue to count manager completions correctly after routing completion events through the cleanup helper.
3. If cleanup helper performs persistence too often during shutdown, it may need a final coalesced save rather than multiple intermediate writes.

## Why This Is Safer Than App-Side Special Casing
1. It preserves the current event-driven ownership model.
2. It avoids splitting removal logic between app and manager.
3. It keeps file deletion and non-deleting removal as two variants of the same lifecycle.
4. It reduces the chance of future regressions where one path updates config and the other only updates UI.

## Test Plan

### Unit / integration coverage
1. Confirm `d` sends `ManagerCommand::Remove { delete_files: false }`.
2. Confirm `D` sends `ManagerCommand::Remove { delete_files: true }`.
3. Confirm remove-without-files emits `ManagerEvent::DeletionComplete(Ok(()))`.
4. Confirm remove-with-files emits `ManagerEvent::DeletionComplete` after file deletion attempt.
5. Confirm app runtime handling removes the torrent from:
   - `app_state.torrents`
   - `client_configs.torrents`
   - manager channel maps
6. Confirm shutdown loop also applies the same cleanup when `DeletionComplete` arrives there.
7. Confirm a removed torrent is not reloaded on next startup.

### Manual verification
1. Remove a torrent with `d`; verify UI entry disappears and files remain on disk.
2. Remove a torrent with `D`; verify UI entry disappears and managed files are deleted.
3. Trigger remove, then quit the app immediately; verify the torrent does not return after restart.
4. Remove while the torrent is active/downloading; verify row may briefly turn red, then disappears.

## Acceptance Criteria
1. `d` removes the torrent from the client while keeping files.
2. `D` removes the torrent from the client and deletes files.
3. Both paths converge on `ManagerEvent::DeletionComplete`.
4. Removed torrents do not reappear after restart.
5. `ManagerCommand::Shutdown` remains reserved for whole-app shutdown behavior.
