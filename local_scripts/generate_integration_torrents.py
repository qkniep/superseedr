#!/usr/bin/env python3
"""Generate normalized integration torrent fixtures.

Behavior:
- v1 torrents are regenerated from integration fixture data (source of truth).
- v2/hybrid torrents are copied from committed fixtures and announce metadata normalized.

This keeps phase-1 harness reliable for v1 while preserving existing v2/hybrid
fixtures until a v2-capable generator is added.
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import sys
from pathlib import Path

try:
    from torf import Torrent
except ImportError as exc:  # pragma: no cover
    raise SystemExit(
        "torf is required. Install with: python3 -m pip install torf"
    ) from exc

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TORRENTS_ROOT = ROOT / "integration_tests" / "torrents"
DEFAULT_TEST_DATA_ROOT = ROOT / "integration_tests" / "test_data"
DEFAULT_OUTPUT_ROOT = ROOT / "integration_tests" / "artifacts" / "generated_torrents"


class BencodeError(ValueError):
    pass


def bdecode(data: bytes) -> object:
    value, offset = _decode_at(data, 0)
    if offset != len(data):
        raise BencodeError("Trailing data after bencode payload")
    return value


def _decode_at(data: bytes, i: int) -> tuple[object, int]:
    if i >= len(data):
        raise BencodeError("Unexpected end of bencode data")

    c = data[i : i + 1]
    if c == b"i":
        end = data.find(b"e", i)
        if end < 0:
            raise BencodeError("Invalid int encoding")
        return int(data[i + 1 : end]), end + 1
    if c == b"l":
        i += 1
        out = []
        while data[i : i + 1] != b"e":
            item, i = _decode_at(data, i)
            out.append(item)
        return out, i + 1
    if c == b"d":
        i += 1
        out: dict[bytes, object] = {}
        while data[i : i + 1] != b"e":
            key, i = _decode_at(data, i)
            if not isinstance(key, bytes):
                raise BencodeError("Dictionary key must be bytes")
            val, i = _decode_at(data, i)
            out[key] = val
        return out, i + 1
    if c.isdigit():
        colon = data.find(b":", i)
        if colon < 0:
            raise BencodeError("Invalid bytes encoding")
        size = int(data[i:colon])
        start = colon + 1
        end = start + size
        return data[start:end], end
    raise BencodeError(f"Unsupported bencode marker at offset {i}: {c!r}")


def bencode(value: object) -> bytes:
    if isinstance(value, int):
        return f"i{value}e".encode("ascii")
    if isinstance(value, bytes):
        return f"{len(value)}:".encode("ascii") + value
    if isinstance(value, str):
        payload = value.encode("utf-8")
        return f"{len(payload)}:".encode("ascii") + payload
    if isinstance(value, list):
        return b"l" + b"".join(bencode(v) for v in value) + b"e"
    if isinstance(value, dict):
        out = bytearray(b"d")
        for key in sorted(value.keys()):
            key_bytes = key if isinstance(key, bytes) else str(key).encode("utf-8")
            out.extend(bencode(key_bytes))
            out.extend(bencode(value[key]))
        out.extend(b"e")
        return bytes(out)
    raise TypeError(f"Unsupported bencode type: {type(value)!r}")


def normalize_announce(payload: dict[bytes, object], announce_url: str) -> None:
    announce_bytes = announce_url.encode("utf-8")
    payload[b"announce"] = announce_bytes
    payload[b"announce-list"] = [[announce_bytes]]


def rewrite_announce(src_path: Path, dest_path: Path, announce_url: str) -> None:
    payload = bdecode(src_path.read_bytes())
    if not isinstance(payload, dict):
        raise BencodeError(f"Root payload is not dict for {src_path}")
    normalize_announce(payload, announce_url)
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    dest_path.write_bytes(bencode(payload))


def write_v1_single_file_torrent_manual(
    source: Path,
    dest: Path,
    announce_url: str,
    piece_size: int,
) -> None:
    data = source.read_bytes()
    pieces = bytearray()
    for i in range(0, len(data), piece_size):
        piece = data[i : i + piece_size]
        pieces.extend(hashlib.sha1(piece).digest())

    info = {
        b"length": len(data),
        b"name": source.name.encode("utf-8"),
        b"piece length": piece_size,
        b"pieces": bytes(pieces),
    }
    payload = {
        b"announce": announce_url.encode("utf-8"),
        b"announce-list": [[announce_url.encode("utf-8")]],
        b"created by": b"superseedr integration harness",
        b"creation date": 0,
        b"info": info,
    }
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes(bencode(payload))


def generate_v1_torrents(test_data_root: Path, output_root: Path, announce_url: str) -> None:
    specs: list[tuple[Path, Path, int]] = [
        (test_data_root / "single" / "single_4k.bin", output_root / "v1" / "single_4k.bin.torrent", 16384),
        (test_data_root / "single" / "single_8k.bin", output_root / "v1" / "single_8k.bin.torrent", 16384),
        (test_data_root / "single" / "single_16k.bin", output_root / "v1" / "single_16k.bin.torrent", 16384),
        (test_data_root / "single" / "single_25k.bin", output_root / "v1" / "single_25k.bin.torrent", 20000),
        (test_data_root / "multi_file", output_root / "v1" / "multi_file.torrent", 16384),
        (test_data_root / "nested", output_root / "v1" / "nested.torrent", 16384),
    ]

    for source, dest, piece_size in specs:
        if not source.exists():
            raise FileNotFoundError(f"Missing fixture source: {source}")
        if source.name == "single_25k.bin":
            write_v1_single_file_torrent_manual(
                source=source,
                dest=dest,
                announce_url=announce_url,
                piece_size=piece_size,
            )
            print(f"WRITE {dest}")
            continue
        torrent = Torrent(
            path=source,
            trackers=[announce_url],
            piece_size=piece_size,
            creation_date=0,
            created_by="superseedr integration harness",
            randomize_infohash=False,
        )
        torrent.generate()
        dest.parent.mkdir(parents=True, exist_ok=True)
        torrent.write(dest, overwrite=True)
        print(f"WRITE {dest}")


def copy_and_normalize_existing_modes(
    source_torrents_root: Path,
    output_root: Path,
    announce_url: str,
) -> None:
    for mode in ("v2", "hybrid"):
        for src in sorted((source_torrents_root / mode).glob("*.torrent")):
            dest = output_root / mode / src.name
            rewrite_announce(src, dest, announce_url)
            print(f"WRITE {dest}")


def verify_announce(output_root: Path, announce_url: str) -> tuple[bool, int]:
    target = announce_url.encode("utf-8")
    failures = 0
    for path in sorted(output_root.rglob("*.torrent")):
        payload = bdecode(path.read_bytes())
        if not isinstance(payload, dict):
            print(f"FAIL {path} root payload not dictionary")
            failures += 1
            continue
        announce = payload.get(b"announce")
        if announce != target:
            print(f"MISMATCH {path} announce={announce!r}")
            failures += 1
        else:
            print(f"OK {path}")
    return failures == 0, failures


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Generate normalized integration torrents")
    p.add_argument("--announce-url", default="http://tracker:6969/announce")
    p.add_argument("--torrents-root", default=str(DEFAULT_TORRENTS_ROOT))
    p.add_argument("--test-data-root", default=str(DEFAULT_TEST_DATA_ROOT))
    p.add_argument("--output-root", default=str(DEFAULT_OUTPUT_ROOT))
    p.add_argument("--verify", action="store_true", help="Verify output announce metadata")
    return p.parse_args()


def main() -> int:
    args = parse_args()
    output_root = Path(args.output_root).resolve()

    if args.verify:
        ok, failures = verify_announce(output_root, args.announce_url)
        if not ok:
            print(f"FAILURES {failures}")
            return 1
        return 0

    torrents_root = Path(args.torrents_root).resolve()
    test_data_root = Path(args.test_data_root).resolve()

    if output_root.exists():
        shutil.rmtree(output_root)
    output_root.mkdir(parents=True, exist_ok=True)

    generate_v1_torrents(test_data_root, output_root, args.announce_url)
    copy_and_normalize_existing_modes(torrents_root, output_root, args.announce_url)
    return 0


if __name__ == "__main__":
    sys.exit(main())
