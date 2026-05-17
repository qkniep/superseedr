from __future__ import annotations

from pathlib import Path

from integration_tests.libtorrent_lab.run import (
    CLIENT_LIBTORRENT,
    CLIENT_SUPERSEEDR,
    LabScenario,
    _client_payload_path,
    _project_name,
    _superseedr_seed_is_ready,
    _superseedr_download_path,
    _validate_download,
)


def test_project_name_is_compose_safe() -> None:
    assert _project_name("libtorrent_lab_basic_ul_dl_2026-05-17") == "ltlablibtorrentlabbasiculdl20260517"


def test_load_basic_scenario() -> None:
    scenario = LabScenario.from_file(
        Path("integration_tests/libtorrent_lab/scenarios/basic_ul_dl.json")
    )
    assert scenario.name == "basic_ul_dl"
    assert scenario.seed_client == CLIENT_LIBTORRENT
    assert scenario.leech_client == CLIENT_LIBTORRENT
    assert scenario.mode == "v1"
    assert scenario.seed_listen_port != scenario.leech_listen_port
    assert scenario.libtorrent_settings["enable_dht"] is False


def test_load_superseedr_to_libtorrent_scenario() -> None:
    scenario = LabScenario.from_file(
        Path("integration_tests/libtorrent_lab/scenarios/superseedr_to_libtorrent.json")
    )
    assert scenario.seed_client == CLIENT_SUPERSEEDR
    assert scenario.leech_client == CLIENT_LIBTORRENT
    assert scenario.superseedr_peer_transport == "tcp"
    assert scenario.libtorrent_settings["enable_incoming_utp"] is False


def test_superseedr_payload_path_preserves_fixture_bucket(tmp_path: Path) -> None:
    scenario = LabScenario.from_file(
        Path("integration_tests/libtorrent_lab/scenarios/libtorrent_to_superseedr.json")
    )
    assert _client_payload_path(CLIENT_SUPERSEEDR, tmp_path, scenario) == (
        tmp_path / "v1" / "single" / "single_16k.bin"
    )
    assert _superseedr_download_path("leech", scenario) == (
        "/superseedr-data/leech/v1/single"
    )


def test_superseedr_lab_uses_fast_lab_image() -> None:
    compose = Path(
        "integration_tests/libtorrent_lab/docker/docker-compose.libtorrent-lab.yml"
    ).read_text(encoding="utf-8")

    assert "integration_tests/libtorrent_lab/docker/Dockerfile.superseedr" in compose
    assert "dockerfile: Dockerfile" not in compose


def test_superseedr_seed_ready_accepts_completed_data() -> None:
    status = {
        "status": "ok",
        "activity_messages": ["Finished"],
        "complete_torrents": 1,
        "data_available_torrents": 1,
    }

    assert _superseedr_seed_is_ready(status)


def test_validate_download_reports_hash_match(tmp_path: Path) -> None:
    expected = tmp_path / "expected.bin"
    actual = tmp_path / "actual.bin"
    expected.write_bytes(b"deterministic payload")
    actual.write_bytes(b"deterministic payload")

    report = _validate_download(actual, expected)

    assert report["ok"] is True
    assert report["issues"] == []
    assert report["expected_sha256"] == report["actual_sha256"]


def test_validate_download_reports_missing_file(tmp_path: Path) -> None:
    expected = tmp_path / "expected.bin"
    missing = tmp_path / "missing.bin"
    expected.write_bytes(b"deterministic payload")

    report = _validate_download(missing, expected)

    assert report["ok"] is False
    assert report["issues"] == ["missing missing.bin"]
