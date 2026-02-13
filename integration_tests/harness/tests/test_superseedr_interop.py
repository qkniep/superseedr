from __future__ import annotations

import os
import subprocess

import pytest


@pytest.mark.interop
@pytest.mark.interop_superseedr
@pytest.mark.slow
@pytest.mark.parametrize("mode", ["v1", "v2", "hybrid"])
def test_superseedr_interop_mode(mode: str) -> None:
    if os.environ.get("RUN_INTEROP") != "1":
        pytest.skip("Set RUN_INTEROP=1 to execute docker interop tests")

    cmd = [
        "python3",
        "-m",
        "integration_tests.harness.run",
        "--scenario",
        "superseedr_to_superseedr",
        "--mode",
        mode,
        "--timeout-secs",
        os.environ.get("INTEROP_TIMEOUT_SECS", "300"),
    ]
    subprocess.run(cmd, check=True)
