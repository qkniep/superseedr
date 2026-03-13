#!/usr/bin/env python3
"""One-time migration from legacy settings.toml to layered shared config files.

This script reads a legacy flat settings file, writes shared `settings.toml`,
shared `catalog.toml`, and `hosts/<host-id>.toml`, and copies file-based torrent
artifacts into `<shared-root>/torrents/<info-hash>.torrent` when possible.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tomllib
from pathlib import Path
from typing import Any

SHARED_TORRENT_PREFIX = "shared:"
HEX_INFO_HASH_LENGTH = 40

SHARED_SETTING_KEYS = [
    "client_id",
    "lifetime_downloaded",
    "lifetime_uploaded",
    "private_client",
    "torrent_sort_column",
    "torrent_sort_direction",
    "peer_sort_column",
    "peer_sort_direction",
    "ui_theme",
    "default_download_folder",
    "max_connected_peers",
    "bootstrap_nodes",
    "global_download_limit_bps",
    "global_upload_limit_bps",
    "max_concurrent_validations",
    "connection_attempt_permits",
    "resource_limit_override",
    "upload_slots",
    "peer_upload_in_flight_limit",
    "tracker_fallback_interval_secs",
    "client_leeching_fallback_interval_secs",
    "output_status_interval",
    "rss",
]

HOST_SETTING_KEYS = ["client_port", "watch_folder"]
TORRENT_KEYS = [
    "torrent_or_magnet",
    "name",
    "validation_status",
    "download_path",
    "container_name",
    "torrent_control_state",
    "file_priorities",
]


class MigrationError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Migrate a legacy settings.toml into layered shared-config files."
    )
    parser.add_argument("--input", required=True, help="Path to the legacy settings.toml")
    parser.add_argument(
        "--shared-root",
        required=True,
        help="Destination shared config root for settings.toml/catalog.toml/hosts/",
    )
    parser.add_argument("--host-id", required=True, help="Host id for hosts/<host-id>.toml")
    parser.add_argument(
        "--path-root",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="Portable root mapping used to convert download paths",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Overwrite existing layered config files if they already exist",
    )
    return parser.parse_args()


def parse_path_roots(values: list[str]) -> dict[str, Path]:
    roots: dict[str, Path] = {}
    for value in values:
        if "=" not in value:
            raise MigrationError(f"Invalid --path-root '{value}'. Use NAME=PATH.")
        name, raw_path = value.split("=", 1)
        name = name.strip()
        raw_path = raw_path.strip()
        if not name or not raw_path:
            raise MigrationError(f"Invalid --path-root '{value}'. Use NAME=PATH.")
        roots[name] = Path(raw_path).expanduser()
    return roots


def toml_quote(value: str) -> str:
    return json.dumps(value)


def inline_toml_value(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return toml_quote(value)
    if value is None:
        raise MigrationError("None cannot be serialized inline in TOML")
    if isinstance(value, list):
        return "[" + ", ".join(inline_toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        parts = [f"{key} = {inline_toml_value(val)}" for key, val in value.items()]
        return "{ " + ", ".join(parts) + " }"
    raise MigrationError(f"Unsupported TOML value: {value!r}")


def portable_relative(path: Path) -> str:
    return "/".join(part for part in path.parts if part not in (path.anchor, ""))


def encode_path_ref(raw_value: Any, path_roots: dict[str, Path]) -> Any:
    if not raw_value:
        return None
    path = Path(str(raw_value)).expanduser()

    matches: list[tuple[int, str, Path]] = []
    for root_name, root_path in path_roots.items():
        try:
            relative = path.relative_to(root_path)
        except ValueError:
            continue
        matches.append((len(root_path.parts), root_name, relative))

    if matches:
        _, root_name, relative = max(matches, key=lambda item: item[0])
        return {"root": root_name, "relative": portable_relative(relative)}
    return str(path)


def info_hash_stem(path: Path) -> str | None:
    stem = path.stem.lower()
    if len(stem) == HEX_INFO_HASH_LENGTH and all(ch in "0123456789abcdef" for ch in stem):
        return stem
    return None


def write_if_allowed(path: Path, content: str, force: bool) -> None:
    if path.exists() and not force:
        raise MigrationError(f"Refusing to overwrite existing file: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="\n")


def write_key_values(lines: list[str], data: dict[str, Any], keys: list[str]) -> None:
    for key in keys:
        if key not in data:
            continue
        value = data[key]
        if value is None:
            continue
        lines.append(f"{key} = {inline_toml_value(value)}")


def write_shared_settings(path: Path, settings_data: dict[str, Any], force: bool) -> None:
    lines: list[str] = []
    scalar_data = {key: settings_data[key] for key in SHARED_SETTING_KEYS if key != "rss" and key in settings_data}
    write_key_values(lines, scalar_data, [key for key in SHARED_SETTING_KEYS if key != "rss"])

    rss = settings_data.get("rss")
    if isinstance(rss, dict):
        if lines:
            lines.append("")
        lines.append("[rss]")
        write_key_values(lines, rss, ["enabled", "poll_interval_secs", "max_preview_items"])

        feeds = rss.get("feeds") or []
        for feed in feeds:
            lines.append("")
            lines.append("[[rss.feeds]]")
            write_key_values(lines, feed, ["url", "enabled"])

        filters = rss.get("filters") or []
        for filter_item in filters:
            lines.append("")
            lines.append("[[rss.filters]]")
            write_key_values(lines, filter_item, ["query", "mode", "enabled"])

    write_if_allowed(path, "\n".join(lines).strip() + "\n", force)


def write_catalog(path: Path, torrents: list[dict[str, Any]], force: bool) -> None:
    lines: list[str] = []
    for index, torrent in enumerate(torrents):
        if index:
            lines.append("")
        lines.append("[[torrents]]")
        write_key_values(lines, torrent, TORRENT_KEYS)
    content = ("\n".join(lines).strip() + "\n") if lines else ""
    write_if_allowed(path, content, force)


def write_host(path: Path, host_data: dict[str, Any], force: bool) -> None:
    lines: list[str] = []
    write_key_values(lines, host_data, HOST_SETTING_KEYS + ["client_id"])

    path_roots = host_data.get("path_roots") or {}
    if path_roots:
        if lines:
            lines.append("")
        lines.append("[path_roots]")
        for key, value in path_roots.items():
            lines.append(f"{key} = {inline_toml_value(value)}")

    write_if_allowed(path, "\n".join(lines).strip() + "\n", force)


def migrate_torrents(
    torrents: list[dict[str, Any]],
    shared_root: Path,
    path_roots: dict[str, Path],
) -> tuple[list[dict[str, Any]], list[str]]:
    warnings: list[str] = []
    shared_torrents_dir = shared_root / "torrents"
    shared_torrents_dir.mkdir(parents=True, exist_ok=True)

    migrated: list[dict[str, Any]] = []
    for torrent in torrents:
        migrated_torrent = {key: torrent.get(key) for key in TORRENT_KEYS if key in torrent}
        source = str(torrent.get("torrent_or_magnet", ""))
        if source.startswith("magnet:"):
            migrated_torrent["torrent_or_magnet"] = source
        elif source:
            source_path = Path(source).expanduser()
            stem = info_hash_stem(source_path)
            if stem and source_path.exists():
                destination = shared_torrents_dir / f"{stem}.torrent"
                shutil.copy2(source_path, destination)
                migrated_torrent["torrent_or_magnet"] = f"{SHARED_TORRENT_PREFIX}torrents/{destination.name}"
            else:
                if not stem:
                    warnings.append(
                        f"Skipped shared .torrent copy for '{source}' because the filename stem is not a 40-char hex info hash."
                    )
                elif not source_path.exists():
                    warnings.append(
                        f"Skipped shared .torrent copy for '{source}' because the source file does not exist."
                    )
                migrated_torrent["torrent_or_magnet"] = source
        if "download_path" in migrated_torrent:
            migrated_torrent["download_path"] = encode_path_ref(
                migrated_torrent.get("download_path"),
                path_roots,
            )
        migrated.append(migrated_torrent)

    return migrated, warnings


def main() -> int:
    args = parse_args()
    legacy_settings_path = Path(args.input).expanduser()
    shared_root = Path(args.shared_root).expanduser()
    path_roots = parse_path_roots(args.path_root)

    if not legacy_settings_path.exists():
        raise MigrationError(f"Legacy settings file does not exist: {legacy_settings_path}")

    with legacy_settings_path.open("rb") as handle:
        legacy = tomllib.load(handle)

    shared_settings: dict[str, Any] = {
        key: legacy[key] for key in SHARED_SETTING_KEYS if key in legacy
    }
    if "default_download_folder" in shared_settings:
        shared_settings["default_download_folder"] = encode_path_ref(
            shared_settings.get("default_download_folder"),
            path_roots,
        )

    host_data: dict[str, Any] = {key: legacy[key] for key in HOST_SETTING_KEYS if key in legacy}
    host_data["path_roots"] = {key: str(value) for key, value in path_roots.items()}

    torrents, warnings = migrate_torrents(
        legacy.get("torrents") or [],
        shared_root,
        path_roots,
    )

    settings_path = shared_root / "settings.toml"
    catalog_path = shared_root / "catalog.toml"
    host_path = shared_root / "hosts" / f"{args.host_id}.toml"

    write_shared_settings(settings_path, shared_settings, args.force)
    write_catalog(catalog_path, torrents, args.force)
    write_host(host_path, host_data, args.force)

    print(f"Wrote shared settings to: {settings_path}")
    print(f"Wrote shared catalog to: {catalog_path}")
    print(f"Wrote host config to: {host_path}")
    if warnings:
        print("\nWarnings:")
        for warning in warnings:
            print(f"- {warning}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except MigrationError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
