from __future__ import annotations

import os
import subprocess

import pytest


@pytest.mark.interop
@pytest.mark.interop_transmission
@pytest.mark.slow
@pytest.mark.parametrize("mode", ["v1"])
def test_superseedr_to_transmission_interop_mode(mode: str) -> None:
    if os.environ.get("RUN_INTEROP") != "1":
        pytest.skip("Set RUN_INTEROP=1 to execute docker interop tests")

    cmd = [
        "python3",
        "-m",
        "integration_tests.harness.run",
        "--scenario",
        "superseedr_to_transmission",
        "--mode",
        mode,
        "--timeout-secs",
        os.environ.get("INTEROP_TIMEOUT_SECS", "300"),
    ]
    subprocess.run(cmd, check=True)
