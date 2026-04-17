#!/usr/bin/env python3
"""Clear integration test output files while preserving output directory layout."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT_ROOT = ROOT / "integration_tests" / "test_output"
DEFAULT_MODES = ("v1", "v2", "hybrid")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Remove files from integration_tests/test_output. "
            "By default targets v1, v2, hybrid."
        )
    )
    parser.add_argument(
        "--mode",
        action="append",
        choices=DEFAULT_MODES,
        help="Mode to clear. Repeatable. Default clears all modes.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would be removed without deleting anything.",
    )
    return parser.parse_args()


def clear_mode(mode: str, dry_run: bool) -> tuple[int, int]:
    mode_root = OUTPUT_ROOT / mode
    removed_files = 0
    removed_dirs = 0

    if not mode_root.exists():
        if not dry_run:
            mode_root.mkdir(parents=True, exist_ok=True)
        print(f"SKIP    {mode_root} (missing)")
        return removed_files, removed_dirs

    files = [p for p in mode_root.rglob("*") if p.is_file()]
    dirs = sorted([p for p in mode_root.rglob("*") if p.is_dir()], reverse=True)

    for file_path in files:
        print(f"{'WOULD_REMOVE' if dry_run else 'REMOVE'} {file_path}")
        if not dry_run:
            file_path.unlink()
        removed_files += 1

    for dir_path in dirs:
        # Remove any empty nested dirs, keep the mode root itself.
        if dir_path == mode_root:
            continue
        if dry_run:
            # Report empty dirs that would be pruned after file deletion.
            print(f"WOULD_PRUNE {dir_path}")
            removed_dirs += 1
            continue
        try:
            dir_path.rmdir()
            print(f"PRUNE   {dir_path}")
            removed_dirs += 1
        except OSError:
            # Not empty; ignore.
            pass

    if not dry_run:
        mode_root.mkdir(parents=True, exist_ok=True)
    return removed_files, removed_dirs


def main() -> int:
    args = parse_args()
    modes = args.mode if args.mode else list(DEFAULT_MODES)

    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    for mode in DEFAULT_MODES:
        (OUTPUT_ROOT / mode).mkdir(parents=True, exist_ok=True)

    total_files = 0
    total_dirs = 0
    for mode in modes:
        files, dirs = clear_mode(mode, args.dry_run)
        total_files += files
        total_dirs += dirs

    print(f"SUMMARY files={total_files} dirs={total_dirs} dry_run={args.dry_run}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
