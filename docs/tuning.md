# Tuning Design Notes

## Purpose

Document the self-tuning control loop and define a refactor path that improves algorithm agility before changing runtime behavior.

## Current Implementation Snapshot

- Tuning cadence is fixed at 15 minutes in the app loop.
- Tuning score uses a 60-second lookback of throughput history.
- Each cycle evaluates the current candidate, may revert to last best limits, then applies a new random adjustment.
- Mode switches (leeching <-> seeding) reset tuning score state.
- Countdown and cadence assumptions are currently hardcoded in multiple places.

## Agreed Direction

Refactor first, behavior changes second.

1. Extract a `TuningController` with a clear API and internal state.
2. Move cadence/window/countdown policy into that controller.
3. Keep existing 60/900 behavior as the initial policy to preserve parity.
4. Add tests around controller behavior and invariants.
5. Only after parity is proven, enable adaptive cadence/window policy.

## Refactor Goals (No Behavior Change)

- Single source of truth for:
  - cadence seconds
  - lookback window seconds
  - countdown state
  - last best score/limits tracking
- `app.rs` should call controller methods rather than owning tuning constants directly.
- `ui_telemetry.rs` should not own tuning cadence assumptions.
- Existing score math and random adjustment logic should remain functionally equivalent during this phase.

## Planned Adaptive Policy (Post-Refactor)

### Core Ideas

- Use exponential backoff when no improvement is observed for consecutive cycles.
- Speed up cadence on rapid regression or strong penalty spikes.
- Keep lookback window and cadence linked (`cadence >= window`) to avoid noisy comparisons.
- On mode switch, bootstrap with fast cadence for a few cycles, then let adaptive control settle.

### Guardrails

- Clamp cadence to bounded range (example: 10s..180s).
- Clamp window to bounded range (example: 15s..60s).
- Require minimum improvement threshold before accepting a new best score.
- Add cooldown/hysteresis to avoid tuning thrash.

## Why This Matters

- Short, high-throughput torrents can complete before a slow fixed cadence adapts.
- Long-running seeding workloads benefit from backing off when stable.
- A controller abstraction makes policy changes local, testable, and reversible.

## Acceptance Criteria For Refactor Phase

- Existing fixed behavior is preserved by default (parity mode).
- Tuning constants are no longer hardcoded across unrelated modules.
- Countdown display is driven by controller state.
- Unit tests cover:
  - fixed-policy parity
  - mode-switch reset behavior
  - controller state transitions

## Next Steps

1. Introduce `TuningController` types and wire-up with fixed policy.
2. Move countdown ownership into controller.
3. Add parity tests against current behavior.
4. Add adaptive policy behind a feature flag or config toggle.
