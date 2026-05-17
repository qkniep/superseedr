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
    _validate_superseedr_transport_observations,
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


def test_load_utp_only_scenarios_disable_tcp() -> None:
    for name in ("superseedr_utp_to_libtorrent", "libtorrent_utp_to_superseedr"):
        scenario = LabScenario.from_file(
            Path(f"integration_tests/libtorrent_lab/scenarios/{name}.json")
        )

        assert scenario.superseedr_peer_transport == "utp"
        assert scenario.libtorrent_settings["enable_incoming_tcp"] is False
        assert scenario.libtorrent_settings["enable_outgoing_tcp"] is False
        assert scenario.libtorrent_settings["enable_incoming_utp"] is True
        assert scenario.libtorrent_settings["enable_outgoing_utp"] is True


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


def test_utp_only_scenario_requires_superseedr_utp_payload() -> None:
    scenario = LabScenario.from_file(
        Path("integration_tests/libtorrent_lab/scenarios/libtorrent_utp_to_superseedr.json")
    )
    seed_status = {"client": CLIENT_LIBTORRENT}
    leech_status = {
        "status": "ok",
        "tcp_peer_count": 0,
        "utp_peer_count": 1,
        "beneficial_utp_peer_count": 1,
    }

    assert _validate_superseedr_transport_observations(
        scenario,
        seed_status,
        leech_status,
    ) == {"ok": True, "issues": []}


def test_utp_only_scenario_rejects_superseedr_tcp_payload() -> None:
    scenario = LabScenario.from_file(
        Path("integration_tests/libtorrent_lab/scenarios/superseedr_utp_to_libtorrent.json")
    )
    seed_status = {
        "status": "ok",
        "tcp_peer_count": 1,
        "utp_peer_count": 0,
        "beneficial_utp_peer_count": 0,
    }
    leech_status = {"client": CLIENT_LIBTORRENT}

    report = _validate_superseedr_transport_observations(
        scenario,
        seed_status,
        leech_status,
    )

    assert report["ok"] is False
    assert report["issues"] == [
        "seed Superseedr observed 1 TCP peer(s) in uTP-only mode",
        "seed Superseedr did not observe a uTP peer",
        "seed Superseedr did not move payload over uTP",
    ]


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
