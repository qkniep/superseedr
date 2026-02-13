# Dockerized Integration Harness (Phase 1)

## Summary
Build a Python `pytest` harness that runs in Docker locally and in GitHub CI, starting with `superseedr -> superseedr`, and designed to add `qBittorrent`/`Transmission` via adapters.

## Key Decisions Locked
- Scope now: `superseedr x2` + pluggable adapter layer.
- Test framework: `pytest`.
- CI policy: `workflow_dispatch` + nightly (not PR-gating yet).
- Monitoring policy:
  - Use client-native telemetry for progress and early failure:
    - Superseedr: `status_files/app_state.json`
    - qBittorrent: Web API
    - Transmission: RPC API
  - Final pass/fail gate: filesystem hash validator only.
- Polling default: adaptive `1s` (active) to `5s` (stable).

## Public Interfaces
- Runner:
  - `python -m integration_tests.harness.run --scenario superseedr_to_superseedr --mode v1|v2|hybrid|all --timeout-secs <n>`
- Adapter base interface:
  - `start()`, `stop()`, `add_torrent(...)`, `wait_for_download(...)`, `collect_logs(...)`
- Normalized status outputs:
  - `integration_tests/artifacts/raw_client_status/*.json`
  - `integration_tests/artifacts/normalized_status.json`
  - `integration_tests/artifacts/validator_report.json`

## Planned Files
- `integration_tests/docker/docker-compose.interop.yml`
- `integration_tests/harness/run.py`
- `integration_tests/harness/docker_ctl.py`
- `integration_tests/harness/manifest.py`
- `integration_tests/harness/clients/base.py`
- `integration_tests/harness/clients/superseedr.py`
- `integration_tests/harness/clients/qbittorrent.py` (stub)
- `integration_tests/harness/clients/transmission.py` (stub)
- `integration_tests/harness/scenarios/superseedr_to_superseedr.py`
- `integration_tests/harness/tests/test_superseedr_interop.py`
- `requirements-integration.txt`
- `.github/workflows/integration-interop.yml`
- `docs/integration-harness.md`

## Compose Topology
- Services:
  - `tracker` (`:6969`)
  - `superseedr_seed`
  - `superseedr_leech`
- Isolated config/data mounts per instance.
- Shared torrent fixtures mount.
- Leech output mount for validation.
- Unique compose project per run for isolation.

## Data + Validation Flow
1. Generate/verify deterministic fixtures.
2. Generate deterministic torrents (v1/v2/hybrid) with announce `http://tracker:6969/announce`.
3. Start compose stack.
4. Seed container loads torrents and serves canonical data.
5. Leech container loads same torrents and downloads.
6. Observer loop collects telemetry + normalized status.
7. Final SHA-256 validator checks output tree vs canonical fixtures.
8. Always collect logs/artifacts on teardown.

## Test Matrix
- Scenario: `superseedr -> superseedr`
- Modes: `v1`, `v2`, `hybrid`
- Cases:
  - Happy path all modes
  - Timeout behavior
  - Determinism rerun
  - Intentional partial failure diagnostics
  - Adapter-stub contract behavior (clear not-implemented errors)

## Acceptance Criteria
- One local command runs full phase-1 harness.
- CI manual/nightly runs same harness.
- No machine-specific absolute paths required.
- Failures provide actionable artifacts (client raw status, normalized status, compose logs, validator diff).
- qBittorrent/Transmission can be added by implementing adapters + scenarios, no core harness redesign.

## Assumptions
- Linux CI (`ubuntu-latest`) for integration workflow.
- Existing Rust app code unchanged unless non-interactive runtime issues force container command wrapping.
