# CLI And Shared Config Agent Validation Plan

## Summary
Use an AI agent to run an end-to-end validation sweep for the new CLI control surface and layered shared-config behavior. The agent should create an isolated scratch workspace under `tmp/`, launch one disposable Superseedr instance against that workspace, drive the new CLI commands, mutate shared config files when needed, and validate outcomes using:

- `superseedr status`
- `status_files/app_state.json`
- `superseedr journal`
- shared-config files on disk

The agent must produce a final report that records every step as pass or fail, and for failures it must capture why the step failed, what evidence was collected, and whether the failure looks like an environment/setup issue or an application defect.

## Scope
This plan covers only the branch areas that added or materially changed:

- CLI control commands
  - `status`
  - `status --follow`
  - `status --stop`
  - `pause`
  - `resume`
  - `delete`
  - `priority`
  - `journal`
- Online command delivery through watch folders and `.control` files
- Offline CLI behavior that edits settings directly
- Layered shared-config mode
  - `SUPERSEEDR_SHARED_CONFIG_DIR`
  - `SUPERSEEDR_HOST_ID`
  - shared `settings.toml`
  - shared `catalog.toml`
  - host-local `hosts/<host-id>.toml`
  - single-host shared-config live reload and reconcile
  - stale-write protection

Do not spend time on unrelated TUI-only feature validation unless it is directly required to unblock a CLI/shared-config scenario.

Cross-host propagation between multiple simultaneously running local instances is out of scope for this automated plan. The normal local app data directory owns the lock file, so multi-instance live-sync coverage should be validated manually in a genuinely isolated environment.

## Local Runtime Note
Even in shared-config mode, several runtime artifacts remain in the normal local app data directory rather than under the scratch shared root. The agent must treat these as local runtime outputs and copy them into the scratch evidence directory when needed.

These include:

- `status_files/app_state.json`
- `event_journal.toml`
- logs
- lock file

The agent should resolve the actual local app data directory first, then read or copy these files from there during validation.

## Safety Rails
The agent must follow these safeguards before running any test:

1. Refuse to run if another `superseedr` process is already active outside the test plan.
2. Use a dedicated scratch root under `tmp/` and never write test artifacts outside that root unless the app itself requires OS-local config/data paths.
3. Before launching the app, detect the normal Superseedr OS config/data directories. If they already contain user data, back them up into the scratch root and restore them during cleanup.
4. Use a dedicated host ID and client port for the test instance.
5. Never use destructive git commands.
6. Treat all failures as evidence first. Do not patch code during the run. Record the failure and continue unless the environment is unusable.
7. Record the resolved local app data path early in the report so later steps know where `status_files/`, the event journal, and logs actually live.

## Scratch Layout
Create a unique run root:

```text
tmp/cli_shared_config_validation_<timestamp>/
```

Inside it create:

```text
bin/
evidence/
evidence/logs/
evidence/status/
evidence/journal/
evidence/shared_snapshots/
evidence/commands/
reports/
run/
run/shared-root/
run/shared-root/hosts/
run/shared-root/torrents/
run/host-a-watch/
run/host-a-downloads/
run/os-config-backup/
run/os-data-backup/
```

## Test Fixtures
Use existing tracked fixtures from the repo and copy them into the scratch root:

- `integration_tests/torrents/v1/single_4k.bin.torrent`
- `integration_tests/torrents/v1/single_8k.bin.torrent`

Copy them into:

```text
tmp/.../run/shared-root/torrents/
```

Rename them to stable shared names if helpful:

- `alpha.torrent`
- `beta.torrent`

Using two torrents is important because one scenario needs a second live torrent to trigger an unrelated save while validating shared-catalog removal behavior.

## Build And Launch Strategy
1. Build the binary once:
   - `cargo build`
2. Use the built binary for all commands:
   - `target/debug/superseedr`
3. Launch the runtime instance with `SUPERSEEDR_SHARED_CONFIG_DIR` and `SUPERSEEDR_HOST_ID` set.
4. Prefer detached/background process launch so the agent can keep issuing CLI commands.
5. Record stdout/stderr for the launched instance into `evidence/logs/`.

If detached launch is not reliable in the current environment, the agent may use a second terminal session or platform-equivalent background process runner, but it must still preserve the same evidence layout.

## Shared Config Seed Files
Create these files before the first launch.

### Shared `settings.toml`
Use values that make CLI/status validation easier:

