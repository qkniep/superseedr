# Integration E2E Automation Plan

## Goal
Build a repeatable, one-command end-to-end test pipeline that:
- Generates deterministic fixture data.
- Produces torrent files (including non-aligned piece-length cases).
- Runs qBittorrent-based download/seeding tests against `integration_tests/settings.toml`.
- Verifies output content against canonical fixtures.
- Cleans up outputs between runs.

This document captures the current manual foundation and a concrete plan to automate it.

## Current Foundation (Completed)
- Fixture generation script:
  - `scripts/generate_integration_bins.py`
  - Generates deterministic test data in `integration_tests/test_data`.
- Output validation script:
  - `scripts/validate_integration_output.py`
  - Compares `integration_tests/test_output/<mode>` to canonical `integration_tests/test_data` by size + SHA-256.
  - Supports mode-aware validation and v1-only expectation for `single_25k.bin`.
- Output cleanup script:
  - `scripts/clear_integration_output.py`
  - Clears generated outputs while preserving the `v1`, `v2`, `hybrid` scaffold.
- Torrent/settings setup:
  - `integration_tests/torrents/{v1,v2,hybrid}` populated.
  - `integration_tests/settings.toml` populated with torrent entries.
  - Added non-aligned test case:
    - `integration_tests/test_data/single/single_25k.bin`
    - `integration_tests/torrents/v1/single_25k.bin.torrent` with `piece length = 20000`.
- Repo hygiene:
  - `integration_tests/test_output/` ignored in `.gitignore`.

## Gaps To Automate
- No single orchestrator command (current flow is multiple scripts + manual qB steps).
- Torrent generation is partly manual (especially non-aligned edge torrents).
- qBittorrent execution/verification still manual.
- No CI matrix execution for v1/v2/hybrid with clear pass/fail gating.
- No automatic mapping from `.torrent` to expected output topology.

## Target End State
One command, e.g.:

```bash
python3 scripts/run_integration_e2e.py --mode all --client qbittorrent
```

Behavior:
1. Clears old output.
2. Regenerates deterministic fixtures.
3. Regenerates torrents (including edge/non-aligned cases).
4. Generates/refreshes `integration_tests/settings.toml`.
5. Launches qBittorrent test run (or API-driven variant).
6. Waits for completion with timeout + progress logs.
7. Runs strict output validator.
8. Exits non-zero on any mismatch or timeout.

## Implementation Plan

### Phase 1: Standardize Data and Torrent Generation
1. Add `scripts/generate_integration_torrents.py`.
2. Input source: `integration_tests/test_data`.
3. Output targets:
   - `integration_tests/torrents/v1`
   - `integration_tests/torrents/v2`
   - `integration_tests/torrents/hybrid`
4. Include explicit edge-case profiles:
   - aligned baseline (`piece length = 16384`)
   - non-aligned v1 case (`piece length = 20000`, currently `single_25k.bin`)
5. Add `--verify` mode to confirm torrent metadata constraints (piece length, file list, hashes count).

Acceptance:
- Re-running generation is idempotent.
- Non-aligned torrent metadata is reproducible and validated.

### Phase 2: Generate Settings from Source of Truth
1. Add `scripts/generate_integration_settings.py`.
2. Build `integration_tests/settings.toml` from discovered torrents and mode rules.
3. Avoid manual drift by deriving `download_path` conventionally:
   - `integration_tests/test_output/<mode>/...`
4. Preserve deterministic ordering for stable diffs.

Acceptance:
- `settings.toml` can be regenerated without manual edits.
- Every `.torrent` has a corresponding `[[torrents]]` entry.

### Phase 3: qBittorrent Orchestration
1. Add `scripts/run_qb_integration.py`.
2. Modes:
   - `seed` mode for loading canonical data set.
   - `download` mode for writing into `integration_tests/test_output`.
3. Use qBittorrent Web API for automation:
   - Auth, add torrents, set save path, start/stop torrents, query completion state.
4. Add robust timeout + retries + per-torrent status diagnostics.
5. Record run artifact log:
   - `integration_tests/test_output/_run_logs/<timestamp>.log`

Acceptance:
- Script can execute full run without UI clicks.
- Failures identify torrent/hash/save-path mismatch quickly.

### Phase 4: End-to-End Runner and Exit Codes
1. Add `scripts/run_integration_e2e.py` orchestrator.
2. Sequence:
   - clear -> generate bins -> generate torrents -> generate settings -> qb run -> validate
3. Strict exit policy:
   - non-zero on setup failure, timeout, validation mismatch, or missing expected files.
4. Add CLI:
   - `--mode v1|v2|hybrid|all`
   - `--skip-seed`
   - `--timeout-secs`
   - `--allow-extra` (for debugging only)

Acceptance:
- One command executes entire flow and gates success.

### Phase 5: CI Integration
1. Add CI workflow target (manual trigger first, then scheduled/nightly).
2. Gate on:
   - fixture verify
   - settings generation diff clean
   - e2e pass
3. Collect and upload artifacts:
   - validator logs
   - qB run logs
   - summary JSON

Acceptance:
- CI reliably surfaces regressions without local-only steps.

## Test Matrix

### Data Variants
- `single_4k.bin`, `single_8k.bin`, `single_16k.bin`, `single_25k.bin`
- `multi_file` set
- `nested` set

### Protocol Variants
- v1
- v2
- hybrid

### Edge Cases
- Non-aligned piece-length torrent:
  - v1 `single_25k.bin.torrent` (`piece length = 20000`)
- Tail piece / partial-piece verification
- Nested directory path fidelity
- Duplicate filename handling across modes

## Operational Notes
- qBittorrent state paths observed on this machine:
  - `~/Library/Application Support/qBittorrent/BT_backup/`
  - `~/Library/Preferences/qBittorrent/`
  - `~/Library/Preferences/org.qbittorrent.qBittorrent.plist`
- Per-torrent save paths are stored in `.fastresume` (`qBt-savePath` / `save_path`).

## Risks and Mitigations
- Risk: qBittorrent piece-size UI limitations (16 KiB multiples only).
  - Mitigation: generate non-aligned torrents directly via script/bencode.
- Risk: settings and torrent drift over time.
  - Mitigation: generated settings + generation verify checks.
- Risk: hidden macOS files (`.DS_Store`) pollute validation.
  - Mitigation: validators ignore hidden files and cleanup script removes outputs.
- Risk: mode-specific expectations evolve (e.g., v1-only edge cases).
  - Mitigation: keep explicit mode override rules in validator generator config.

## Immediate Next Implementation Steps
1. Implement `generate_integration_torrents.py`.
2. Implement `generate_integration_settings.py`.
3. Add `run_qb_integration.py` API automation.
4. Add `run_integration_e2e.py` orchestrator.
5. Add CI workflow for manual dispatch and artifact upload.

## TODO From Current Review
1. Remove machine-specific absolute paths from `integration_tests/settings.toml` and switch to repo-relative or env-derived roots for portability.
2. Replace hardcoded validator mode exception (`v1`-only `single_25k.bin`) with settings/manifests-driven expected-file derivation.
3. Document and/or formalize fixture seed compatibility mapping in `generate_integration_bins.py` (nested path alias behavior).
4. Add smoke tests for:
   - `scripts/generate_integration_bins.py`
   - `scripts/validate_integration_output.py`
   - `scripts/clear_integration_output.py`
5. Improve cleanup script dry-run output so `WOULD_PRUNE` only reports directories that would actually be empty.
