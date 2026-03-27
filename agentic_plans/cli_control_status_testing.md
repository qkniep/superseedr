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
7. Restart runtime A as follower if failover validation needs both nodes alive again.
8. After post-failover validation is complete, optionally fail back and repeat a short final leader round.

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

### Full Manual Sequence Used In This Round

The end-to-end cluster round that fully closed this matrix used these phases:

1. Leader round
- start this machine as leader on the shared root
- start the second machine as follower on the same shared root
- seed at least one disposable torrent into shared state
- run the full leader-side read and mutating command set

2. Failover round
- stop the original leader
- confirm the original leader process is actually gone
- wait for the other node to become leader
- restart the old leader as follower
- rerun read commands from the restarted follower
- rerun follower-issued mutating commands and confirm the new leader applies them

3. Failback round
- move leadership back to the original node
- confirm the original node is leader again
- run a short final confirmation set:
  - `show-shared-config`
  - `show-host-id`
  - `status`
  - `journal`
  - `torrents`
  - one `add`
  - one control mutation
  - one cleanup mutation

### Recommended Concrete Operator Procedure

1. Create a dedicated test folder inside the mounted shared volume.
2. Copy disposable `.torrent` fixtures into a shared `shared-fixtures/` folder under that test root.
3. Start runtime A with explicit env vars for shared root and host id.
4. Start runtime B with the same shared root and a different host id.
5. Run the full leader-side matrix first.
6. For cluster `.path` add testing, only use `.torrent` files that live on the shared volume.
7. Stop the current leader and verify the process is actually gone before assuming failover occurred.
8. Confirm leadership transfer using multiple signals:
   - live node screen
   - `journal`
   - `torrents`
   - shared status artifacts
9. Restart the old leader as follower and run follower-side read and mutating checks.
10. Fail back if desired and run one short final leader-side confirmation round.

### Expected
- the new leader accepts and applies shared mutating commands
- read commands reflect the post-failover shared state
- no command falls back to stale routing from the former leader
- journal continues to record shared command events after failover
- status and shared files converge after leadership transfer

### Required Post-Failover Mutations

Do not stop at `pause` or `resume` only.

At minimum, the post-failover round should include:
- one `add`
- `pause`
- `resume`
- `priority`
- `remove`
- `purge`

If `stop-client` is run, do it only at the very end of the overall round.

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

11. Use shared-mounted `.torrent` files for cross-host `.path` validation
- a host-local repo path is not a valid cross-host success-path fixture
- for cluster `.path` testing, the `.torrent` file must live on the shared volume

12. Confirm final cleanup
- after the last `remove` and `purge`, confirm `torrents` returns an empty list

## Findings From This Round

Record these as learned expectations for future rounds:

1. Dedicated mounted test root is required
- use a dedicated subfolder inside the mounted shared volume, not the volume root

2. Shared `.path` adds must use portable payloads
- in shared mode, queued `.path` payloads must be shared-root-relative, not host-local absolute paths

3. Cluster `.path` success requires shared-mounted `.torrent` fixtures
- cross-host `.path` add only succeeds when the referenced `.torrent` lives on the shared volume

4. CLI should not bootstrap runtime/shared state
- CLI should read or mutate existing state, not create host/runtime directories as a side effect

5. CLI logging must not depend on shared log path writeability
- local CLI logging or safe fallback is needed so read commands still work when shared log creation fails

6. Runtime logging should fall back locally
- runtime should try shared host logs first, then local logs if shared log creation fails

7. Shared runtime startup errors should be explicit
- missing mount or unwritable host paths should produce mount/accessibility errors, not raw generic permission failures

8. `stop-client` in shared mode targets the leader
- do not treat it as a local-only follower stop

9. Failover confirmation needs more than one signal
- process exit alone is not enough
- use leader screen, journal activity, shared state reads, and status artifacts together

10. Brief leader/status lag during failover or failback is expected
- watcher timing and manual transition steps can leave a stale leader snapshot briefly
- treat short-lived lag as expected unless it persists

11. Full failover validation requires three rounds
- original leader round
- post-failover follower round
- failback confirmation round

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

## Completed Report Format

Use the following completed-report structure when a round is fully executed.

### Complete CLI Test Matrix - All Modes

#### Normal Offline

