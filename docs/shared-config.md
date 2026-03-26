# Shared Config Cluster Mode

## Overview
Shared config mode is now a layered cluster mode.

- Every node is a full Superseedr client.
- Every node can run torrents, accept peers, and seed.
- One node is still the leader.
- The leader is the only node allowed to mutate shared desired state and consume the shared inbox.
- Other nodes follow leader-written shared catalog changes and apply them locally.

Shared mode is enabled from the first available source in this order:

1. `SUPERSEEDR_SHARED_CONFIG_DIR`
2. persisted launcher sidecar config set by the CLI
3. normal mode

This makes installed browser and OS protocol launches work even when they do not
inherit your shell environment.

## Environment Variables

### `SUPERSEEDR_SHARED_CONFIG_DIR`
Absolute path to the shared network-drive root.

Superseedr automatically places all shared config and runtime artifacts under a
`superseedr-config/` subdirectory inside that root.

Shared download paths are resolved relative to the mounted share root itself, not
relative to `superseedr-config/`.

Shared mode uses:

- `settings.toml`
- `catalog.toml`
- `torrent_metadata.toml`
- `hosts/<host-id>/config.toml`

Example:

```bash
SUPERSEEDR_SHARED_CONFIG_DIR=/mnt/shared-drive
```

This still has highest precedence and overrides any persisted launcher config.

### `SUPERSEEDR_SHARED_HOST_ID`
Optional explicit host id for selecting `hosts/<host-id>/config.toml`.

If unset, Superseedr falls back to a sanitized hostname.

`SUPERSEEDR_HOST_ID` is still accepted as a legacy fallback, but
`SUPERSEEDR_SHARED_HOST_ID` is the canonical name.

Example:

```bash
SUPERSEEDR_SHARED_HOST_ID=seedbox-a
```

## Launcher Commands

Use these commands to persist shared mode in the normal per-user config area
without editing runtime `settings.toml`:

```bash
superseedr set-shared-config /mnt/shared-drive
superseedr show-shared-config
superseedr clear-shared-config
```

Rules:

- `set-shared-config` requires an absolute path
- the path may be either the mount root or an explicit `.../superseedr-config`
- Superseedr normalizes and stores the mount root in a dedicated launcher sidecar file
- `show-shared-config` reports the effective source and resolved paths
- `clear-shared-config` disables persisted shared mode unless the env var is still set

Examples:

```bash
superseedr set-shared-config /mnt/shared-drive
superseedr set-shared-config /mnt/shared-drive/superseedr-config
```

## Shared Root Layout

```text
/mnt/shared-drive/
  superseedr-config/
    settings.toml
    catalog.toml
    torrent_metadata.toml
    cluster.revision
    hosts/
      seedbox-a/
        config.toml
        logs/
        persistence/
        status.json
      desktop-a/
        config.toml
        logs/
        persistence/
        status.json
    torrents/
    inbox/
    processed/
    staged-adds/
    status/
      leader.json
    superseedr.lock
  downloads/
  library/
```

Each host can mount that same shared drive at a different local path, for example:

- Windows: `C:\Users\jagat\Documents\seedbox`
- macOS: `/Volumes/seedbox`

Each host may either:

- set `SUPERSEEDR_SHARED_CONFIG_DIR` to its local mount root, or
- run `superseedr set-shared-config <mount-root>` once for that user account

Superseedr will then use `<mount-root>/superseedr-config/` automatically.

Examples:

- `default_download_folder = ""` resolves to the mounted share root itself
- `default_download_folder = "downloads"` resolves to `<mount-root>/downloads`

## Layered Files

### `settings.toml`
Cluster-wide shared global settings.

Leader-written:
- shared `client_id` default
- RSS settings
- shared UI and performance settings
- shared default download folder

### `catalog.toml`
Cluster-wide desired torrent state.

Leader-written:
- torrent list
- pause/resume/delete outcomes
- per-torrent download path
- per-torrent file priorities

All nodes:
- read this file
- converge local runtime to it

### `torrent_metadata.toml`
Leader-written derived torrent metadata.

### `hosts/<host-id>/config.toml`
Host-local runtime settings.

This file is bootstrapped automatically on first load if it does not exist yet.

Kept fields:
- optional `client_id`
- `client_port`
- `watch_folder`

Removed from shared mode:
- `path_roots`

### `hosts/<host-id>/`
Host-local runtime artifacts stored on the shared root.

Examples:
- `hosts/<host-id>/logs/`
- `hosts/<host-id>/persistence/network_history.bin`
- `hosts/<host-id>/persistence/activity_history.bin`
- `hosts/<host-id>/persistence/rss.toml`
- `hosts/<host-id>/persistence/event_journal.toml`
- `hosts/<host-id>/status.json`

