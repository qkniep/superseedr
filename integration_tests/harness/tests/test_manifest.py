from __future__ import annotations

from pathlib import Path

from integration_tests.harness.manifest import build_expected_manifest, validate_output


def test_build_expected_manifest_skips_v1_only_for_non_v1(tmp_path: Path) -> None:
    root = tmp_path / "test_data"
    (root / "single").mkdir(parents=True)
    (root / "single" / "single_25k.bin").write_bytes(b"x")
    (root / "single" / "single_4k.bin").write_bytes(b"y")

    v1 = build_expected_manifest(root, "v1")
    v2 = build_expected_manifest(root, "v2")

    assert "single/single_25k.bin" in v1
    assert "single/single_25k.bin" not in v2


def test_validate_output_detects_missing_and_extra(tmp_path: Path) -> None:
    expected_root = tmp_path / "expected"
    out_root = tmp_path / "out"
    (expected_root / "single").mkdir(parents=True)
    (out_root / "single").mkdir(parents=True)

    (expected_root / "single" / "a.bin").write_bytes(b"abc")
    (out_root / "single" / "b.bin").write_bytes(b"zzz")

    expected = build_expected_manifest(expected_root, "v1")
    issues = validate_output(out_root, expected)

    assert "single/a.bin" in issues["missing"]
    assert "single/b.bin" in issues["extra"]
