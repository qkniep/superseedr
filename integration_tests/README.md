# Integration Tests Harness

Dockerized integration harness for cross-client torrent interoperability.

Current Phase 1 scope: `superseedr -> superseedr` (seed + leech).
Planned next scope: `superseedr <-> qbittorrent`, `superseedr <-> transmission`.

## Purpose

This harness exists to validate real interoperability behavior, not just unit-level correctness.

- Verify that one client can seed data that another client can fully download and validate.
- Catch protocol and metadata compatibility issues across torrent modes (`v1`, `v2`, `hybrid`).
- Produce deterministic artifacts (status snapshots, logs, validator reports) for local debugging and CI triage.
- Provide a stable adapter/scenario framework so additional clients can be added with minimal redesign.

## Test Design

Each mode run (`v1`, `v2`, or `hybrid`) follows the same design:

1. Generate deterministic fixture binaries and mode-specific torrent files with a local announce URL.
2. Create isolated runtime directories per run/mode (seed data, leech output, configs, logs).
3. Start Docker services in controlled order:
   - build image once
   - start tracker first and wait for readiness
   - start seed + leech clients
4. Poll client status periodically (currently Superseedr state JSON) and write normalized snapshots.
5. Validate leech output against expected filesystem manifest/hash from `integration_tests/test_data`.
6. Emit per-mode artifacts and final run summary.

Pass criteria:

- No missing files and no hash/content mismatches in the leech output.

Failure criteria:

- Timeout before convergence, or any missing/mismatched files.

## Requirements

- Docker + Docker Compose plugin (`docker compose`)
- Python 3.12+ (3.10+ may work, CI uses 3.12)
- Python deps from `requirements-integration.txt`
- Git checkout of this repo
- Optional: Rust/Cargo for broader project tests (`cargo test`) outside this harness

Install Python dependencies:

```bash
python3 -m pip install -r requirements-integration.txt
```

## Commands

### Main local entrypoint

```bash
./integration_tests/run_interop.sh [all|v1|v2|hybrid]
```

Environment variables:

- `INTEROP_TIMEOUT_SECS` (default `300`): per-mode timeout

Example:

```bash
INTEROP_TIMEOUT_SECS=300 ./integration_tests/run_interop.sh all
```

### Direct Python harness entrypoint

```bash
python3 -m integration_tests.harness.run \
  --scenario superseedr_to_superseedr \
  --mode all|v1|v2|hybrid \
  --timeout-secs 300 \
  [--run-id run_YYYYMMDD_HHMMSS] \
  [--skip-generation]
```

Accepted arguments:

- `--scenario`: currently only `superseedr_to_superseedr`
- `--mode`: `all`, `v1`, `v2`, `hybrid`
- `--timeout-secs`: timeout per mode in seconds
- `--run-id`: optional explicit run id
- `--skip-generation`: skip fixture/torrent regeneration

### Pytest wrapper

Unit tests (fast, no Docker):

```bash
python3 -m pytest integration_tests/harness/tests -m "not interop"
```

Interop tests via pytest (Docker):

```bash
RUN_INTEROP=1 INTEROP_TIMEOUT_SECS=300 \
python3 -m pytest integration_tests/harness/tests -m interop
```

## Artifacts and Monitoring

Per run output:

- `integration_tests/artifacts/runs/<run_id>/summary.json`
- `integration_tests/artifacts/runs/<run_id>/<mode>/validator_report.json`
- `integration_tests/artifacts/runs/<run_id>/<mode>/normalized_status.json`
- `integration_tests/artifacts/runs/<run_id>/<mode>/raw_client_status/*`
- `integration_tests/artifacts/runs/<run_id>/<mode>/logs/*`

Monitoring model:

- Superseedr is polled via its status JSON (`app_state.json`) and normalized into harness snapshots.
- Final pass/fail is determined by filesystem manifest/hash validation vs `integration_tests/test_data`.
- Tracker readiness is explicitly waited on before starting seed/leech services to reduce hybrid-mode flakes.

## CI

GitHub Actions workflow:

- `.github/workflows/integration-interop.yml`

Behavior:

- Runs matrix modes: `v1`, `v2`, `hybrid`
- Supports manual `workflow_dispatch` inputs:
  - `mode` (`all|v1|v2|hybrid`)
  - `timeout_secs`
- Uploads artifacts from `integration_tests/artifacts/`

## Current Status

As of February 12, 2026:

- Phase 1 (`superseedr -> superseedr`) harness is implemented.
- Modes `v1`, `v2`, and `hybrid` pass in local reruns.
- Hybrid startup race was mitigated by tracker-first startup + readiness wait.
- qBittorrent/Transmission adapters are present as explicit stubs (`NotImplementedError`).

## Plan / Next Tasks

1. Implement qBittorrent adapter (`integration_tests/harness/clients/qbittorrent.py`) with API auth, add torrent, status polling, and log collection.
2. Implement Transmission adapter (`integration_tests/harness/clients/transmission.py`) with RPC session handling, add torrent, status polling, and log collection.
3. Add mixed-client scenario modules under `integration_tests/harness/scenarios/`.
4. Extend compose stack for multi-client runs and keep deterministic fixture generation.
5. Add client-specific telemetry normalization and richer failure diagnostics.
6. Expand CI matrix to cover mixed-client scenarios once adapters are live.
