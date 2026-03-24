# Branch Validation Plan: codex/unified-config

## Purpose

This document replaces the older CLI-only validation plan with a practical agent-run test plan for the full codex/unified-config branch.

This branch now spans much more than CLI control. It includes:

- layered shared-config mode
- env-driven shared-mode activation
- launcher-persisted shared-mode activation
- shared leader and follower behavior
- shared inbox and watch-folder routing
- CLI control, status, and journal commands
- event journal persistence
- status file changes
- TUI changes including journal, help, config, and browser behavior
- migration tooling
- Docker and runtime layout changes

This plan is optimized for an autonomous agent:
- prioritize highest-signal shared-mode tests first
- collect evidence under the current working directory
- run the current local codebase directly
- stop and report on critical failures
- avoid spending excessive time on low-value permutations unless the core path passes

## Scope

Validate the branch against these goals:

1. Shared mode works through both:
   - `SUPERSEEDR_SHARED_CONFIG_DIR`
   - persisted launcher config via `set-shared-config`
2. Browser and protocol-style direct magnet launches are correctly routed in shared mode.
3. Shared config layering and file layout are correct.
4. CLI status, control, and journal flows behave correctly online, offline, and in shared mode.
5. Host-only vs shared settings persistence is correct.
6. No obvious regressions in TUI startup and major screens.
7. Docs and examples match behavior closely enough for release confidence.
8. True concurrent leader and follower behavior is validated when the environment supports it.
9. Normal mode receives only light validation here; fuller normal-mode testing is manual or Docker-based.

## Core Execution Rule

- The agent should test the current checked-out codebase by running `cargo run`, not an installed global binary.
- Prefer `cargo run -- <args>` for CLI validation.
- Prefer env-prefixed `cargo run -- <args>` for shared-mode validation.
- If the agent needs a release-mode check later, it may optionally use `cargo run --release -- <args>`, but the default path is normal `cargo run`.
- Do not assume that a previously installed `superseedr` binary matches the current branch.
- When reporting evidence, note that commands were executed through `cargo run` against the current working tree.

## Validation Priorities

### Priority 1: Shared Mode Required
These tests are the default path and should always run.

Covers:
- env-driven shared activation
- direct magnet routing into shared inbox
- shared file layout
- host-only vs shared settings persistence
- shared-mode CLI status, journal, and control
- sequential shared-role checks on one machine
- shared-mode TUI smoke

### Priority 2: Concurrent Shared Cluster Optional
Run only when the environment supports two active instances.

Covers:
- true leader and follower simultaneous behavior
- follower relay while leader is active
- live convergence across nodes
- lock handoff and promotion under contention
- multi-node pause, resume, remove, and purge timing and propagation

### Priority 3: Launcher Config And Normal Mode Limited
This plan does not use `set-shared-config` as the default shared-mode activation path, because persisting launcher config can overwrite or replace a previously set local shared-config preference.

Only do:
- a focused validation of `show-shared-config`, `set-shared-config`, and `clear-shared-config` later in the plan
- build and startup smoke if needed for baseline confidence
- quick normal-mode manual sanity if convenient
- Docker-based normal-mode sanity if part of broader runtime validation

Do not spend significant time on exhaustive normal-mode coverage in this plan.

## Agent Workspace And Shared Root Rules

- Use the current working directory's `./tmp/` as the default shared mount root for this plan.
- Treat `./tmp/` as both:
  - the scratch workspace for generated validation artifacts
  - the default local shared-config mount root for shared-mode tests
- Do not scatter scratch files elsewhere in the repository.
- Do not commit `./tmp/` contents.
- When the plan says shared root or shared mount root, default to the absolute path of `./tmp` unless a specific environment requires another path.

Recommended layout under the current directory:

- `./tmp/superseedr-config/hosts/`
- `./tmp/superseedr-config/inbox/`
- `./tmp/superseedr-config/processed/`
- `./tmp/superseedr-config/status/`
- `./tmp/superseedr-config/torrents/`
- `./tmp/evidence/`
- `./tmp/logs/`
- `./tmp/config-snapshots/`
- `./tmp/reports/`

