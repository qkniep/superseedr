#!/usr/bin/env python3
"""Cross-validate integration test outputs against canonical test_data files."""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TEST_DATA_ROOT = ROOT / "integration_tests" / "test_data"
TEST_OUTPUT_ROOT = ROOT / "integration_tests" / "test_output"
ALL_MODES = ("v1", "v2", "hybrid")
V1_ONLY_EXPECTED = {
    "single/single_25k.bin",
}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def collect_files(root: Path) -> dict[str, Path]:
    files: dict[str, Path] = {}
    if not root.exists():
        return files
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if path.name.startswith("."):
            continue
        rel = path.relative_to(root).as_posix()
        files[rel] = path
    return files


def validate_mode(
    mode: str, expected: dict[str, Path], allow_missing: bool, allow_extra: bool
) -> tuple[bool, int]:
    output_root = TEST_OUTPUT_ROOT / mode
    actual = collect_files(output_root)
    ok = True
    issues = 0

    expected_for_mode = dict(expected)
    if mode != "v1":
        for rel in V1_ONLY_EXPECTED:
            expected_for_mode.pop(rel, None)

    print(f"\n=== Mode: {mode} ===")
    print(f"expected_files={len(expected_for_mode)} actual_files={len(actual)}")

    for rel, exp_path in expected_for_mode.items():
        act_path = actual.get(rel)
        if act_path is None:
            msg = f"MISSING  {mode}/{rel}"
            print(msg)
            if not allow_missing:
                ok = False
                issues += 1
            continue

        exp_size = exp_path.stat().st_size
        act_size = act_path.stat().st_size
        if exp_size != act_size:
            print(f"SIZE_MISMATCH {mode}/{rel} expected={exp_size} actual={act_size}")
            ok = False
            issues += 1
            continue

        exp_hash = sha256_file(exp_path)
        act_hash = sha256_file(act_path)
        if exp_hash != act_hash:
            print(
                f"HASH_MISMATCH {mode}/{rel} expected_sha256={exp_hash} actual_sha256={act_hash}"
            )
            ok = False
            issues += 1
            continue

        print(f"OK       {mode}/{rel} bytes={exp_size} sha256={act_hash}")

    for rel in sorted(set(actual) - set(expected_for_mode)):
        msg = f"EXTRA    {mode}/{rel}"
        print(msg)
        if not allow_extra:
            ok = False
            issues += 1

    if ok:
        print(f"MODE_RESULT {mode}: PASS")
    else:
        print(f"MODE_RESULT {mode}: FAIL issues={issues}")
    return ok, issues


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description=(
            "Validate integration_tests/test_output/<mode> against "
            "integration_tests/test_data using size + SHA-256."
        )
    )
    p.add_argument(
        "--mode",
        action="append",
        choices=ALL_MODES,
        help="Mode(s) to validate. Repeatable. Default validates all modes.",
    )
    p.add_argument(
        "--allow-missing",
        action="store_true",
        help="Do not fail for missing output files.",
    )
    p.add_argument(
        "--allow-extra",
        action="store_true",
        help="Do not fail for extra output files.",
    )
    return p.parse_args()


def main() -> int:
    args = parse_args()
    modes = args.mode if args.mode else list(ALL_MODES)

    expected = collect_files(TEST_DATA_ROOT)
    if not expected:
        print(f"No canonical files found under {TEST_DATA_ROOT}")
        return 1

    print(f"Canonical root: {TEST_DATA_ROOT}")
    print(f"Output root:    {TEST_OUTPUT_ROOT}")
    print(f"Modes:          {', '.join(modes)}")

    all_ok = True
    total_issues = 0
    for mode in modes:
        ok, issues = validate_mode(
            mode=mode,
            expected=expected,
            allow_missing=args.allow_missing,
            allow_extra=args.allow_extra,
        )
        all_ok = all_ok and ok
        total_issues += issues

    if all_ok:
        print("\nOVERALL_RESULT PASS")
        return 0

    print(f"\nOVERALL_RESULT FAIL total_issues={total_issues}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
