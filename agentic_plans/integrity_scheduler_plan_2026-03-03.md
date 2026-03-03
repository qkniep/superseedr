# No-Config Integrity Scheduler

## Summary
Replace the current fixed-interval full probe sweep with a dedicated integrity scheduler that runs continuously with bounded budgets. The scheduler should be automatic by default, with no new user-facing config, and should scale to very large torrent/file counts by doing incremental work instead of full rescans.

This design also leaves a clear path for future random small hash audits:
- metadata probing stays cheap and budgeted separately
- hash auditing uses manager-acquired `DiskRead` permits
- healthy torrents are checked conservatively
- unavailable torrents are prioritized for quick recovery detection

## Goals
- Scale to very large fleets, including hundreds of thousands or millions of files.
- Avoid bursty `probe everything every N seconds` behavior.
- Keep the user experience no-config by default.
- Preserve manager ownership of torrent manifests and probe/hash execution.
- Let app own global scheduling, prioritization, and policy.
- Be ready for future random hash sampling without redesigning again.

## Non-Goals
- No new TUI work in this phase.
- No full healthy-file list reporting on the hot path.
- No user-facing scheduler config section in `settings.toml`.
- No separate background-read permit class for now.

## High-Level Design

### 1. Add a dedicated integrity scheduler module
Introduce a new app-owned module, for example:
- `src/integrity_scheduler.rs`

Responsibilities:
- track per-torrent scheduling state
- choose which torrent to probe next
- enforce global fairness and bounded work
- prefer recovery work over healthy background work
- later schedule random hash audits

It should not know torrent file layouts itself. It only coordinates work.

### 2. Move from full sweeps to incremental batch work
Stop asking each manager to fully probe all files on each cycle.

Instead, app asks for bounded work:
- probe a batch of files
- later hash a bounded amount of data

Each torrent gets a rolling cursor:
- `next_probe_file_index`
- later `next_hash_cursor` or sampled-piece cursor

This keeps work proportional to budget, not total file count.

### 3. App owns scheduling; managers own execution
App/scheduler decides:
- when a torrent is due
- how much work to assign
- whether the system is in recovery-focused or background mode

Manager decides:
- how to probe files from its own manifest
- how to skip padding/skipped files
- how to read/hash when that feature is added
- what concrete problems were found

## API / Interface Changes

### Manager commands
Replace the current `probe all files` shape with bounded batch commands.

Add:
```rust
ManagerCommand::ProbeFileBatch {
    start_file_index: usize,
    max_files: usize,
}
```

Later add:
```rust
ManagerCommand::HashAuditBatch {
    budget_bytes: u64,
}
```

For this plan, `HashAuditBatch` is defined as future-facing but not implemented yet unless explicitly requested.

### Manager events
Replace full-status snapshots with batch results.

Add:
```rust
ManagerEvent::FileProbeBatchResult {
    info_hash: Vec<u8>,
    result: FileProbeBatchResult,
}
```

With:
```rust
struct FileProbeBatchResult {
    scanned_files: usize,
    next_file_index: usize,
    reached_end_of_manifest: bool,
    pending_metadata: bool,
    problem_files: Vec<FileProbeEntry>,
}
```

Keep `FileProbeEntry` as the problem-only payload:
```rust
struct FileProbeEntry {
    relative_path: PathBuf,
    absolute_path: PathBuf,
    error: StorageError,
    expected_size: u64,
    observed_size: Option<u64>,
}
```

Notes:
- `problem_files` contains only failing files.
- Healthy files are never returned on the background path.
- `pending_metadata` means `skip this torrent for now`.
- `error` should stay concrete so logs/UI can use real detail.

### App-side scheduler state
Add app-owned scheduling state per torrent, separate from user config and metrics.

Example shape:
```rust
struct IntegrityTorrentState {
    next_probe_file_index: usize,
    last_probe_started_at: Instant,
    last_probe_completed_at: Instant,
    last_full_probe_completed_at: Option<Instant>,
    pending_metadata: bool,
    known_problem_files: Vec<FileProbeEntry>,
    availability: DataAvailabilityState,
    priority_class: IntegrityPriorityClass,
    next_due_at: Instant,
}
```

