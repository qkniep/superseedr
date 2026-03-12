# Zero-Config Multi-Instance Scaling

## Summary
Explore a future Superseedr feature where multiple instances can cooperate over a shared torrent library and behave like one logical system. The core value is not "distributed systems for its own sake", but zero-manual-sharding scale-out for serious operators who already have shared storage, containers, and stable infrastructure.

The first useful version should stay narrow:
- shared storage is assumed
- one active owner per torrent
- automatic ownership, failover, and rebalance
- no manual shard maps
- no separate operator control plane required for basic use

This plan is intentionally long-lived and high level. It should be refined over time before implementation begins.

## Product Intent
- Make Superseedr stand out as a self-scaling torrent system, not only a fast terminal client.
- Preserve the small-scale "just run it" experience while allowing large installations to add workers without manual sharding.
- Target serious operators honestly: shared NAS, container volumes, stable mounts, and multiple hosts are acceptable assumptions.

## High-Level Goals
- Support multiple instances operating against one logical torrent catalog.
- Keep the user experience no-config at the torrent-placement level.
- Avoid requiring a permanent master node for normal operation.
- Ensure one torrent can be cleanly reassigned after instance failure.
- Keep private-tracker safety and single-owner semantics as first-class constraints.
- Create an architecture that can later support replicated seeding for completed torrents.
- Build toward very large libraries over time, including million-class fleets, without needing a full product reset later.

## Non-Goals For The First Version
- No cooperative downloading of a single incomplete torrent across multiple instances.
- No blockchain-style consensus or hostile-node trust model.
- No requirement that the first version be fully peer-to-peer and coordination-free.
- No fully automatic cross-machine data movement when storage is not shared.
- No promise that one process today can simply be scaled to extreme torrent counts without broader refactors.

## Target User
- Operators with serious storage and automation already in place.
- Users comfortable with Docker, shared volumes, NAS, or clustered filesystems.
- Users who care more about operational simplicity than about avoiding all infrastructure assumptions.

## Core Assumptions
- Multiple instances can access the same underlying torrent payloads through shared storage.
- Instances may run on the same machine or on multiple machines.
- Each instance has its own transient runtime identity, port, peer sessions, logs, and metrics.
- The system should act like one logical client from the operator's point of view.

## Why Shared Storage Is Acceptable
For the target scale, shared storage is not a weakness. It is a normal infrastructure assumption. The zero-config promise should mean:
- zero manual torrent sharding
- zero manual failover choreography
- zero hand-maintained ownership maps

It does not need to mean:
- zero infrastructure
- zero mounts
- zero shared storage

## Architectural Principles

### 1. Separate Shared Desired State From Per-Instance Runtime State
Do not let multiple instances mutate the current single-process snapshot model directly.

Shared state should eventually include:
- torrent catalog
- user-facing torrent settings and desired policies
- ownership or lease metadata

Per-instance runtime state should include:
- local peer sessions
- transient metrics
- activity and network history
- instance identity and health
- logs and local diagnostics

### 2. Prefer Single-Owner Execution First
The simplest correct cluster model is:
- one active owner per torrent
- a dead owner loses its claim
- another instance can resume ownership

This keeps tracker behavior, safety, and recovery understandable.

### 3. Make Placement Automatic
The operator should not have to pick which instance runs each torrent.

The system should eventually decide:
- who owns an unclaimed torrent
- when ownership should move
- how to rebalance when workers join or leave

### 4. Start With Strong Coordination, Not Fancy Coordination
It is acceptable to start with a simple authoritative coordination mechanism if it keeps behavior correct and debuggable. Avoid over-optimizing for "fully decentralized" before the feature proves value.

### 5. Design For Future Replicated Seeding
Later, completed torrents may support multiple simultaneous hosts for load sharing. This should be treated as a separate policy from ordinary single-owner execution.

## Candidate Coordination Shapes

### Option A: Shared Embedded Control Plane
Examples:
- SQLite on shared storage
- file-lock-backed journals

Pros:
- simpler deployment
- strong enough coordination for leases and ownership
- easier to reason about than gossip-only ownership

Cons:
- still a control plane
- shared filesystem semantics matter

### Option B: External Database Control Plane
Examples:
- PostgreSQL

Pros:
- stronger long-term scale path
- operationally explicit
- better for large catalogs and observability

Cons:
- higher setup cost
- weaker "drop in another worker" story

### Option C: Fully Masterless Membership And Derived Ownership
Examples:
- gossip membership
- rendezvous hashing
- lease-like local claims