Suggested setup:
- create `./tmp` before starting the plan
- use the absolute path of `./tmp` whenever the CLI requires an absolute shared root

When this plan refers to:
- absolute-shared-mount-root: use the absolute path of `./tmp`
- shared-root: use `./tmp`

## How To Run The Current Client Codebase

Use these rules for all main test execution.

### Default invocation style
- use `cargo run -- <args>` for CLI commands
- use `cargo run` for launching the TUI without args
- use env-prefixed `cargo run` for shared-mode tests

### Examples for local CLI execution
- `cargo run -- --help`
- `cargo run -- show-shared-config`
- `cargo run -- status`
- `cargo run -- journal`
- `cargo run -- "magnet:?xt=..."`

### Examples for local shared-mode CLI execution
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run -- show-shared-config`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- status`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- "magnet:?xt=..."`

### Examples for launching the current TUI code in shared mode
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run`

### Important rule
- Do not replace these with `superseedr ...` during the main validation flow.
- Only use an installed binary if a later packaging or Docker check explicitly requires it.
- The main branch-validation evidence should come from `cargo run` against the current checkout.

## How To Run The Client In Shared Mode With The Env Var

Use this section whenever the plan says to test env-driven shared mode.

### Default shared root for this plan
- shared mount root: the absolute path to `./tmp`
- shared config root: the absolute path to `./tmp/superseedr-config`

### Important rule for this plan
- Do not use `set-shared-config` as the default way to activate shared mode during the main validation flow.
- Prefer one-off env-var launches so the test does not modify or replace any previously persisted launcher shared-config setting.
- Only test `set-shared-config` and `clear-shared-config` in the dedicated launcher-config section later in the plan.

### Shell setup
Before launching the client in env-driven shared mode:

- create `./tmp` if it does not already exist
- set `SUPERSEEDR_SHARED_CONFIG_DIR` to the absolute path of `./tmp`
- optionally set `SUPERSEEDR_SHARED_HOST_ID` to a stable test host id such as `host-a` or `host-b`

### Example launch flow on Unix-like shells
Use one-off env-prefixed launches such as:

- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run`

For CLI commands in shared mode, use the same env prefix, for example:

- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run -- show-shared-config`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- status`
- `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" SUPERSEEDR_SHARED_HOST_ID="host-a" cargo run -- "magnet:?xt=..."`

### Example launch flow on PowerShell
Use:

- `$env:SUPERSEEDR_SHARED_CONFIG_DIR = "$PWD\tmp"`
- optionally:
  - `$env:SUPERSEEDR_SHARED_HOST_ID = "host-a"`
- then run:
  - `cargo run`
- for CLI commands:
  - `cargo run -- show-shared-config`

For one-off cleanup after the test:
- `Remove-Item Env:SUPERSEEDR_SHARED_CONFIG_DIR`
- `Remove-Item Env:SUPERSEEDR_SHARED_HOST_ID`

### Example launch flow on Windows cmd.exe
Use:

- `set SUPERSEEDR_SHARED_CONFIG_DIR=%cd%\tmp`
- optionally:
  - `set SUPERSEEDR_SHARED_HOST_ID=host-a`
- then run:
  - `cargo run`
- for CLI commands:
  - `cargo run -- show-shared-config`

### Sequential one-machine shared-role testing
If only one machine is available, run the client twice against the same env-driven shared root with different host ids.

Example sequence:
- launch with `SUPERSEEDR_SHARED_CONFIG_DIR` pointing at the absolute path of `./tmp` and `SUPERSEEDR_SHARED_HOST_ID=host-a`
- quit cleanly
- relaunch with the same shared root but `SUPERSEEDR_SHARED_HOST_ID=host-b`

Expected result:
- shared global files stay under `./tmp/superseedr-config/`
- host-scoped files are separated by host id under `hosts/` and `status/`

### Expected observable result of env-driven shared launch
After launch, verify:
- `show-shared-config` reports source `env`
- the effective mount root is the absolute path of `./tmp`
- the effective config root is the absolute path of `./tmp/superseedr-config`
- shared command routing uses `./tmp/superseedr-config/inbox/`

## Asset Reuse Rules

