from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlsplit

from integration_tests.harness.config import resolve_paths


@dataclass(frozen=True)
class ClusterFixture:
    id: str
    mode: str
    torrent_path: Path
    payload_path: Path
    info_hash_hex: str
    magnet_uri: str
    representative_relative_path: str


def manifest_path() -> Path:
    return resolve_paths().integration_root / "cluster_cli" / "fixtures" / "manifest.json"


def load_fixture_manifest() -> list[ClusterFixture]:
    payload = json.loads(manifest_path().read_text(encoding="utf-8"))
    root = resolve_paths().root
    fixtures: list[ClusterFixture] = []
    for entry in payload["fixtures"]:
        fixtures.append(
            ClusterFixture(
                id=entry["id"],
                mode=entry["mode"],
                torrent_path=root / entry["torrent"],
                payload_path=root / entry["payload"],
                info_hash_hex=entry["info_hash_hex"],
                magnet_uri=entry["magnet_uri"],
                representative_relative_path=entry["representative_relative_path"],
            )
        )
    return fixtures


def fixture_by_id(fixture_id: str) -> ClusterFixture:
    for fixture in load_fixture_manifest():
        if fixture.id == fixture_id:
            return fixture
    raise KeyError(f"Unknown cluster fixture '{fixture_id}'")


def magnet_info_hash_hex(magnet_uri: str) -> str:
    parsed = urlsplit(magnet_uri)
    if parsed.scheme != "magnet":
        raise ValueError(f"Expected magnet URI, got '{magnet_uri}'")
    xt_values = parse_qs(parsed.query).get("xt", [])
    prefix = "urn:btih:"
    for value in xt_values:
        if value.startswith(prefix):
            return value[len(prefix) :].lower()
    raise ValueError(f"Magnet URI is missing btih xt parameter: '{magnet_uri}'")


def torrent_info_hash_hex(torrent_path: Path) -> str:
    raw = torrent_path.read_bytes()
    return hashlib.sha1(_extract_top_level_info_bytes(raw)).hexdigest()


def _extract_top_level_info_bytes(data: bytes) -> bytes:
    if not data or data[0:1] != b"d":
        raise ValueError("Torrent payload must start with a dictionary")
    index = 1
    while index < len(data) and data[index:index + 1] != b"e":
        key, index = _parse_bytes(data, index)
        value_start = index
        _, index = _parse_any(data, index)
        if key == b"info":
            return data[value_start:index]
    raise ValueError("Torrent payload did not include a top-level info dictionary")


def _parse_any(data: bytes, index: int) -> tuple[Any, int]:
    token = data[index:index + 1]
    if token == b"i":
        return _parse_int(data, index)
    if token == b"l":
        return _parse_list(data, index)
    if token == b"d":
        return _parse_dict(data, index)
    if token.isdigit():
        return _parse_bytes(data, index)
    raise ValueError(f"Unsupported bencode token at {index}: {token!r}")


def _parse_int(data: bytes, index: int) -> tuple[int, int]:
    end = data.index(b"e", index)
    return int(data[index + 1:end]), end + 1


def _parse_bytes(data: bytes, index: int) -> tuple[bytes, int]:
    colon = data.index(b":", index)
    length = int(data[index:colon])
    start = colon + 1
    end = start + length
    return data[start:end], end


def _parse_list(data: bytes, index: int) -> tuple[list[Any], int]:
    items: list[Any] = []
    index += 1
    while data[index:index + 1] != b"e":
        item, index = _parse_any(data, index)
        items.append(item)
    return items, index + 1


def _parse_dict(data: bytes, index: int) -> tuple[dict[bytes, Any], int]:
    result: dict[bytes, Any] = {}
    index += 1
    while data[index:index + 1] != b"e":
        key, index = _parse_bytes(data, index)
        value, index = _parse_any(data, index)
        result[key] = value
    return result, index + 1
