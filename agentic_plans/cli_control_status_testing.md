# Shared-Config CLI Feature Validation Matrix: codex/unified-config

## Purpose

This is a focused validation plan for the current shared-config CLI surface in this branch.

It is not a full regression plan.

This plan validates:
- normal offline CLI behavior
- shared-config activation and precedence
- launcher shared-config commands
- launcher host-id commands
- standalone/shared conversion commands
- shared-mode read and mutating CLI commands
- shared offline CLI behavior with no leader running
- optional concurrent leader/follower shared behavior
- node-cluster failover behavior after leadership transfer
- docs matching the current CLI and shared-layout behavior

This plan does not require:
- a full download lifecycle
- tracker correctness
- deep TUI walkthroughs outside journal/status spot checks

## Core Execution Rule

- Test the checked-out code with `cargo run`, not an installed global binary.
- Prefer `cargo run -- <args>` for all CLI validation.
- Prefer env-prefixed `cargo run -- <args>` for shared-mode validation.
- Do not assume an old launcher sidecar or a previously running runtime reflects the intended test setup.

## Workspace And Shared Root Rules

- Use `./tmp/` as the default shared mount root.
- Treat `./tmp/` as both scratch space and the shared-root mount for the local round.
- Do not scatter temporary artifacts elsewhere in the repo.
- Do not commit `./tmp/` contents.
- If testing against a real mounted shared volume, create a dedicated test subfolder inside that mounted volume and use that subfolder as the shared mount root.
- Do not point tests at the root of a production or long-lived shared volume.