- Prefer repo-local reusable fixtures over ad hoc generated assets.
- Before creating any new torrent fixtures or payload files, inspect the repository for reusable assets such as:
  - existing `.torrent` fixtures
  - sample payload files
  - integration or fixture directories
  - test assets referenced by docs, scripts, or existing tests
- If reusable assets exist, use them first and record exactly which files were used.
- If reusable assets do not exist or are insufficient, create temporary artifacts only under `./tmp/`.
- Do not place ad hoc test torrents or payload data elsewhere in the repository.
- If the repo contains an `integration_tests`, `fixtures`, `testdata`, `samples`, or similar folder, prefer those assets over generated ones.
- If no fixture folder exists, note that in the final report and continue using `./tmp`-generated assets.

## Branch Surface Summary

Key behavior areas introduced or changed in this branch:

- shared config layering under `superseedr-config/`
- shared mount root normalization
- persisted launcher-sidecar shared mode selection
- new CLI commands:
  - `set-shared-config`
  - `clear-shared-config`
  - `show-shared-config`
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
- control request serialization and watched command sink
- per-host status files in shared mode
- event journal
- follower relay and inbox behavior
- migration script for legacy settings
- TUI screens and help and config changes

## Execution Model For The Agent

Use this strategy:

### Phase 1: Shared-Mode Fast Confidence
Run the highest-signal tests first:
- env-driven shared mode activation
- direct positional magnet in shared mode
- shared root file-layout smoke
- shared-mode journal and status JSON
- shared add, pause, resume, remove, and purge smoke

If Phase 1 has a critical failure, stop and report before spending time elsewhere.

### Phase 2: Shared-Mode Deep Validation
Only if Phase 1 is mostly green:
- host-only vs shared settings persistence
- sequential shared-role checks on one machine
- priority propagation
- files, info, and torrents read-path validation
- TUI shared-mode screens
- migration script
- docs mismatch review

### Phase 3: Concurrent Cluster Validation
Only if two active instances are possible:
- simultaneous leader and follower behavior
- follower relay while leader is active
- live convergence across nodes
- lock contention, handoff, and promotion

### Phase 4: Launcher-Config And Limited Normal-Mode Validation
Only after shared-mode confidence is established:
- focused validation of `show-shared-config`, `set-shared-config`, and `clear-shared-config`
- normal-mode manual sanity, if convenient
- Docker-based runtime validation, including normal mode if needed

### Phase 5: Resilience
If time remains:
- broken shared mount
- corrupt shared files
- concurrent save conflict behavior
- lock handoff and promotion behavior

## Required Test Environments

### Minimum
- one local environment where the branch can build and run
- one shared-root directory available on that machine, defaulting to `./tmp`

### Preferred
- Linux
- macOS or Windows
- Docker

### Optional For Full Cluster Validation
- a second machine, or
- a second isolated runtime on the same machine

If concurrent two-node coverage is not available, the agent should still complete Phase 1 and Phase 2 and clearly report that concurrent shared-cluster validation was skipped due to environment limits.

## Test Data To Prepare

Prepare or collect:

- 2 working magnet links
- 2 valid `.torrent` files
  - one single-file
  - one multi-file
- one `.path` input referencing a valid `.torrent`
- one bad `.path`
- one malformed magnet input
- one torrent with multiple files for priority
- one torrent with payload on disk for purge
- one shared root directory at `./tmp`

Fixture sourcing order:
1. repo-local reusable assets
2. documented sample assets referenced by the branch
3. temporary assets created under `./tmp`

Default shared root for this plan:

- `./tmp/superseedr-config/hosts/`
- `./tmp/superseedr-config/inbox/`
- `./tmp/superseedr-config/processed/`
- `./tmp/superseedr-config/status/`
- `./tmp/superseedr-config/torrents/`

## Evidence Requirements

For every major section:
- capture command output
- note pass or fail
- save important file snapshots when relevant
- record exact paths used
- keep, when possible:
  - command transcript
  - JSON output
  - relevant config files
  - short explanation of result

Store evidence under:
- `./tmp/evidence/`
- `./tmp/logs/`
- `./tmp/config-snapshots/`
- `./tmp/reports/`