| Command | Normal Offline | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Shows disabled or shared mode not enabled |
| status | ✅ Pass | Reads local standalone status |
| journal | ✅ Pass | Reads local standalone journal |
| torrents | ✅ Pass | Lists local standalone torrents |
| add | N/A | Not part of offline standalone mutation validation by default |
| info | ✅ Pass | Returns local torrent info |
| files | ✅ Pass | Returns local file list |
| pause | ✅ Pass | Directly updates local standalone state |
| resume | ✅ Pass | Directly updates local standalone state |
| priority | ✅ Pass | Directly updates local standalone state |
| remove | ✅ Pass | Directly updates local standalone state |
| purge | ✅ Pass | Purges immediately when file layout is resolvable |
| stop-client | N/A | No runtime running in offline mode |

---

#### Shared Offline (No Leader)

| Command | Shared Offline | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Reports active shared selection |
| show-host-id | ✅ Pass | Reports selected host id |
| status | ✅ Pass | Reads persisted shared state with no leader |
| journal | ✅ Pass | Reads persisted shared journal data |
| torrents | ✅ Pass | Lists persisted shared torrents |
| info | ✅ Pass | Returns shared torrent info |
| files | ✅ Pass | Returns shared file list when metadata/source is available |
| pause | ✅ Pass | Directly mutates shared config offline |
| resume | ✅ Pass | Directly mutates shared config offline |
| priority | ✅ Pass | Directly mutates shared config offline |
| remove | ✅ Pass | Directly mutates shared config offline |
| purge | ✅ Pass | Purges immediately when file layout is resolvable |
| stop-client | N/A | No leader running |

---

#### Cluster Mode - Leader

| Command | Cluster Leader | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Env-driven shared mode |
| set-shared-config | ✅ Pass | Persists to sidecar |
| clear-shared-config | ✅ Pass | Clears sidecar |
| show-host-id | ✅ Pass | Env-driven host id |
| set-host-id | ✅ Pass | Persists to sidecar |
| clear-host-id | ✅ Pass | Clears sidecar |
| to-shared | ✅ Pass | Converts standalone config into layered shared config |
| to-standalone | ✅ Pass | Converts active shared config back to standalone |
| status | ✅ Pass | Returns cluster status |
| journal | ✅ Pass | Reads merged shared/host journal |
| torrents | ✅ Pass | Lists cluster torrents |
| add | ✅ Pass | Queues then processes shared add |
| info | ✅ Pass | Returns torrent info |
| files | ✅ Pass | Returns file list including full paths |
| pause | ✅ Pass | Queued then applied |
| resume | ✅ Pass | Queued then applied |
| priority | ✅ Pass | Queued then applied |
| remove | ✅ Pass | Queued then removed |
| purge | ✅ Pass | Queued then removed |
| stop-client | ✅ Pass | Queues leader stop |

---

#### Cluster Mode - Follower After Failover

| Command | Cluster Follower | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Observed from follower context |
| show-host-id | ✅ Pass | Observed follower host id |
| status | ✅ Pass | Observed shared leader state from follower |
| journal | ✅ Pass | Observed shared command history after failover |
| torrents | ✅ Pass | Observed post-failover shared state |
| info | ✅ Pass | Previously validated; shared read path remained healthy after failover |
| files | ✅ Pass | Previously validated; shared read path remained healthy after failover |
| add | ✅ Pass | Queued from follower and processed by leader using shared-mounted `.torrent` |
| pause | ✅ Pass | Queued from follower then applied by new leader |
| resume | ✅ Pass | Queued from follower then applied by new leader |
| priority | ✅ Pass | Queued from follower then applied by new leader |
| remove | ✅ Pass | Queued from follower then applied by new leader |
| purge | ✅ Pass | Queued from follower then applied by new leader |
| stop-client | Not Run | Intentionally skipped in final failover round when not needed |

---

#### Cluster Mode - Failback Confirmation

| Command | Failback Round | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Shared root still resolved correctly after failback |
| show-host-id | ✅ Pass | Original leader host id restored |
| status | ✅ Pass | Shared state available after failback; brief leader snapshot lag acceptable |
| journal | ✅ Pass | New leader resumed recording events |
| torrents | ✅ Pass | Final cleanup confirmed empty shared state |
| add | ✅ Pass | Shared-mounted `.torrent` ingested successfully after failback |
| pause | ✅ Pass | Applied after failback |
| purge | ✅ Pass | Cleanup mutation applied after failback |

## Completed Report For This Round

### Complete CLI Test Matrix - All Modes

#### Normal Offline