Enums:
```rust
enum DataAvailabilityState {
    Available,
    Unavailable,
    Unknown,
}

enum IntegrityPriorityClass {
    Recovery,
    ActiveHealthy,
    IdleHealthy,
}
```

## Scheduling Policy

### 1. No-config default behavior
No new user-facing config is added.

The scheduler auto-runs with built-in defaults:
- healthy torrents: conservative background probing
- unavailable torrents: fast recovery probing
- future hash audits: only when the system has spare capacity

If an override is ever needed later, add one advanced internal escape hatch only after real data says it is necessary.

### 2. Frequent internal tick, bounded work
Run the scheduler on a small fixed tick, for example every `250ms` to `1s`.

Each tick has a bounded metadata probe budget:
- example conceptual budget: `up to N files worth of stat work`
- not `probe all due torrents`

The exact numeric defaults should be internal constants, not config.

Implementation detail:
- the scheduler should own explicit time in its state, not depend directly on wall clock in core logic
- production wiring can drive it from a real app interval
- unit tests should advance scheduler time manually, like the existing `Action::Tick { dt_ms }` pattern in torrent state
- core shape should therefore be something like `tick(dt, signals)` or `poll(now, signals)`, so overtime behavior is deterministic and cheap to test

### 3. Priority classes
Use three classes:

- `Recovery`
  - torrents currently marked unavailable
  - highest priority
  - target full-manifest revisit horizon: roughly `30s` to `2m`

- `ActiveHealthy`
  - healthy torrents with recent download/upload activity
  - medium priority
  - target horizon: tens of minutes

- `IdleHealthy`
  - healthy torrents without recent activity
  - lowest priority
  - target horizon: hours

This matches the chosen no-config, conservative policy.

### 4. Back off during heavy foreground work
Integrity work should yield when the client is busy.

Scheduler should consider:
- recent aggregate download/upload throughput
- active validations
- number of active disk reads/writes if cheaply available
- recent backlog/latency indicators if already exposed

Policy:
- if foreground disk activity is high, reduce or skip healthy background probing
- recovery probing still gets a minimum trickle budget
- future hash audits run only when the system is not busy

This is scheduler policy, not manager policy.

### 5. Million-file behavior
The scheduler must assume full sweeps can take a long time.

For very large fleets:
- do not try to finish every torrent on a short wall-clock interval
- advance cursors incrementally
- stretch healthy sweep horizons automatically
- keep recovery horizons tighter

The design scales because:
- memory tracks cursors and current problem files, not healthy manifests
- concurrency stays bounded
- work is proportional to budget, not cardinality

## Manager Execution Rules

### 1. Metadata probe batches
Manager implementation:
- starts at `start_file_index`
- scans up to `max_files`
- skips padding files internally
- omits skipped files from output
- collects only problem files
- returns `next_file_index` for continuation

Derivation of failures remains based on real filesystem state:
- missing file
- inaccessible file
- wrong type
- size mismatch

### 2. Hash auditing
Future hash auditing should:
- be initiated by app/scheduler
- be executed by manager
- use manager-acquired `DiskRead` permits
- read small bounded samples only
- never bypass normal read-permit discipline

Chosen default:
- reuse existing `DiskRead` permits
- rely on scheduler budgets and low scheduling priority to keep it safe

### 3. No manager-local timers
Remove manager-owned recurring probe timers in the scalable design.

Managers should be passive executors:
- receive batch commands
- perform bounded work
- return results

That avoids overlapping independent timers and gives app full control.

## Logging and State Transitions

### 1. Logging
Keep transition-only logging in app:
- unavailable transition: one warning with saved location and all problem files
- recovery transition: one info

Do not log every batch.

### 2. Data availability
App determines `data_available` from the current known problem set:
- if any non-skipped problem files are currently known, mark unavailable
- when a full pass completes with no problems, mark available

