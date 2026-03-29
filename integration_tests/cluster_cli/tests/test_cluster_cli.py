from __future__ import annotations

import os
import subprocess

import pytest

from integration_tests.cluster_cli.runner import run_cluster_cli_smoke


pytestmark = [pytest.mark.cluster_cli, pytest.mark.slow]


def _docker_available() -> bool:
    result = subprocess.run(
        ["docker", "version"],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0


@pytest.mark.skipif(
    os.environ.get("RUN_CLUSTER_CLI") != "1",
    reason="set RUN_CLUSTER_CLI=1 to run the Dockerized cluster CLI smoke harness",
)
@pytest.mark.skipif(not _docker_available(), reason="docker is required for the cluster CLI lane")
def test_cluster_cli_smoke() -> None:
    summary = run_cluster_cli_smoke()
    assert summary["status"] == "ok"
