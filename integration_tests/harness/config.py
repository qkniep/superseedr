from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INTEGRATION_ROOT = ROOT / "integration_tests"
ARTIFACTS_ROOT = INTEGRATION_ROOT / "artifacts"


@dataclass(frozen=True)
class HarnessPaths:
    root: Path
    integration_root: Path
    fixtures_root: Path
    torrents_root: Path
    test_data_root: Path
    artifacts_root: Path
    compose_file: Path
    tracker_script: Path


@dataclass(frozen=True)
class HarnessDefaults:
    announce_url: str = "http://tracker:6969/announce"
    status_poll_active_secs: float = 1.0
    status_poll_idle_secs: float = 5.0
    stable_window_secs: float = 10.0


def resolve_paths() -> HarnessPaths:
    return HarnessPaths(
        root=ROOT,
        integration_root=INTEGRATION_ROOT,
        fixtures_root=INTEGRATION_ROOT,
        torrents_root=INTEGRATION_ROOT / "torrents",
        test_data_root=INTEGRATION_ROOT / "test_data",
        artifacts_root=ARTIFACTS_ROOT,
        compose_file=INTEGRATION_ROOT / "docker" / "docker-compose.interop.yml",
        tracker_script=INTEGRATION_ROOT / "docker" / "tracker.py",
    )


def env_bool(name: str, default: bool = False) -> bool:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}