- `output_status_interval = 0`
- `bootstrap_nodes = []`
- `default_download_folder` should point at the scratch downloads root or use portable paths if the test explicitly wants to exercise portable roots
- keep RSS empty

### Shared `catalog.toml`
Seed two torrents:

- `alpha`
- `beta`

Both should point at the copied `.torrent` files under the shared root. Set:

- `torrent_control_state = "Running"`
- `container_name = ""`
- `validation_status = false`
- `file_priorities` with only `0 = "Normal"`

### Host file
Create:

- `hosts/host-a.toml`

Set:

- `client_port`
- host-specific `watch_folder`
- any required `path_roots`

## Evidence Rules
For each test step, the agent must capture:

1. The exact command(s) run.
2. The relevant environment variables.
3. The pre-state snapshot.
4. The post-state snapshot.
5. The pass/fail decision.
6. If fail:
   - observed behavior
   - expected behavior
   - likely failure class
     - setup error
     - test harness issue
     - product bug

At minimum, persist:

- raw `status` JSON outputs
- copies of `status_files/app_state.json`
- `superseedr journal` output after mutating steps
- copies of shared `settings.toml`, `catalog.toml`, and the host file before and after each shared-config test

## Validation Heuristics
Use the following sources of truth:

- CLI success text confirms request acceptance, not final correctness
- `superseedr status` confirms live or offline resolved state
- `status_files/app_state.json` confirms daemon-observed runtime state
- shared config files confirm persistence/routing behavior
- `superseedr journal` confirms queue/applied/failed recording

Prefer JSON/file evidence over console prose when deciding pass or fail.

## Run List

### Phase 0: Environment Preparation
1. Create the scratch root under `tmp/`.
2. Build `superseedr`.
3. Detect the normal OS config/data locations used by Superseedr.
4. If those locations already contain files, copy them into `run/os-config-backup/` and `run/os-data-backup/`.
5. Seed the shared config files and copy the two torrent fixtures into the shared root.
6. Record the resolved local app data path and local config path in the report.
7. Snapshot the initial shared config files into `evidence/shared_snapshots/phase0_*`.

Pass criteria:
- scratch root exists
- binary builds
- shared files are valid TOML
- no existing user state is lost

### Phase 1: Shared Config Bootstrap And Single-Host Sanity
1. Launch host A with:
   - `SUPERSEEDR_SHARED_CONFIG_DIR=<scratch shared-root>`
   - `SUPERSEEDR_HOST_ID=host-a`
2. Wait for `status_files/app_state.json` to appear.
3. Run `superseedr status` against host A's shared env.
4. Validate:
   - both torrents are present
   - info hashes are visible in status output
   - host A is using the expected client port
   - `output_status_interval` is initially disabled until explicitly requested
   - the local app data directory contains the expected runtime `status_files/app_state.json`

Pass criteria:
- host A starts successfully
- both catalog entries load
- status JSON matches seeded shared config

### Phase 2: Online CLI Status Controls
1. Run `superseedr status`.
2. Save the JSON output.
3. Run `superseedr status --follow`.
4. Observe `status_files/app_state.json` modification times for at least three updates.
5. Run `superseedr status --stop`.
6. Confirm status file updates stop after a grace period.

Pass criteria:
- `status` returns fresh JSON
- `--follow` causes repeated file updates
- `--stop` halts repeated updates

Failure notes:
- If `status` works but file updates do not continue, classify as runtime follow bug.
- If `--stop` is accepted but updates continue, classify as runtime stop bug.

### Phase 3: Online CLI Pause/Resume/Priority/Delete
Use host A while it is running.

1. From `status`, capture the `info_hash_hex` for `alpha` and `beta`.
2. Run `pause <alpha-hash>`.
3. Validate through `status` or `app_state.json` that `alpha` is paused.
4. Run `resume <alpha-hash>`.
5. Validate it returns to running.
6. Run `priority <alpha-hash> --file-index 0 skip`.
7. Validate persisted/configured file priority changed.
8. Run `priority <alpha-hash> --file-index 0 normal`.
9. Validate the override is removed or reset.
10. Run `delete <beta-hash>`.
11. Validate `beta` is removed from runtime and shared catalog.
12. Run `superseedr journal`.
13. Validate control entries include queued/applied records for the online actions.

Pass criteria:
- runtime state changes match each CLI action
- persistence matches runtime state
- journal records exist

### Phase 4: Offline CLI Behavior
1. Stop host A cleanly.
2. Run offline commands against the same shared root and host ID:
   - `status`
   - `pause <alpha-hash>`
   - `resume <alpha-hash>`
   - `priority <alpha-hash> --file-index 0 skip`
   - `priority <alpha-hash> --file-index 0 normal`