Pros:
- no permanent master
- conceptually elegant

Cons:
- harder to guarantee safe exclusive ownership
- higher split-brain risk
- more dangerous for private-tracker correctness

### Current Bias
Initial versions should favor correctness and operability over elegance. A simple embedded or shared authoritative coordination layer is likely the most practical first step.

## Execution Modes To Consider

### Phase 1 Mode: Exclusive Ownership
- one active owner per torrent
- other instances do not run it
- failover occurs by releasing or expiring ownership

### Later Mode: Replicated Seeding
- completed torrents may have multiple seed hosts
- each host must have access to the full payload
- intended for load sharing or network diversity

### Much Later Mode: Cooperative Downloading
- multiple instances jointly download one incomplete torrent
- likely a major distributed-systems feature
- explicitly out of scope for initial versions

## Major Design Areas

### 1. Torrent Catalog
Questions:
- What is the durable source of truth for "all torrents known to the system"?
- How are add, remove, pause, move, and priority changes expressed safely?

### 2. Ownership And Leases
Questions:
- How is a torrent claimed?
- How does an instance renew ownership?
- How quickly does failover occur after instance death?
- What prevents duplicate active owners?

### 3. Instance Identity And Membership
Questions:
- How are instances identified?
- How are they discovered?
- How are dead workers detected?
- Should workers be considered equivalent, or can they advertise capabilities?

### 4. Shared Storage Assumptions
Questions:
- Must all instances see the same path?
- How strict should path normalization and storage-root identity be?
- How do we detect misconfigured mounts early and loudly?

### 5. Safety For Private Trackers
Questions:
- Can the cluster present one logical identity or must each worker be distinct?
- How do we guarantee single-owner semantics for tracker-facing behavior?
- How do we avoid duplicate announces during ownership transitions?

### 6. Observability
Questions:
- How does an operator see which instance owns which torrents?
- How are stuck leases, failed rebalances, and failover events surfaced?
- Should there be a cluster summary view in TUI, CLI, or external status output?

## Broader Scalability Considerations
The new integrity probe work only solves one scaling axis. Multi-instance scale will eventually require revisiting other areas too, especially if torrent counts become very large:
- whole-library metric draining and torrent list resorting
- per-second telemetry passes across all torrents
- persistence that rebuilds or clones large in-memory snapshots
- startup validation behavior that still touches full layouts
- one-manager-task-per-torrent runtime model

This means the cluster feature should not be treated as isolated from broader catalog and runtime scalability work.

## Suggested Phasing

### Phase 0: Research And Constraints
- Document assumptions about shared storage, private trackers, and ownership safety.
- Decide whether the first control plane is embedded/shared or external.
- Define what "zero-config" means operationally.

### Phase 1: Shared Catalog + Single-Owner Execution
- Multiple instances point at the same logical catalog.
- One owner per torrent.
- Ownership is automatic.
- Dead instance ownership expires and is recoverable.

### Phase 2: Rebalance And Capacity Growth
- New instances automatically take work.
- Existing instances shed work cleanly.
- Ownership movement is visible and auditable.

### Phase 3: Better Operator Visibility
- Cluster-oriented status output.
- Ownership and failover diagnostics.
- Clear surfacing of unhealthy workers or stuck claims.

### Phase 4: Replicated Seeding
- Completed torrents can have multiple hosts.
- Replica count and placement policy become explicit.
- Keep this separate from ordinary single-owner downloading.

### Phase 5: Million-Class Library Hardening
- Reduce full-library passes.
- Revisit persistence shape and cold-state handling.
- Consider catalog/index structures suitable for very large fleets.

## Success Criteria For A First Version
- Adding another worker requires no manual torrent-to-worker mapping.
- A worker crash does not require manual recovery of all its torrents.
- One torrent is not accidentally run by multiple owners during steady state.
- The operator can understand current ownership with minimal effort.
- The system remains honest about its assumptions: shared storage and serious infrastructure are expected.

## Open Questions
- Is the first coordination layer embedded/shared or external?
- How strict should lease expiration be for tracker safety versus fast failover?
- What is the minimum operator-visible surface needed to build trust?
- Should the first rollout be public-torrent-first, with private-tracker support only after stricter safeguards?
- When does it become worth separating hot runtime state from cold catalog state?

## Current Position
This idea is ambitious but not crazy. It is a plausible long-term signature feature if kept narrow at first:
- shared storage
- single-owner execution
- automatic failover
- no manual sharding

The main risk is not that the idea is unsound. The main risk is trying to solve too many distributed-systems problems in the first iteration.
