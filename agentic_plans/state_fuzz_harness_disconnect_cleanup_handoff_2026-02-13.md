# State Fuzz Harness Handoff: Disconnect/Cleanup Fidelity + Remaining Liveness Bug

## Context
We were stabilizing this property test:
- `torrent_manager::state::prop_tests::fuzz_piece_block_selection_and_completion`
- Located in `src/torrent_manager/state.rs`

Original issue observed by user:
- Proptest aborted with `Too many global rejects` from `prop_assume!(progressed || !pending_actions.is_empty())`.

## Design Decisions Made
1. **Treat stall as deterministic failure, not reject noise**
- Removed reject-based branch and switched to hard `prop_assert!` with repro context.
- Goal: get deterministic failing seeds/cases instead of global reject quota aborts.

2. **Keep work in state-level tests (no full integration harness)**
- Added a lightweight production-flow shim in the existing property harness:
  - Simulated manager command queue for disconnect flow.
  - Periodic `Action::Cleanup` injection based on virtual time.

3. **Prefer production-like disconnect handling**
- `Effect::DisconnectPeer` is translated to manager command and then to:
  - `Action::PeerDisconnected { peer_id, force: false }`
- This preserves batching semantics instead of forcing immediate disconnect.

## What Was Implemented
All edits are in `src/torrent_manager/state.rs` (prop test module area).

### A) Removed reject-based stall policy
- Removed `allow_stall_reject` from `FuzzHarnessConfig`.
- Replaced conditional `prop_assume!/prop_assert!` with a single assert.

### B) Added state-test manager shim
- Added config fields:
  - `manager_delivery_batch_max`
  - `simulated_tick_ms`
  - `cleanup_interval_ms`
- Added local enum:
  - `SimulatedManagerCommand::Disconnect(String)`
- Extended `enqueue_from_effect(...)` to accept manager queue and translate:
  - `Effect::DisconnectPeer { peer_id }` -> enqueue `SimulatedManagerCommand::Disconnect(peer_id)`

### C) Extended main harness loop behavior
- Added `pending_manager_commands: Vec<SimulatedManagerCommand>`.
- Added manager command delivery loop that applies:
  - `Action::PeerDisconnected { peer_id, force: false }`
- Added virtual time and periodic cleanup:
  - `elapsed_ms += simulated_tick_ms`
  - trigger `state.update(Action::Cleanup)` when elapsed reaches cleanup boundary

### D) Handshake simulation improvement
- On peer setup, after `PeerSuccessfullyConnected`, now also applies:
  - `Action::UpdatePeerId { peer_addr, new_id }`
- This avoids cleanup treating all peers as “stuck” due to empty peer IDs.

### E) Improved assertion diagnostics
Final stall assert now reports:
- `pieces_remaining`
- `pending_actions`
- `pending_manager_commands`
- `need_queue` len
- `pending_queue` len
- `queued_piece_count`
- `has_serviceable_piece`
- `peers`
- `seed`
- `loop_guard`

## Current Outcome
The harness-level bug (reject-abort noise) is fixed, but test now exposes a **real deterministic liveness issue**.

Latest failing profile:
- `pieces_remaining=1`
- `need_queue=0`
- `pending_queue=1`
- `pending_actions=0`
- `pending_manager_commands=0`
- `has_serviceable_piece=true`

Meaning:
- A piece can remain globally pending with no in-flight simulated work/events despite being serviceable by peers.
- This is consistent with a queue/liveness bug in state logic (not just harness modeling error).

## Known Repro Seeds Seen During Work
Examples seen in failures (not exhaustive):
- `random_seed = 16521762201929936452` (V2 case; strong diagnostic signal)
- Earlier failures before/while harness changes: `8438808584678952797`, `3400861518042494735`

## Validation Commands Used
Primary:
```bash
cargo test -q torrent_manager::state::prop_tests::fuzz_piece_block_selection_and_completion -- --nocapture
```

## Next Steps (Implementation)
1. **Trace pending-piece lifecycle invariants around stall point**
- Focus on transitions involving:
  - `Action::AssignWork`
  - `Action::PieceVerified`
  - `Action::PieceWrittenToDisk`
  - `Action::PeerDisconnected`
  - `piece_manager.pending_queue` / `need_queue`

2. **Add targeted assertion/trace near assignment logic**
- Detect when a piece exists in `pending_queue` but no peer can actively make progress on it.
- Confirm whether peer-local `pending_requests` and global `pending_queue` diverge.

3. **Fix liveness condition in state logic**
- Candidate fix direction:
  - Requeue or re-eligibilize pending pieces when no active owner peer remains able to service them.
  - Ensure `AssignWork` can always regenerate work for serviceable pending pieces in Endgame/standard paths.

4. **Add regression test(s)**
- Add deterministic repro test (fixed case + seed) for this stall pattern.
- Keep original property test as broad fuzz coverage.

5. **Re-run test matrix**
- Re-run target property test repeatedly.
- Run nearby `state.rs` prop/unit tests to ensure no behavior regressions.

## Notes for Resume
- Working tree currently has only this modified file from this task:
  - `src/torrent_manager/state.rs`
- `proptest-regressions/torrent_manager/state.txt` was auto-touched during failures and then restored to avoid unrelated noise.