## Leadership

Shared mode still uses the existing file-lock mechanism.

- Leader: holds `/shared-root/superseedr.lock`
- Non-leader node: does not hold the lock
- If a non-leader later acquires the lock at runtime, it promotes itself to leader and starts leader-only services without requiring a restart.

The leader is responsible for:

- consuming `/shared-root/inbox/`
- mutating shared desired state
- writing `settings.toml`, `catalog.toml`, and `torrent_metadata.toml`
- writing `cluster.revision`

All nodes are still active clients.

## Cluster Convergence

The leader writes `cluster.revision` after shared desired state changes.

Host-only changes do not bump `cluster.revision`.

Non-leader nodes:

- watch the shared root
- reload shared layered config when `cluster.revision` changes
- converge local runtime to the new shared catalog

Convergence includes:

- starting newly added torrents
- pausing or resuming torrents
- applying file-priority and path changes
- deleting torrents removed from the shared catalog

Delete is cluster-wide:

- once the leader removes a torrent from `catalog.toml`, every node removes it locally
- remove-only delete removes the torrent from all nodes but keeps payload data
- purge delete marks the torrent for deletion in shared desired state, then every node deletes payload data before the leader removes the catalog entry

## Watch Folder Model

Each host may still define its own local ingress folder:

```toml
client_port = 6681
watch_folder = "/srv/local-watch"
```

Behavior:

- Leader watches its own local `watch_folder`
- Leader also watches `/shared-root/inbox/`
- Non-leader nodes watch their own local `watch_folder`
- Non-leader nodes relay supported files from their local watch folder into `/shared-root/inbox/`
- Non-leader nodes do not mutate shared desired state directly from local ingress

Supported dropped file types:

- `.torrent`
- `.magnet`
- `.path`
- `.control`

Notes:

- follower `.torrent` adds may stage the torrent file into `/shared-root/staged-adds/` before queuing a leader request
- follower `.path` adds require a shared-mode `default_download_folder`; the follower resolves the referenced torrent file locally and stages the actual `.torrent` for the leader

## Data Path Rules

Shared mode requires all torrent payload data to live under the shared root.

Rules:

- shared-mode download paths must resolve inside the shared root
- shared-mode download paths are stored root-relative in layered config
- host-specific path translation is not supported

Example:

```toml
# settings.toml
default_download_folder = ""
```

```toml
# catalog.toml
[[torrents]]
name = "Shared Collection"
download_path = "library/shared-collection"
```

At runtime, those resolve under the effective shared mount root, whether it came
from `SUPERSEEDR_SHARED_CONFIG_DIR` or the persisted launcher config.

## Shared Mutation Model

Shared mutations always go through the leader.

CLI and nodes may queue:

- add requests
- pause requests
- resume requests
- delete requests
- priority requests
- shared config edits remain leader-only in the UI for now

Those requests are dropped into `/shared-root/inbox/`.

The leader consumes them and writes the resulting desired state into the layered shared config.

Non-leader nodes never call direct shared `save_settings`.

Follower config editing rules:

- shared settings and RSS edits remain leader-only
- host-local `client_id`, `client_port`, and `watch_folder` edits are allowed on followers and save only to `hosts/<host-id>/config.toml`

## Status Files

Shared mode now writes per-node status files:

- `/shared-root/hosts/<host-id>/status.json`

This avoids active nodes overwriting a single shared status snapshot.

`superseedr status` in shared mode reads the current node's status file.

## Example

Shared root:

```text
/srv/shared-drive/
  superseedr-config/
    settings.toml
    catalog.toml
    torrent_metadata.toml
    cluster.revision
    hosts/
      seedbox-a/
        config.toml
        logs/
        persistence/
        status.json
      desktop-a/
        config.toml
        logs/
        persistence/
        status.json
    torrents/
    inbox/
    processed/
    status/
      leader.json
    data/
    superseedr.lock
```

Shared settings:

```toml
# settings.toml
client_id = "shared-node"
default_download_folder = "downloads"
global_upload_limit_bps = 8000000
```

Shared catalog:

```toml
# catalog.toml
[[torrents]]
name = "Shared Collection"
torrent_or_magnet = "shared:torrents/0123456789abcdef0123456789abcdef01234567.torrent"
download_path = "downloads/shared-collection"
```

Host config:

```toml
# hosts/seedbox-a/config.toml
client_port = 6681
watch_folder = "/srv/local-watch"
```

## Notes

- Shared mode is opt-in.
- Environment configuration still overrides persisted launcher configuration.
- The layered file layout is preserved.
- Shared desired state still has a single writer.
- Runtime torrent activity is cluster-wide across all nodes.
- Shared mode does not use multi-writer reconciliation.
