# Shared Config Cluster Mode

## Overview
Shared config mode is now a layered cluster mode.

- Every node is a full Superseedr client.
- Every node can run torrents, accept peers, and seed.
- One node is still the leader.
- The leader is the only node allowed to mutate shared desired state and consume the shared inbox.
- Other nodes follow leader-written shared catalog changes and apply them locally.

Shared mode is enabled only when `SUPERSEEDR_SHARED_CONFIG_DIR` is set.

## Environment Variables

### `SUPERSEEDR_SHARED_CONFIG_DIR`
Absolute path to the shared cluster root.

Shared mode uses:

- `settings.toml`
- `catalog.toml`
- `torrent_metadata.toml`
- `hosts/<host-id>.toml`

Example:

```bash
SUPERSEEDR_SHARED_CONFIG_DIR=/mnt/superseedr-root
```

### `SUPERSEEDR_SHARED_HOST_ID`
Optional explicit host id for selecting `hosts/<host-id>.toml`.

If unset, Superseedr falls back to a sanitized hostname.

`SUPERSEEDR_HOST_ID` is still accepted as a legacy fallback, but
`SUPERSEEDR_SHARED_HOST_ID` is the canonical name.

Example:

```bash
SUPERSEEDR_SHARED_HOST_ID=seedbox-a
```

## Shared Root Layout

```text
/mnt/superseedr-root/
  settings.toml
  catalog.toml
  torrent_metadata.toml
  cluster.revision
  hosts/
    seedbox-a.toml
    desktop-a.toml
  torrents/
  inbox/
  processed/
  staged-adds/
  status/
    seedbox-a.json
    desktop-a.json
  data/
  superseedr.lock
```

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

### `hosts/<host-id>.toml`
Host-local runtime settings.

This file is bootstrapped automatically on first load if it does not exist yet.

Kept fields:
- optional `client_id`
- `client_port`
- `watch_folder`

Removed from shared mode:
- `path_roots`

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
default_download_folder = "data/downloads"
```

```toml
# catalog.toml
[[torrents]]
name = "Shared Collection"
download_path = "data/downloads/shared-collection"
```

At runtime, those resolve under `SUPERSEEDR_SHARED_CONFIG_DIR`.

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
- host-local `client_id`, `client_port`, and `watch_folder` edits are allowed on followers and save only to `hosts/<host-id>.toml`

## Status Files

Shared mode now writes per-node status files:

- `/shared-root/status/<host-id>.json`

This avoids active nodes overwriting a single shared status snapshot.

`superseedr status` in shared mode reads the current node's status file.

## Example

Shared root:

```text
/srv/superseedr-root/
  settings.toml
  catalog.toml
  torrent_metadata.toml
  cluster.revision
  hosts/
    seedbox-a.toml
    desktop-a.toml
  torrents/
  inbox/
  processed/
  status/
    seedbox-a.json
    desktop-a.json
  data/
  superseedr.lock
```

Shared settings:

```toml
# settings.toml
client_id = "shared-node"
default_download_folder = "data/downloads"
global_upload_limit_bps = 8000000
```

Shared catalog:

```toml
# catalog.toml
[[torrents]]
name = "Shared Collection"
torrent_or_magnet = "shared:torrents/0123456789abcdef0123456789abcdef01234567.torrent"
download_path = "data/downloads/shared-collection"
```

Host config:

```toml
# hosts/seedbox-a.toml
client_port = 6681
watch_folder = "/srv/local-watch"
```

## Notes

- Shared mode is opt-in.
- The layered file layout is preserved.
- Shared desired state still has a single writer.
- Runtime torrent activity is cluster-wide across all nodes.
- Shared mode does not use multi-writer reconciliation.