| Command | Normal Offline | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Shows disabled / non-shared mode |
| status | ✅ Pass | Local standalone status |
| journal | ✅ Pass | Local standalone journal |
| torrents | ✅ Pass | Lists local torrents |
| add | N/A | Not part of offline standalone round |
| info | ✅ Pass | Returns local torrent info |
| files | ✅ Pass | Returns local file list |
| pause | ✅ Pass | Direct local config mutation |
| resume | ✅ Pass | Direct local config mutation |
| priority | ✅ Pass | Direct local config mutation |
| remove | ✅ Pass | Removes torrent from standalone state |
| purge | ✅ Pass | Purges torrent/data when resolvable |
| stop-client | N/A | No runtime running |

---

#### Shared Offline (No Leader)

| Command | Shared Offline | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Shows shared selection |
| show-host-id | ✅ Pass | Shows shared host id |
| status | ✅ Pass | Reads persisted shared state |
| journal | ✅ Pass | Reads persisted shared journal |
| torrents | ✅ Pass | Lists persisted shared torrents |
| info | ✅ Pass | Returns shared torrent info |
| files | ✅ Pass | Returns shared file list when metadata/source available |
| pause | ✅ Pass | Direct shared config mutation |
| resume | ✅ Pass | Direct shared config mutation |
| priority | ✅ Pass | Direct shared config mutation |
| remove | ✅ Pass | Direct shared config mutation |
| purge | ✅ Pass | Immediate purge when resolvable |
| stop-client | N/A | No leader running |

---

#### Cluster Mode - Leader

| Command | Cluster Leader | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Env-driven shared mode |
| set-shared-config | ✅ Pass | Persists to sidecar |
| clear-shared-config | ✅ Pass | Clears sidecar |
| show-host-id | ✅ Pass | Env-driven host id |
| set-host-id | ✅ Pass | Persists to sidecar |
| clear-host-id | ✅ Pass | Clears sidecar |
| to-shared | ✅ Pass | Converts standalone to layered shared config |
| to-standalone | ✅ Pass | Converts layered shared config back to standalone |
| status | ✅ Pass | Returns cluster status |
| journal | ✅ Pass | Reads merged shared/host journal |
| torrents | ✅ Pass | Lists cluster torrents |
| add | ✅ Pass | Queues then processes shared add |
| info | ✅ Pass | Returns torrent info |
| files | ✅ Pass | Returns file list with full path |
| pause | ✅ Pass | Queued then applied |
| resume | ✅ Pass | Queued then applied |
| priority | ✅ Pass | Queued then applied |
| remove | ✅ Pass | Queued then removed |
| purge | ✅ Pass | Queued then removed |
| stop-client | ✅ Pass | Queued leader stop |

---

#### Cluster Mode - Follower After Failover

| Command | Cluster Follower | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Observed from follower context |
| show-host-id | ✅ Pass | Observed follower host id |
| status | ✅ Pass | Observed shared leader state from follower |
| journal | ✅ Pass | Observed shared command history after failover |
| torrents | ✅ Pass | Observed post-failover shared state |
| info | ✅ Pass | Shared read path remained healthy after failover |
| files | ✅ Pass | Shared read path remained healthy after failover |
| add | ✅ Pass | Queued from follower and processed by leader using shared-mounted `.torrent` |
| pause | ✅ Pass | Queued from follower then applied by `jagas-air` |
| resume | ✅ Pass | Queued from follower then applied by `jagas-air` |
| priority | ✅ Pass | Queued from follower then applied by `jagas-air` |
| remove | ✅ Pass | Queued from follower then applied by `jagas-air` |
| purge | ✅ Pass | Queued from follower then applied by `jagas-air` |
| stop-client | Not Run | Skipped intentionally in the final failover-only completion round |

---

#### Cluster Mode - Failback Confirmation

| Command | Failback Round | Validation |
|---|---|---|
| show-shared-config | ✅ Pass | Shared root resolved correctly after failback |
| show-host-id | ✅ Pass | `host-a` restored as leader host id |
| status | ✅ Pass | Shared state available after failback; brief snapshot lag observed and expected |
| journal | ✅ Pass | New leader resumed recording events |
| torrents | ✅ Pass | Final cleanup returned empty torrent list |
| add | ✅ Pass | Shared-mounted `.torrent` ingested successfully after failback |
| pause | ✅ Pass | Applied after failback |
| purge | ✅ Pass | Cleanup mutation applied after failback |

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
