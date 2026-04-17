#!/usr/bin/env python3
"""Generate deterministic small binary fixtures for integration tests."""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

FIXTURE_SPECS: list[tuple[str, int, str]] = [
    (
        "integration_tests/test_data/single/single_4k.bin",
        4 * 1024,
        "integration_tests/test_data/single/single_4k.bin",
    ),
    (
        "integration_tests/test_data/single/single_8k.bin",
        8 * 1024,
        "integration_tests/test_data/single/single_8k.bin",
    ),
    (
        "integration_tests/test_data/single/single_16k.bin",
        16 * 1024,
        "integration_tests/test_data/single/single_16k.bin",
    ),
    (
        "integration_tests/test_data/single/single_25k.bin",
        25 * 1024,
        "integration_tests/test_data/single/single_25k.bin",
    ),
    (
        "integration_tests/test_data/multi_file/multi_a_4k.bin",
        4 * 1024,
        "integration_tests/test_data/multi_file/multi_a_4k.bin",
    ),
    (
        "integration_tests/test_data/multi_file/multi_b_8k.bin",
        8 * 1024,
        "integration_tests/test_data/multi_file/multi_b_8k.bin",
    ),
    (
        "integration_tests/test_data/multi_file/multi_c_16k.bin",
        16 * 1024,
        "integration_tests/test_data/multi_file/multi_c_16k.bin",
    ),
    (
        "integration_tests/test_data/nested/nested_16k.bin",
        16 * 1024,
        "integration_tests/test_data/nested/subdir/nested_16k.bin",
    ),
    (
        "integration_tests/test_data/nested/subdir1/nested_8k.bin",
        8 * 1024,
        "integration_tests/test_data/nested/subdir/nested_8k.bin",
    ),
    (
        "integration_tests/test_data/nested/subdir1/subdir2a/nested_4k.bin",
        4 * 1024,
        "integration_tests/test_data/nested/subdir/nested_4k.bin",
    ),
    (
        "integration_tests/test_data/nested/subdir1/subdir2b/nested_4k.bin",
        4 * 1024,
        "integration_tests/test_data/nested/subdir/nested_4k.bin",
    ),
]


def expected_bytes(seed_key: str, size: int) -> bytes:
    seed = f"{seed_key}|{size}".encode("utf-8")
    out = bytearray()
    counter = 0
    while len(out) < size:
        digest = hashlib.sha256(seed + counter.to_bytes(8, "big")).digest()
        out.extend(digest)
        counter += 1
    return bytes(out[:size])


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def check_specs() -> tuple[bool, int]:
    ok = True
    total_bytes = 0
    for rel_path, size, seed_key in FIXTURE_SPECS:
        path = ROOT / rel_path
        expected = expected_bytes(seed_key, size)
        total_bytes += size
        if not path.exists():
            print(f"MISSING  {rel_path}")
            ok = False
            continue
        actual = path.read_bytes()
        if len(actual) != size:
            print(f"SIZE_MISMATCH {rel_path} expected={size} actual={len(actual)}")
            ok = False
            continue
        if actual != expected:
            print(
                f"CONTENT_MISMATCH {rel_path} expected_sha256={sha256_hex(expected)} "
                f"actual_sha256={sha256_hex(actual)}"
            )
            ok = False
            continue
        print(f"OK      {rel_path} bytes={size} sha256={sha256_hex(actual)}")
    print(f"TOTAL_BYTES {total_bytes}")
    return ok, total_bytes


def generate_specs() -> int:
    total_bytes = 0
    for rel_path, size, seed_key in FIXTURE_SPECS:
        path = ROOT / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        data = expected_bytes(seed_key, size)
        path.write_bytes(data)
        total_bytes += size
        print(f"WRITE   {rel_path} bytes={size} sha256={sha256_hex(data)}")
    print(f"TOTAL_BYTES {total_bytes}")
    return total_bytes


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate and verify deterministic integration .bin fixtures."
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Verify fixture files exist and match deterministic content.",
    )
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="Alias of --verify for CI usage. Exits non-zero on mismatch.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.verify or args.check_only:
        ok, _ = check_specs()
        return 0 if ok else 1

    generate_specs()
    return 0


if __name__ == "__main__":
    sys.exit(main())
