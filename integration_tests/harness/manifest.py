from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path

V1_ONLY_EXPECTED = {"single/single_25k.bin"}


@dataclass(frozen=True)
class ExpectedFile:
    rel_path: str
    size: int
    sha256: str


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def build_expected_manifest(test_data_root: Path, mode: str) -> dict[str, ExpectedFile]:
    expected: dict[str, ExpectedFile] = {}
    for path in sorted(test_data_root.rglob("*")):
        if not path.is_file() or path.name.startswith("."):
            continue
        rel = path.relative_to(test_data_root).as_posix()
        if mode != "v1" and rel in V1_ONLY_EXPECTED:
            continue
        expected[rel] = ExpectedFile(rel_path=rel, size=path.stat().st_size, sha256=_sha256_file(path))
    return expected


def validate_output(output_root: Path, expected: dict[str, ExpectedFile]) -> dict[str, list[str]]:
    actual_files: dict[str, Path] = {}
    for path in sorted(output_root.rglob("*")):
        if path.is_file() and not path.name.startswith("."):
            rel = path.relative_to(output_root).as_posix()
            actual_files[rel] = path

    missing: list[str] = []
    extra: list[str] = []
    mismatched: list[str] = []

    for rel, spec in expected.items():
        act = actual_files.get(rel)
        if act is None:
            missing.append(rel)
            continue
        size = act.stat().st_size
        if size != spec.size:
            mismatched.append(f"{rel} size expected={spec.size} actual={size}")
            continue
        digest = _sha256_file(act)
        if digest != spec.sha256:
            mismatched.append(f"{rel} sha256 expected={spec.sha256} actual={digest}")

    for rel in sorted(set(actual_files) - set(expected)):
        extra.append(rel)

    return {
        "missing": missing,
        "extra": extra,
        "mismatched": mismatched,
    }