Also record asset provenance:
- whether assets came from repo fixtures or `./tmp`-generated files
- the exact paths used for torrents and payload data

At the end, produce:
- passed tests
- failed tests
- skipped tests
- unresolved questions
- release-risk summary

## Severity Rules

### Critical
Stop and report immediately if any of these occur:
- app cannot start in shared mode
- direct magnet add routes to wrong sink in shared mode
- shared settings are silently written to wrong file scope
- shared mode destroys or corrupts config data
- remove and purge semantics are dangerously wrong
- repeated panic or crash in common shared-mode flow

### High
Continue, but flag prominently:
- status or journal behavior wrong
- follower and leader role confusion
- docs significantly out of date
- JSON contract broken
- TUI major screen broken

### Medium
Log and continue:
- minor text-output issues
- non-critical doc mismatches
- cosmetic TUI issues
- minor naming inconsistencies

## Phase 1: Shared-Mode Fast Confidence

## 1. Env-Driven Shared Activation Smoke

### Preconditions
- ensure `./tmp` exists
- ensure `SUPERSEEDR_SHARED_CONFIG_DIR` is unset before the test
- do not use `set-shared-config` for this section

### Steps
1. Run `cargo run -- show-shared-config` without shared env vars and note the baseline result.
2. Run `SUPERSEEDR_SHARED_CONFIG_DIR="$(pwd)/tmp" cargo run -- show-shared-config`.
3. Repeat with `SUPERSEEDR_SHARED_HOST_ID=host-a`.

### Expected
- baseline without env does not rely on persisted launcher changes for this test
- env-driven `show-shared-config` reports enabled
- source is `env`
- mount root resolves to the absolute path of `./tmp`
- config root resolves to the absolute path of `./tmp/superseedr-config`

## 2. Shared Direct Magnet Routing

This is the highest-value branch-specific test.

### Steps
1. Use env-driven shared mode pointed at `./tmp`.
2. Run direct positional input with a valid magnet through `cargo run`.
3. Inspect where the queued file lands.

### Expected
- shared mode is activated early
- magnet add lands in `./tmp/superseedr-config/inbox/`
- magnet add does not land in the local normal watch path

If this fails, stop and report as critical.

## 3. Shared File Layout Smoke

### Steps
1. Launch app once in env-driven shared mode using `./tmp` with `cargo run`.
2. Inspect `./tmp` and `./tmp/superseedr-config/`.
3. Confirm expected directories and files exist when relevant:
   - `./tmp/superseedr-config/`
   - `./tmp/superseedr-config/hosts/`
   - `./tmp/superseedr-config/inbox/`
   - `./tmp/superseedr-config/processed/`
   - `./tmp/superseedr-config/status/`
   - `./tmp/superseedr-config/torrent_metadata.toml`
   - `./tmp/superseedr-config/settings.toml`
   - `./tmp/superseedr-config/catalog.toml` if created by flow

### Expected
- shared root layout is sensible
- no duplicated nested config root
- host-specific files are scoped under `hosts/` and `status/`

## 4. Shared Status And Journal JSON Smoke

### Steps
Run on at least one active env-driven shared-mode instance through `cargo run`:
- `status`
- `journal`
- JSON variants of both

### Expected
- commands work
- JSON is valid
- envelope shape is consistent
- journal has meaningful entries

## 5. Shared Add And Control Smoke

### Steps
In env-driven shared mode using `./tmp`, run through `cargo run`:
- add a magnet or `.torrent`
- pause
- resume
- remove

If payload is available, also:
- purge

Then verify using:
- `torrents`
- `status`
- `journal`

### Expected
- commands succeed in reasonable mode
- shared desired-state files update correctly
- journal records actions
- remove and purge are distinct

## Phase 1 Exit Criteria

Proceed to Phase 2 only if:
- env-driven shared activation works
- direct magnet in shared mode lands in `./tmp/superseedr-config/inbox/`
- shared file-layout smoke works
- shared status and journal smoke works
- shared add and control smoke works

If any of the above fails critically, stop and write a focused defect report.

## Phase 2: Shared-Mode Deep Validation