3. After each mutation, inspect shared config files directly.
4. Run `superseedr journal` and save output.

Expected behavior:
- `status` should return offline JSON
- pause/resume/priority should edit settings directly
- journal should record offline applied or failed entries

Pass criteria:
- offline mutations persist without a running daemon
- offline status succeeds
- journal evidence exists for offline actions

### Phase 5: Shared Config Live Remove Without Resurrection
This phase explicitly targets the removal regression.

1. Ensure both `alpha` and `beta` exist and host A is running.
2. Remove `alpha` from the shared catalog by editing `catalog.toml` externally.
3. Validate host A observes the removal and begins local teardown.
4. Before teardown fully settles, trigger an unrelated persisted save from host A by mutating `beta`:
   - `pause <beta-hash>`
   - or `resume <beta-hash>`
   - or file priority change
5. Snapshot `catalog.toml` after host A's save.
6. Validate `alpha` does not reappear in `catalog.toml`.

Pass criteria:
- removed torrent stays removed
- unrelated save does not resurrect the deleted entry

If fail:
- record the exact shared catalog contents before remove, after remove, and after host A save
- classify as shared-catalog resurrection bug

### Phase 6: Shared Config Updated-But-Missing Runtime Case
This phase explicitly targets the missing-runtime update regression.

1. Stop host A.
2. Configure host A so one seeded torrent cannot load on startup:
   - easiest path: make `alpha` point at a missing `.torrent` file in the shared catalog before launching host A
3. Launch host A and verify `alpha` is absent from runtime while still present in shared config.
4. Without restarting host A, repair the catalog entry so it points at a valid shared torrent file and also change one other field to guarantee a diff:
   - name
   - pause/resume state
   - file priority
5. Trigger shared-config reload by writing the updated `catalog.toml`.
6. Validate whether host A loads `alpha` live.

Pass criteria:
- host A loads the previously missing runtime torrent after the update diff

If fail:
- record that the catalog entry exists in both old and new config but runtime stayed absent until restart
- classify as updated-entry missing-runtime reconcile bug

### Phase 7: Stale-Write Protection
1. Keep host A running.
2. Externally edit shared `settings.toml` or `catalog.toml`.
3. Without reloading first, trigger a persisted change from host A.
4. Validate the save is rejected and the app reports reload is required.
5. Confirm the external edit was not overwritten.

Pass criteria:
- conflicting save is rejected
- on-disk shared file keeps the external edit intact

### Phase 8: Watch-Folder Delivery For Online CLI
This phase verifies the CLI-to-daemon online control path, not generic ingest coverage.

1. While host A is running, capture the host A watch folder contents.
2. Run one online CLI control command.
3. Confirm a `.control` file appears and is then archived/renamed after processing.
4. Confirm the requested action is applied.
5. Repeat once with `SUPERSEEDR_WATCH_PATH_1` configured for host A to confirm extra watch-path discovery does not break the primary command path.

Pass criteria:
- CLI writes go to the primary command watch path
- running daemon consumes the control file
- processed artifact cleanup occurs

## Failure Classification
Use these labels in the report:

- `ENVIRONMENT`
  - binary could not launch
  - background process strategy failed
  - permissions/path issue unrelated to app behavior
- `HARNESS`
  - agent could not reliably capture evidence
  - timing window too narrow or script bug
- `PRODUCT`
  - app behavior disagrees with the documented branch intent

## Required Report Outputs
Write:

- `reports/summary.md`
- `reports/results.json`

### `summary.md`
Include:

- overall verdict
- environment summary
- list of phases with pass/fail
- concise explanation of each failure
- high-confidence suspected regressions

### `results.json`
One object per phase with:

- `phase`
- `status`
- `commands`
- `artifacts`
- `observed`
- `expected`
- `classification`

## Cleanup
At the end of the run:

1. Stop all spawned Superseedr instances.
2. Restore any pre-existing OS config/data directories from backup if they were backed up.
3. Leave the scratch root under `tmp/` intact for inspection.

## Success Definition
This validation pass is successful when:

1. The agent completes every phase or records a clear reason it could not.
2. All evidence artifacts are saved under the scratch root.
3. CLI behavior is validated both online and offline.
4. Single-host shared-config live update/remove semantics are validated through external file edits and reload.
5. The final report clearly distinguishes environment problems from product bugs.