Important detail:
- do not mark a torrent healthy just because one partial batch found no issues
- only clear unavailability after a completed full-manifest pass with zero problems

This avoids false recovery on partial scans.

### 3. Problem-file state
App keeps the latest known problem-file set for each torrent.

Because background results are problem-only:
- update the set incrementally during a sweep
- when a full pass completes, replace the old set with the newly accumulated set
- availability transitions are based on that completed-pass result

## Resource / Performance Model

### Metadata probing
- Uses `fs::metadata(...)`
- Does not acquire `DiskRead` permits
- Is controlled by scheduler batch size and cadence
- Bounded concurrency must remain low and centralized

### Hash auditing
- Uses manager `DiskRead` permits
- Is scheduled only when system load allows
- Bounded by byte budgets, not file counts

### File descriptor safety
The scheduler must never fan out per-file work unboundedly.

Rules:
- no per-file spawned storm
- bounded number of in-flight manager batch requests
- managers process batch items sequentially or with very small internal concurrency
- hash reads open/read/close promptly

## Implementation Steps

### Phase 1. Introduce scheduler scaffolding
- Add `integrity_scheduler` module.
- Add app-owned per-torrent scheduler state.
- Keep current availability policy and logs.
- Replace `request_torrent_file_probes()` timer behavior with scheduler tick wiring.

### Phase 2. Convert manager API to batch probing
- Replace `ManagerCommand::ProbeFiles` with `ProbeFileBatch`.
- Replace `ManagerEvent::FileProbeStatus` with `FileProbeBatchResult`.
- Refactor manager probing to return bounded slices and cursor progress.
- Preserve current problem-file detection logic and `StorageError` payloads.

### Phase 3. Add completed-sweep availability semantics
- Accumulate problem files over a full pass.
- Only update availability on completed-pass boundaries.
- Preserve transition-only logging.

### Phase 4. Add load-aware throttling
- Feed scheduler recent app/system activity signals.
- Suppress healthy probing under heavy load.
- Guarantee a small minimum budget for recovery class.

### Phase 5. Future hash audit extension
- Add `HashAuditBatch`.
- Use existing `DiskRead` permits in manager.
- Add scheduler byte budgets and idle-only policy.

## Test Cases and Scenarios

### Scheduler unit tests
- Recovery torrents are scheduled before healthy torrents.
- Healthy probing backs off when app reports heavy activity.
- Idle healthy torrents get much slower revisit horizons than recovery torrents.
- Scheduler respects per-tick budgets and advances cursors incrementally.
- Partial batch results do not clear unavailable state.
- Scheduler tests use explicit/manual time, not real sleeps or Tokio time control.
- Large-scale scheduler tests use synthetic torrent/file counts and cursors, not real files on disk.
- A `1_000_000`-file synthetic fleet test confirms bounded work per tick and forward progress over many ticks.

### Manager probe tests
- Batch probing returns only problem files.
- Batch probing skips padding files.
- Batch probing advances `next_file_index` correctly.
- End-of-manifest is reported correctly.
- Pending metadata returns `pending_metadata = true`.

### App integration tests
- A torrent with one missing file becomes unavailable only after a completed pass that includes the file.
- A previously unavailable torrent becomes available only after a full completed pass with no problems.
- Transition logging fires once on unavailable and once on recovery.
- Large torrents do not require storing healthy file entries.

### Future hash tests
- Hash audit batch acquires `DiskRead` permits.
- Hash audits do not run when scheduler marks system as busy.
- Hash byte budgets are enforced across ticks.

## Assumptions and Defaults
- No new user-facing config is added in `settings.toml`.
- Healthy background integrity work is conservative by default.
- Unavailable torrents are prioritized for faster recovery detection.
- Metadata probing stays outside the read-permit pool.
- Future hash audits use the existing `DiskRead` permit pool.
- Manager remains owner of torrent manifests and low-level probe/hash execution.
- App remains owner of scheduling, policy, availability transitions, and logging.
- Background probe payloads contain only problem files, never full healthy file lists.