## 6. Shared Env Activation Variants

### Steps
1. Launch with `SUPERSEEDR_SHARED_CONFIG_DIR` set to the absolute path of `./tmp`.
2. Launch again with both:
   - `SUPERSEEDR_SHARED_CONFIG_DIR` set to the absolute path of `./tmp`
   - `SUPERSEEDR_SHARED_HOST_ID=host-a`
3. Run `show-shared-config` through the same env-driven `cargo run` approach each time.

### Expected
- source remains `env`
- mount root and config root stay correct
- host-specific files are attributable to the provided host id when relevant

## 7. Host-Only vs Shared Settings Persistence

### Goal
Verify that host-local values and shared values are written to the correct files under `./tmp/superseedr-config/`.

### Shared-field examples
- default download folder
- global speed limits
- RSS and shared settings
- shared theme or performance settings if applicable

### Host-local examples
- client port
- watch folder
- host-specific client id override

### Steps
1. In env-driven shared mode, change one shared field.
2. Inspect:
   - `./tmp/superseedr-config/settings.toml`
   - `./tmp/superseedr-config/hosts/<host>.toml`
   - `./tmp/superseedr-config/cluster.revision`
3. Then change one host-local field.
4. Inspect the same files again.

### Expected
- shared field writes to shared settings
- host-only field writes only to host file
- host-only changes do not incorrectly rewrite shared global data
- revision behavior matches branch design

## 8. Sequential Shared-Role Check On One Machine

Use this when only one Superseedr instance can run on the computer.

### Steps
1. Enable shared mode using env vars pointing at the absolute path of `./tmp`.
2. Set host id to `host-a`.
3. Launch app through `cargo run` and quit cleanly.
4. Inspect:
   - `./tmp/superseedr-config/hosts/host-a.toml`
   - `./tmp/superseedr-config/status/host-a.json` if produced
5. Change host id to `host-b`.
6. Relaunch against the same shared root through `cargo run` and quit.
7. Inspect:
   - `./tmp/superseedr-config/hosts/host-b.toml`
   - `./tmp/superseedr-config/status/host-b.json` if produced
8. Compare shared files:
   - shared settings
   - shared metadata
   - shared catalog if present

### Expected
- `host-a` and `host-b` create distinct host-scoped files
- shared global files remain shared
- no corruption occurs when switching host identity sequentially
- this validates host scoping without needing two concurrent runtimes

## 9. Shared Add Flow Validation

### Steps
In env-driven shared mode on one machine using `./tmp`, run through `cargo run`:
- add magnet
- add `.torrent`
- add `.path` if supported in test setup

Then inspect:
- `./tmp/superseedr-config/catalog.toml`
- `./tmp/superseedr-config/torrent_metadata.toml`
- `./tmp/superseedr-config/inbox/`
- `./tmp/superseedr-config/processed/`

### Expected
- shared desired-state files update correctly
- metadata is updated
- command routing uses shared sink

## 10. Shared Pause, Resume, Remove, And Purge Validation

### Steps
Execute in env-driven shared mode through `cargo run`:
- pause
- resume
- remove
- purge

From:
- running shared instance
- offline shared CLI if practical, still using env vars

### Expected
- desired state is updated correctly
- remove keeps payload if intended
- purge removes payload when intended
- no orphan config entries remain

## 11. Shared File Priority Validation

### Preconditions
- multi-file torrent active

### Steps
Run through `cargo run`:
- `priority` against file index
- `priority` against relative file path

Then inspect:
- runtime effect if visible
- persisted metadata
- shared propagation if applicable

### Expected
- file priority updates persist
- path and index targeting both work

## 12. Shared Read-Path Validation

### Steps
Run in both text and JSON through `cargo run`:
- `torrents`
- `info` by info hash
- `info` by path where supported
- `files` by info hash
- `files` by path where supported

All shared-mode invocations in this section should be env-driven.

### Expected
- read commands remain read-only
- stable field types
- JSON output envelope is correct
- `files` remains an array field
- errors are helpful

## 13. Shared Status Follow Validation

### Steps
With a running env-driven shared-mode instance:
- run `status --follow`
- cause one or two state changes
- stop as applicable using `status --stop`

