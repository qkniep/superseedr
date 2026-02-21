from __future__ import annotations

import os
import socket
import time

import pytest

from integration_tests.harness.clients.transmission import TransmissionAdapter
from integration_tests.harness.config import resolve_paths
from integration_tests.harness.docker_ctl import DockerCompose


def _reserve_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


@pytest.mark.interop
@pytest.mark.interop_transmission
@pytest.mark.slow
def test_transmission_container_and_auth() -> None:
    if os.environ.get("RUN_INTEROP") != "1":
        pytest.skip("Set RUN_INTEROP=1 to execute docker interop tests")

    paths = resolve_paths()
    run_id = f"run_{time.strftime('%Y%m%d_%H%M%S')}_transmission_auth"
    run_root = paths.artifacts_root / "runs" / run_id / "transmission_auth"
    config_root = run_root / "transmission_config"
    downloads_root = run_root / "transmission_downloads"
    seed_data_root = run_root / "seed_data_unused"
    leech_data_root = run_root / "leech_data_unused"
    seed_config_root = run_root / "seed_config_unused"
    leech_config_root = run_root / "leech_config_unused"
    seed_share_root = run_root / "seed_share_unused"
    leech_share_root = run_root / "leech_share_unused"
    logs_root = run_root / "logs"
    config_root.mkdir(parents=True, exist_ok=True)
    downloads_root.mkdir(parents=True, exist_ok=True)
    seed_data_root.mkdir(parents=True, exist_ok=True)
    leech_data_root.mkdir(parents=True, exist_ok=True)
    seed_config_root.mkdir(parents=True, exist_ok=True)
    leech_config_root.mkdir(parents=True, exist_ok=True)
    seed_share_root.mkdir(parents=True, exist_ok=True)
    leech_share_root.mkdir(parents=True, exist_ok=True)
    logs_root.mkdir(parents=True, exist_ok=True)

    tracker_port = _reserve_local_port()
    transmission_rpc_port = _reserve_local_port()
    transmission_user = "interop"
    transmission_pass = "interop"
    project_name = f"interop_transmission_auth_{int(time.time())}"
    compose = DockerCompose(
        paths.compose_file,
        project_name,
        {
            "INTEROP_PROJECT_NAME": project_name,
            "INTEROP_TRACKER_PORT": str(tracker_port),
            "INTEROP_TRACKER_SCRIPT_PATH": str(paths.tracker_script.resolve()),
            "INTEROP_FIXTURES_PATH": str(paths.fixtures_root.resolve()),
            "INTEROP_TRANSMISSION_CONFIG_PATH": str(config_root.resolve()),
            "INTEROP_TRANSMISSION_DOWNLOADS_PATH": str(downloads_root.resolve()),
            "INTEROP_TRANSMISSION_RPC_PORT": str(transmission_rpc_port),
            "INTEROP_TRANSMISSION_USER": transmission_user,
            "INTEROP_TRANSMISSION_PASS": transmission_pass,
            "INTEROP_SEED_DATA_PATH": str(seed_data_root.resolve()),
            "INTEROP_LEECH_DATA_PATH": str(leech_data_root.resolve()),
            "INTEROP_SEED_CONFIG_PATH": str(seed_config_root.resolve()),
            "INTEROP_LEECH_CONFIG_PATH": str(leech_config_root.resolve()),
            "INTEROP_SEED_SHARE_PATH": str(seed_share_root.resolve()),
            "INTEROP_LEECH_SHARE_PATH": str(leech_share_root.resolve()),
        },
    )
    adapter = TransmissionAdapter(
        compose=compose,
        base_url=f"http://127.0.0.1:{transmission_rpc_port}/transmission/rpc",
        username=transmission_user,
        password=transmission_pass,
        auth_timeout_secs=120,
    )

    try:
        adapter.start()
        status = adapter.read_status()
        assert status["status"] == "ok"
        assert status["torrent_count"] >= 0
        adapter.collect_logs(logs_root)
        assert (logs_root / "transmission.log").exists()
    finally:
        compose.down()