Examples of acceptable mounted-volume test roots:
- `X:\superseedr-test-round\`
- `/Volumes/seedbox/superseedr-test-round/`
- `/mnt/shared-drive/superseedr-test-round/`

Recommended layout:
- `./tmp/superseedr-config/hosts/`
- `./tmp/superseedr-config/inbox/`
- `./tmp/superseedr-config/processed/`
- `./tmp/superseedr-config/status/`
- `./tmp/superseedr-config/torrents/`
- `./tmp/superseedr-config/journal/`
- `./tmp/evidence/`
- `./tmp/reports/`

## Human Operator Preflight

Before recording any results, the human operator should set up the cluster intentionally.

Required preflight checks:
- pick one shared mount root and reuse it consistently for the whole round
- if using a mounted volume, create a dedicated test folder inside that volume first
- confirm every runtime can read and write that same shared root
- assign distinct host ids for each runtime, for example `host-a` and `host-b`
- decide whether the phase is testing:
  - shared offline mutation with no leader running
  - shared online behavior with one leader running
  - optional concurrent leader/follower behavior
- confirm which runtime is expected to become leader first

Recommended setup sequence:
1. Clear launcher sidecars unless the specific test is about them:
   - `cargo run -- clear-shared-config`
   - `cargo run -- clear-host-id`
2. Set or export the intended shared root and host id explicitly for each shell.
3. Start only the runtime needed for that phase.
4. Confirm leader/follower state before issuing mutating CLI commands.
5. Record the exact shared root path, host id, and whether a leader was already running.

Do not treat stale launcher sidecars, a forgotten local runtime, or mismatched host ids as acceptable setup.

## Shared Mode With Env Vars

Use env-driven launches for the main validation flow. Do not use launcher persistence as the default activation path.

Unix-like examples:
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run -- show-shared-config`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- show-host-id`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- status`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- add "magnet:?xt=..."`

PowerShell:
- `$env:SUPERSEEDR_SHARED_CONFIG_DIR = "$PWD\tmp"`
- `$env:SUPERSEEDR_SHARED_HOST_ID = "host-a"`
- `cargo run -- show-shared-config`
- `cargo run -- show-host-id`

Expected env-driven result:
- `show-shared-config` reports source `env`
- mount root resolves to `./tmp`
- config root resolves to `./tmp/superseedr-config`
- `show-host-id` reports source `env`

## Launcher And Host-ID Precedence

Shared-config precedence:
1. `SUPERSEEDR_SHARED_CONFIG_DIR`
2. persisted launcher shared-config sidecar
3. normal mode

Host-id precedence:
1. `SUPERSEEDR_SHARED_HOST_ID`
2. persisted launcher host-id sidecar
3. hostname or default fallback

## Required Test Data

Prepare only what is needed:
- at least one reusable `.torrent` fixture from `integration_tests/` if present
- at least one fabricated magnet string for queue/routing validation if needed
- one shared root at `./tmp`

If only a fabricated magnet is used, record clearly that this validates routing and queueing only.

## Command Matrix

Columns:
- Single Shared Offline: shared env vars set, no running runtime
- Single Shared Online: shared env vars set, one running shared runtime
- Cluster Shared Online: two runtimes on the same shared root
- Cluster After Failover: commands run after the original leader stops and another node takes leadership
- Required: `Yes` means required for this plan; `Optional` means run only if the environment supports it
- Validation Goal: what is being proven

| Command | Single Shared Offline | Single Shared Online | Cluster Shared Online | Cluster After Failover | Required | Validation Goal |
|---|---:|---:|---:|---:|---|---|
| show-shared-config | Yes | Yes | Yes | Yes | Yes | Shared-config selection and precedence are reported correctly |
| set-shared-config | N/A | N/A | N/A | N/A | Yes | Launcher shared-config persistence works |
| clear-shared-config | N/A | N/A | N/A | N/A | Yes | Launcher shared-config clear works |
| show-host-id | Yes | Yes | Yes | Yes | Yes | Host-id selection and precedence are reported correctly |
| set-host-id | N/A | N/A | N/A | N/A | Yes | Launcher host-id persistence works |
| clear-host-id | N/A | N/A | N/A | N/A | Yes | Launcher host-id clear works |
| to-shared | N/A | N/A | N/A | N/A | Yes | Standalone config converts into layered shared config |
| to-standalone | N/A | N/A | N/A | N/A | Yes | Active shared config converts into standalone config |
| add | Yes | Yes | Yes | Yes | Yes | Shared add routing uses shared inbox path |
| status | Yes | Yes | Yes | Yes | Yes | Shared-mode status works in text and JSON |
| journal | Yes | Yes | Yes | Yes | Yes | Shared-mode journal merges shared commands and host-local health |
| torrents | Yes | Yes | Yes | Yes | Yes | Shared-mode torrent listing works |
| info | Yes | Yes | Yes | Yes | Yes | Shared-mode torrent detail lookup works |
| files | Yes | Yes | Yes | Yes | Yes | Shared-mode file listing works when metadata/source is available |
| pause | Yes | Yes | Yes | Yes | Yes | Shared-mode control path works |
| resume | Yes | Yes | Yes | Yes | Yes | Shared-mode control path works |
| remove | Yes | Yes | Yes | Yes | Yes | Shared-mode control path works |
| purge | Yes | Yes | Yes | Yes | Yes | Shared-mode control path works, including immediate offline purge when resolvable |
| priority | Yes | Yes | Yes | Yes | Yes | Shared-mode file-priority path works |
| stop-client | No | Yes | Yes | Yes | Yes | Live runtime stop path works |

Notes:
- `N/A` means the command is not meaningfully an offline-vs-online runtime test and should be covered in its dedicated section.
- Cluster Shared Online is optional unless the environment supports two live runtimes.
- Cluster After Failover is optional unless the environment supports leadership transfer testing.
- For offline shared mutating commands, record whether no leader was running. That path now directly mutates shared config instead of only queueing.

## Validation Levels

For each command, record one or more of:
- accepted
- routed
- queued
- applied
- observed
- cluster-observed

A command should not be marked fully validated unless the report states which levels were observed.

## Phase 1: Environment, Precedence, And Layout

## 0. Offline Baseline Modes

These offline sections should be run before concurrent cluster testing.

## 0A. Normal Offline

### Goal
Prove that normal non-shared offline CLI behavior still works when no runtime is running.

### Operator setup
- ensure no Superseedr runtime is running
- ensure shared env vars are unset
- ensure launcher shared-config sidecar is cleared unless the test explicitly needs it

### Commands to cover
- `status`
- `journal`
- `torrents`
- `info`
- `files`
- `pause`
- `resume`
- `remove`
- `purge`
- `priority`

### Expected
- read commands operate on local standalone persisted state
- offline-capable mutating commands directly update local standalone config
- `purge` removes data immediately only when file layout is safely resolvable
- commands accepting `INFO_HASH_HEX_OR_PATH` should be spot-checked with:
  - direct info hash
  - reverse file-path lookup where a unique match exists

## 0B. Shared Offline (No Leader)

### Goal
Prove that shared-mode offline CLI behavior works when no leader is running.

### Operator setup
- ensure no shared runtime is running
- set shared env vars or launcher sidecars intentionally
- confirm the shared root is the expected one
- confirm no process currently holds leadership

### Commands to cover
- `show-shared-config`
- `show-host-id`
- `status`
- `journal`
- `torrents`
- `info`
- `files`
- `pause`
- `resume`
- `remove`
- `purge`
- `priority`

### Expected
- shared read commands operate on persisted shared state
- offline-capable mutating commands directly update shared config rather than merely queueing
- shared `journal` reflects host-local and shared entries from persisted files
- `purge` removes data immediately only when file layout is safely resolvable
- commands accepting `INFO_HASH_HEX_OR_PATH` should be spot-checked with:
  - direct info hash
  - reverse file-path lookup where a unique match exists

## 1. Env-Driven Shared Activation

### Goal
Prove that the branch enters shared mode from env vars without relying on persisted launcher config.

### Steps
1. Ensure launcher sidecars are cleared unless the phase explicitly needs them.
2. Ensure `SUPERSEEDR_SHARED_CONFIG_DIR` is unset and record baseline `cargo run -- show-shared-config`.
3. Run with `SUPERSEEDR_SHARED_CONFIG_DIR` set to the absolute path of `./tmp`.
4. Repeat with `SUPERSEEDR_SHARED_HOST_ID=host-a` and run `show-host-id`.

### Expected
- env-driven `show-shared-config` reports enabled with source `env`
- mount root is `./tmp`
- config root is `./tmp/superseedr-config`
- env-driven `show-host-id` reports `host-a`

## 2. Shared Root Normalization

### Goal
Prove that both mount-root and explicit `superseedr-config` forms resolve correctly.

### Steps
1. Run with `SUPERSEEDR_SHARED_CONFIG_DIR` pointing at the absolute path of `./tmp`.
2. Run again with `SUPERSEEDR_SHARED_CONFIG_DIR` pointing at the absolute path of `./tmp/superseedr-config`.
3. Compare `show-shared-config`.

### Expected
- both forms resolve correctly
- no duplicated nested config root appears

## 3. Shared File Layout Smoke

### Goal
Prove that the branch creates and uses the expected shared layout.

### Steps
1. Launch once in env-driven shared mode.
2. Inspect `./tmp/superseedr-config/`.

### Expected
Relevant layout exists as needed:
- `hosts/`
- `inbox/`
- `processed/`
- `status/`
- `torrents/`
- `journal/`
- `settings.toml`
- `torrent_metadata.toml`
- `catalog.toml` if created by the exercised flow

## Phase 2: Single-Machine Shared CLI Matrix

Run these tests on one machine against `./tmp` as the shared root.

## 4. Shared Read Commands

### Commands
- `show-shared-config`
- `show-host-id`
- `status`
- `journal`
- `torrents`
- `info`
- `files`

### Required contexts
- offline shared CLI: required
- online shared runtime: required

### Expected
- each command runs successfully or fails with a correct and understandable reason
- output shape is correct in both text and JSON where supported
- read commands do not mutate unrelated shared state
- `journal` reflects merged shared-command entries plus host-local health entries
- `files` works when metadata or a locally readable torrent source is available, otherwise it returns a clear reason
- commands that accept `INFO_HASH_HEX_OR_PATH` should be tested with:
  - direct info hash
  - reverse file-path lookup where a unique match exists

## 5. Shared Mutating Commands

### Commands
- `add`
- `pause`
- `resume`
- `remove`
- `purge`
- `priority`
- `stop-client`

### Required contexts
- offline shared CLI: required for all except `stop-client`
- online shared runtime: required for all
- cluster shared online: optional unless environment supports it

### Expected
- each command reaches the correct shared-mode path
- when a leader is running, commands that should queue do queue to shared infrastructure
- when no leader is running, offline-capable commands directly mutate shared config through the offline path
- commands mutate shared or host-local state in the correct scope
- no command accidentally falls back to normal local routing

## 6. Add Routing Details

### Goal
Prove that add requests route into the shared inbox.

### Steps
1. In env-driven shared mode, run `cargo run -- add "<magnet>"`.
2. In env-driven shared mode, run `cargo run -- add "<torrent-path>"` using a reusable fixture from `integration_tests/` if present.
3. Inspect `./tmp/superseedr-config/inbox/`.

### Expected
- magnet add lands in the shared inbox
- torrent add lands in the shared inbox, typically as a `.path` file
- add does not use the normal local watch sink

### Required note
- If `cargo run -- add` was tested instead of positional direct input, record that clearly.
- If positional direct input was not tested, record that gap.

## 7. Host-ID Separation On One Machine

### Goal
Prove that host-scoped files separate correctly without requiring two concurrent machines.

### Steps
1. Run against `./tmp` with `SUPERSEEDR_SHARED_HOST_ID=host-a`.
2. Quit cleanly.
3. Run again against the same shared root with `SUPERSEEDR_SHARED_HOST_ID=host-b`.
4. Inspect:
   - `./tmp/superseedr-config/hosts/`
   - `./tmp/superseedr-config/status/`
   - `show-host-id` from each shell

### Expected
- `hosts/host-a/config.toml` and `hosts/host-b/config.toml` can coexist
- status files are host-separated when produced
- shared global files remain shared
- `show-host-id` reports the expected host id in each shell

## 8. Launcher Commands

### Commands
- `set-shared-config`
- `clear-shared-config`
- `show-shared-config`
- `set-host-id`
- `clear-host-id`
- `show-host-id`

### Goal
Prove that launcher shared-config and host-id commands work without using them as the default activation path.

### Steps
1. Record baseline `show-shared-config` and `show-host-id`.
2. Run `cargo run -- set-shared-config <absolute-path-to-tmp>`.
3. Run `cargo run -- set-host-id host-a`.
4. Run `show-shared-config` and `show-host-id`.
5. Run `cargo run -- clear-shared-config`.
6. Run `cargo run -- clear-host-id`.
7. Run `show-shared-config` and `show-host-id` again.

### Expected
- `set-shared-config` works
- `show-shared-config` shows launcher after set
- `set-host-id` works
- `show-host-id` shows launcher after set
- `clear-shared-config` works
- `clear-host-id` works
- both show commands return to baseline after clear

## 9. Conversion Commands

### Commands
- `to-shared`
- `to-standalone`

### Goal
Prove that standalone local config can be converted into layered shared config and then flattened back into standalone config.

### Steps
1. Start from a clean standalone local config.
2. Run `cargo run -- to-shared <absolute-path-to-tmp>`.
3. Inspect `./tmp/superseedr-config/` and confirm:
   - `settings.toml`
   - `catalog.toml`
   - `torrent_metadata.toml`
   - `hosts/<host-id>/config.toml`
4. Enable shared mode through env or launcher and run read commands against the converted config.
5. Run `cargo run -- to-standalone`.
6. Inspect the local standalone settings and metadata again.

### Expected
- `to-shared` succeeds from standalone mode
- layered shared files are created with the expected host split
- `to-standalone` succeeds from active shared selection
- local standalone config is restored in a usable form

## Phase 3: Optional Concurrent Shared-Cluster Matrix

Only run if the environment supports two active runtimes.

## 10. Minimal Concurrent Shared-Cluster Setup

### Goal
Create a real concurrent shared-mode environment sufficient to validate the shared CLI surface.

### Acceptable environments
- two machines with a mounted shared directory
- one native `cargo run` instance plus one container instance sharing the same mounted host directory
- two containers sharing the same mounted host directory

### Runtime setup

Runtime A:
- shared root points at the cluster mount
- host id is `host-a`

Runtime B:
- shared root points at the same contents
- host id is `host-b`

### Required operator checks
- both runtimes can create files in the shared root
- files written by one runtime are visible to the other
- both runtimes resolve the same shared-config layout
- both runtimes report the expected host id through `show-host-id`
- operator records which runtime is expected to hold leadership first

## 11. Concurrent Shared Read Commands

### Commands
- `status`
- `journal`
- `torrents`
- `info`
- `files`
- `show-shared-config`
- `show-host-id`

### Expected
- commands run successfully in cluster mode
- output is sensible from both runtimes when applicable
- results reflect shared cluster state
- `journal` shows merged shared commands plus host-local health from the issuing host context

## 12. Concurrent Shared Mutating Commands

### Commands
- `add`
- `pause`
- `resume`
- `remove`
- `purge`
- `priority`
- `stop-client`

### Expected
- both runtimes see the same shared files
- CLI commands operate through the cluster shared-config path
- follower-issued commands do not accidentally use local normal-mode routing
- if the leader is intentionally stopped and offline shared mutation is tested, record that separately from the online cluster matrix

## 13. Cluster Failover After Leadership Transfer

### Goal
Prove that a second node can take leadership and the CLI surface still behaves correctly after failover.

### Setup
1. Start runtime A and runtime B on the same shared root.
2. Confirm runtime A is leader and runtime B is follower.
3. Exercise at least one mutating command while A is leader so there is known shared state.
4. Stop runtime A cleanly or otherwise remove its leadership.
5. Wait until runtime B takes leadership.
6. Confirm runtime B is now leader before issuing more commands.

### Required operator checks
- record which node was original leader
- record which node took leadership after failover
- record how leadership transfer was confirmed
- record whether any lock, status, or journal artifacts lagged before stabilizing

### Commands To Run After Failover
- `show-shared-config`
- `show-host-id`
- `status`
- `journal`
- `torrents`
- `info`
- `files`
- `add`
- `pause`
- `resume`
- `remove`
- `purge`
- `priority`
- `stop-client`

### Expected
- the new leader accepts and applies shared mutating commands
- read commands reflect the post-failover shared state
- no command falls back to stale routing from the former leader
- journal continues to record shared command events after failover
- status and shared files converge after leadership transfer

### Required note
- if any command only worked after a delay, record the delay and what artifact finally proved leadership transfer

## 14. Minimum Concurrent Proof Set

If time is limited, at minimum validate:
- `add`
- `status`
- `pause`
- `resume`
- `remove` or `purge`
- `stop-client`

For failover specifically, at minimum validate:
- `status`
- `journal`
- `pause` or `resume`
- `remove` or `purge`

## 15. Docs Match Actual Behavior

### Review
- `README.md`
- `docs/shared-config.md`

### Confirm
- env-driven activation is documented correctly
- launcher shared-config commands match actual behavior
- launcher host-id commands match actual behavior
- conversion commands match actual behavior
- shared-config precedence is described correctly
- host-id precedence is described correctly
- shared root layout matches observed behavior
- host vs shared settings scope matches observed behavior
- CLI surface described for shared mode is accurate

## Good Additional Behaviors To Preserve

1. Cleanup after launcher testing
- after `set-shared-config`, run `clear-shared-config` unless persistence is intentionally part of the test
- after `set-host-id`, run `clear-host-id` unless persistence is intentionally part of the test

2. Verify clear actually worked
- after clear commands, run the matching show commands again

3. Test both text and JSON for key reads
- shared-mode `status`, `journal`, `torrents`, `info`, and `files` should be spot-checked in both text and `--json`

4. Explicit filesystem verification
- when testing host-id separation, inspect the `hosts/` directory and confirm both host directories exist

5. Distinguish queued online mutation from offline direct mutation
- always record whether a leader was already running when a mutating command was issued

6. Record failover timing honestly
- if leadership transfer required waiting, record how long it took and how it was detected

7. Keep offline modes distinct
- do not merge normal offline findings with shared offline findings
- explicitly state whether a result came from local standalone state or shared persisted state with no leader

8. Write the report to disk
- create a report path under `./tmp/reports/` and write the final validation report there

9. Record add syntax honestly
- if `cargo run -- add "magnet:..."` is used instead of positional direct input, note that clearly

10. Record magnet quality honestly
- if only a fabricated magnet string was used, state that it validates routing and queueing only

## Evidence To Record

Store under `./tmp/reports/` and `./tmp/evidence/`:
- exact commands run through `cargo run`
- exact fixture paths reused from `integration_tests/` if any
- inbox file paths created by add routing
- host directory paths created for `host-a` and `host-b`
- `show-shared-config` outputs
- `show-host-id` outputs
- concise notes on what was proven versus partially validated
- which commands were validated in:
  - normal offline
  - single-machine shared offline
  - single-machine shared online
  - concurrent cluster shared online
  - cluster after failover
- which commands were only validated as routing or queueing checks
- operator notes describing cluster setup, leader/follower identity, and host ids used
- operator notes describing original leader, new leader, and how leadership transfer was confirmed

## Report Matrix

Use this table shape in the final report.

| Command | Single Shared Offline | Single Shared Online | Cluster Shared Online | Cluster After Failover | Validation Level | Notes |
|---|---|---|---|---|---|---|
| show-shared-config |  |  |  |  |  |  |
| set-shared-config | N/A | N/A | N/A | N/A |  |  |
| clear-shared-config | N/A | N/A | N/A | N/A |  |  |
| show-host-id |  |  |  |  |  |  |
| set-host-id | N/A | N/A | N/A | N/A |  |  |
| clear-host-id | N/A | N/A | N/A | N/A |  |  |
| to-shared | N/A | N/A | N/A | N/A |  |  |
| to-standalone | N/A | N/A | N/A | N/A |  |  |
| add |  |  |  |  |  |  |
| status |  |  |  |  |  |  |
| journal |  |  |  |  |  |  |
| torrents |  |  |  |  |  |  |
| info |  |  |  |  |  |  |
| files |  |  |  |  |  |  |
| pause |  |  |  |  |  |  |
| resume |  |  |  |  |  |  |
| remove |  |  |  |  |  |  |
| purge |  |  |  |  |  |  |
| priority |  |  |  |  |  |  |
| stop-client | N/A |  |  |  |  |  |

Suggested values:
- Pass
- Fail
- Skipped
- N/A

Validation Level examples:
- accepted
- routed
- queued
- applied
- observed
- cluster-observed