### Expected
- status follow emits fresh updates
- stop command behaves correctly
- behavior remains sane in shared mode

## 14. Shared Event Journal Validation

### Steps
Generate:
- online control action
- offline control action
- one failed action
- one shared-mode action

Then inspect:
- `journal`
- JSON journal

### Expected
- entries contain action, timing, category, and host info when relevant
- control outcomes are distinguishable
- no malformed output

## 15. Shared-Mode TUI Smoke

This is not a full UX pass, only major-screen sanity.

### Steps
Launch TUI in env-driven shared mode through `cargo run` and visit:
- help screen
- browser screen
- config screen
- journal screen
- normal or main torrent list
- delete confirmation flow

### Expected
- no panic
- screens render
- navigation works
- obvious shared-mode restrictions are respected if present

## 16. Migration Script Validation

### Script
- `local_scripts/migrate_legacy_settings_to_layered.py`

### Steps
1. Prepare a representative legacy config.
2. Run migration.
3. Inspect produced files.
4. Launch the branch using migrated output.

### Expected
- migration output is coherent
- torrent and config data are preserved
- app can load migrated state
- rerun behavior is reasonable or at least not destructive

## Phase 3: Concurrent Shared Cluster Validation

Run this phase only if two active instances are possible.

## 17. Shared Leader And Follower Smoke

### Preconditions
- a real shared root usable by two instances
- default recommendation: use the same absolute path to `./tmp` for both runtimes
- use env vars for both runtimes rather than `set-shared-config`
- use `cargo run` for both runtimes unless the environment specifically requires another runner

### Steps
1. Start instance A in shared mode on the shared root.
2. Confirm it becomes leader.
3. Start instance B on the same shared root.
4. Confirm it becomes follower.
5. Inspect:
   - lock file
   - host status files
   - host config files

### Expected
- one leader and one follower
- shared lock exists
- distinct host files and status files exist
- follower does not behave like independent normal mode

## 18. Follower Relay Validation

### Steps
On follower:
- add magnet
- add `.torrent`
- add `.path`

Observe:
- follower local watch behavior
- shared inbox and staging behavior under `./tmp/superseedr-config/`
- leader processing
- shared catalog change

### Expected
- follower does not directly mutate shared desired state
- requests are relayed through inbox or staging mechanism
- leader performs shared-state write

## 19. Live Convergence Validation

### Steps
With leader and follower both active:
- add torrent on leader
- add torrent on follower
- pause or resume from one node
- remove or purge from one node

### Expected
- all nodes converge to same desired state
- timing may differ, final state should match
- no duplicate or orphan desired state entries

## 20. Lock Handoff And Promotion

### Steps
1. Start leader and follower.
2. Stop or kill leader.
3. Observe follower.
4. Relaunch leader if possible.

### Expected
- follower promotion or recovery matches design
- cluster does not remain stuck or split incorrectly

## Phase 4: Launcher-Config And Limited Normal-Mode Validation

Launcher-config testing is intentionally separate from the main shared-mode flow.

## 21. Launcher-Config Focused Validation

### Goal
Verify that launcher-config commands work, without using them as the default activation path for the rest of this plan.

### Steps
1. Record the current result of `cargo run -- show-shared-config`.
2. Run `cargo run -- set-shared-config <absolute-path-to-tmp>`.
3. Run `cargo run -- show-shared-config`.
4. Run `cargo run -- clear-shared-config`.
5. Run `cargo run -- show-shared-config` again.

### Expected
- launcher-config commands work
- results are understandable
- any preexisting persisted shared-config preference is treated carefully and called out in the report

### Report requirement
If this section is run, note whether it modified any previously persisted launcher shared-config setting.

## 22. Manual Normal-Mode Sanity

### Optional
If convenient, do only:
- startup
- one add
- one status
- one remove

### Expected
- no obvious regression

If not convenient, skip and note that fuller normal-mode coverage is manual.

## 23. Docker Validation

### Minimum
- run compose or documented container path once

### Steps
1. Bring up Docker setup from branch docs or examples.
2. Confirm startup.
3. If possible, test shared mode in Docker with mounted shared root, preferably mapping host `./tmp`.
4. If time remains, do light normal-mode Docker sanity.

### Expected
- compose and runtime still work
- no obvious path mismatch from branch changes

If full Docker validation is not possible, note as skipped with reason.

## Phase 5: Resilience And Failure Tests

## 24. Missing Shared Mount

### Steps
1. Use env-driven shared mode pointed at the absolute path of `./tmp`.
2. Make `./tmp` unavailable or temporarily move it aside.
3. Launch app through `cargo run`.

### Expected
- clear failure or graceful handling
- no silent wrong-mode fallback unless explicitly designed

## 25. Corrupt Shared Config Files

### Steps
Corrupt one at a time under `./tmp/superseedr-config/`:
- shared `settings.toml`
- shared `catalog.toml`
- shared `torrent_metadata.toml`
- host file

Then try launch and read commands in env-driven shared mode through `cargo run`.

### Expected
- useful error
- no silent destructive overwrite
- no crash loop

## 26. Concurrent Shared Save Guard

### Steps
If practical:
- modify shared config externally while app is active
- or have two writers race shared save behavior

### Expected
- stale-write protection behaves sensibly
- shared files are not silently overwritten incorrectly

## Docs Validation

## 27. README And Shared-Config Docs Review

Review:
- `README.md`
- `docs/shared-config.md`

Confirm docs accurately describe:
- env-driven activation
- launcher-sidecar activation
- env precedence
- shared root layout
- host vs shared settings scope
- browser and protocol magnet behavior
- leader and follower behavior
- Docker examples if present
- one-machine validation limits for cluster behavior if documented

Record any mismatches.

## Output Format For Agent Report

At completion, produce this report shape:

## Summary
- overall result: pass, pass with issues, or fail
- highest-severity finding
- confidence level
- phases completed

## Passed
- bullet list of major passed sections

## Failed
For each failure:
- title
- severity
- exact reproduction
- expected vs actual
- likely affected files
- evidence paths or inline snippets

## Skipped
- what was skipped
- why
- whether skip was due to one-machine limitation
- whether skip was due to normal-mode de-prioritization

## Important Evidence
- paths to saved outputs
- config snapshots
- JSON examples
- screenshots if applicable

## Release Recommendation
Choose one:
- ready for merge
- ready after small fixes
- needs another validation pass
- not ready

## Suggested Agent Checklist

1. create and prepare `./tmp`
2. look for repo-local reusable torrent and payload assets
3. run shared mode through `SUPERSEEDR_SHARED_CONFIG_DIR` using `cargo run`
4. direct positional magnet in shared mode through `cargo run`
5. shared file-layout smoke under `./tmp/superseedr-config/`
6. shared status and journal JSON smoke through `cargo run`
7. shared add and control smoke through `cargo run`
8. host-only vs shared settings persistence
9. sequential shared-role check on one machine
10. shared add flow validation
11. shared pause, resume, remove, and purge validation
12. shared file priority validation
13. shared read-path validation
14. shared TUI smoke through `cargo run`
15. migration script
16. concurrent shared-cluster validation if environment allows
17. focused launcher-config validation
18. Docker validation if available
19. resilience checks
20. docs review
21. final report

## Notes

- The agent should test the current checked-out code through `cargo run`.
- Do not use `set-shared-config` as the default shared-mode activation path for this plan.
- Prefer env-driven launches so validation does not replace a previously persisted launcher shared-config setting.
- Do not expand test permutations unnecessarily if a higher-priority shared-mode test already proves a critical defect.
- Prefer file and command evidence over narrative.
- When a failure happens, capture state immediately before continuing.
- Keep the final report concise but precise.
- If only one machine is available, Phase 1 and Phase 2 are still sufficient to produce a meaningful validation result for most of this branch.
- Default all shared-mode validation to the current directory's `./tmp` unless the environment explicitly requires another path.
- Normal-mode testing is intentionally de-prioritized in this agent plan and should be treated as manual or Docker-based follow-up work.
- Reuse repo-local assets whenever available; only generate temporary torrents or payload data under `./tmp` when reusable assets are absent.
