from __future__ import annotations

import json
import socket
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from urllib import error as url_error
from urllib import request as url_request

from integration_tests.harness.clients.superseedr import SuperseedrAdapter
from integration_tests.harness.clients.transmission import TransmissionAdapter
from integration_tests.harness.config import HarnessDefaults, HarnessPaths
from integration_tests.harness.docker_ctl import DockerCompose
from integration_tests.harness.manifest import build_expected_manifest, validate_output

SUPPORTED_MODES = ("v1",)


@dataclass(frozen=True)
class ScenarioResult:
    mode: str
    ok: bool
    duration_secs: float
    missing: list[str]
    extra: list[str]
    mismatched: list[str]


def _bucket_for_torrent(name: str) -> str:
    if name.startswith("single_"):
        return "single"
    if name == "multi_file.torrent":
        return "multi_file"
    if name == "nested.torrent":
        return "nested"
    raise ValueError(f"Unsupported torrent fixture: {name}")


def _transmission_savepath_for_torrent(mode: str, name: str) -> str:
    if name.startswith("single_"):
        return f"/downloads/{mode}/single"
    if name in {"multi_file.torrent", "nested.torrent"}:
        return f"/downloads/{mode}"
    raise ValueError(f"Unsupported torrent fixture: {name}")


def _write_seed_settings(mode: str, config_path: Path, torrent_files: list[str]) -> None:
    role_root = f"/superseedr-data/seed/{mode}"
    lines = [
        'client_id = "-SS1000-SEEDCLIENT01"',
        "client_port = 16881",
        "lifetime_downloaded = 0",
        "lifetime_uploaded = 0",
        "private_client = false",
        'torrent_sort_column = "Up"',
        'torrent_sort_direction = "Ascending"',
        'peer_sort_column = "UL"',
        'peer_sort_direction = "Ascending"',
        'ui_theme = "catppuccin_mocha"',
        f'default_download_folder = "{role_root}"',
        "max_connected_peers = 500",
        "output_status_interval = 2",
        "bootstrap_nodes = []",
        "global_download_limit_bps = 0",
        "global_upload_limit_bps = 0",
        "max_concurrent_validations = 16",
        "connection_attempt_permits = 16",
        "upload_slots = 8",
        "peer_upload_in_flight_limit = 4",
        "tracker_fallback_interval_secs = 10",
        "client_leeching_fallback_interval_secs = 10",
        "",
    ]

    for name in torrent_files:
        bucket = _bucket_for_torrent(name)
        torrent_name = name.replace(".torrent", "")
        lines.extend(
            [
                "[[torrents]]",
                f'torrent_or_magnet = "/fixtures/torrents/{mode}/{name}"',
                f'name = "{torrent_name}"',
                "validation_status = false",
                f'download_path = "{role_root}/{bucket}"',
                'container_name = ""',
                'torrent_control_state = "Running"',
                "",
                "[torrents.file_priorities]",
                '0 = "Normal"',
                "",
            ]
        )

    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text("\n".join(lines), encoding="utf-8")


def _prepare_seed_data(seed_mode_root: Path, canonical_root: Path) -> None:
    seed_mode_root.mkdir(parents=True, exist_ok=True)
    for bucket in ("single", "multi_file", "nested"):
        src = canonical_root / bucket
        dest = seed_mode_root / bucket
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(src, dest)


def _ensure_clean_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def _write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")


def _wait_for_tracker(port: int, timeout_secs: int = 20) -> None:
    deadline = time.monotonic() + timeout_secs
    url = f"http://127.0.0.1:{port}/announce"
    while time.monotonic() < deadline:
        try:
            with url_request.urlopen(url, timeout=1) as resp:
                if resp.status in (200, 400):
                    return
        except url_error.HTTPError as exc:
            if exc.code == 400:
                return
        except Exception:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"Tracker did not become ready within {timeout_secs}s on {url}")


def _reserve_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def run_mode(
    mode: str,
    timeout_secs: int,
    run_root: Path,
    harness_paths: HarnessPaths,
    defaults: HarnessDefaults,
    torrents_root: Path,
) -> ScenarioResult:
    if mode != "v1":
        raise RuntimeError(
            "Transmission interop currently supports only mode=v1 "
            "(v2/hybrid compatibility pending)."
        )

    start = time.monotonic()

    mode_run_root = run_root / mode
    seed_data_root = mode_run_root / "seed_data"
    seed_config_root = mode_run_root / "seed_config"
    seed_share_root = mode_run_root / "seed_share"
    transmission_config_root = mode_run_root / "transmission_config"
    transmission_downloads_root = mode_run_root / "transmission_downloads"
    leech_data_root = mode_run_root / "leech_data_unused"
    leech_config_root = mode_run_root / "leech_config_unused"
    leech_share_root = mode_run_root / "leech_share_unused"
    logs_root = mode_run_root / "logs"
    raw_status_root = mode_run_root / "raw_client_status"
    staged_fixtures_root = mode_run_root / "fixtures"

    _ensure_clean_dir(mode_run_root)
    seed_data_root.mkdir(parents=True, exist_ok=True)
    seed_config_root.mkdir(parents=True, exist_ok=True)
    seed_share_root.mkdir(parents=True, exist_ok=True)
    transmission_config_root.mkdir(parents=True, exist_ok=True)
    transmission_downloads_root.mkdir(parents=True, exist_ok=True)
    leech_data_root.mkdir(parents=True, exist_ok=True)
    leech_config_root.mkdir(parents=True, exist_ok=True)
    leech_share_root.mkdir(parents=True, exist_ok=True)
    logs_root.mkdir(parents=True, exist_ok=True)
    (staged_fixtures_root / "torrents").mkdir(parents=True, exist_ok=True)

    torrents_mode_root = torrents_root / mode
    torrent_files = sorted(p.name for p in torrents_mode_root.glob("*.torrent"))
    if not torrent_files:
        raise RuntimeError(f"No torrent fixtures found for mode={mode} under {torrents_mode_root}")

    _prepare_seed_data(seed_data_root / mode, harness_paths.test_data_root)
    shutil.copytree(torrents_root, staged_fixtures_root / "torrents", dirs_exist_ok=True)
    _write_seed_settings(mode, seed_config_root / "settings.toml", torrent_files)

    project_name = f"interop_transmission_{mode}_{int(time.time())}"
    tracker_port = _reserve_local_port()
    transmission_rpc_port = _reserve_local_port()
    transmission_user = "interop"
    transmission_pass = "interop"
    compose_env = {
        "INTEROP_PROJECT_NAME": project_name,
        "INTEROP_TRACKER_PORT": str(tracker_port),
        "INTEROP_TRACKER_SCRIPT_PATH": str(harness_paths.tracker_script.resolve()),
        "INTEROP_FIXTURES_PATH": str(staged_fixtures_root.resolve()),
        "INTEROP_SEED_DATA_PATH": str(seed_data_root.resolve()),
        "INTEROP_SEED_CONFIG_PATH": str(seed_config_root.resolve()),
        "INTEROP_SEED_SHARE_PATH": str(seed_share_root.resolve()),
        "INTEROP_LEECH_DATA_PATH": str(leech_data_root.resolve()),
        "INTEROP_LEECH_CONFIG_PATH": str(leech_config_root.resolve()),
        "INTEROP_LEECH_SHARE_PATH": str(leech_share_root.resolve()),
        "INTEROP_TRANSMISSION_CONFIG_PATH": str(transmission_config_root.resolve()),
        "INTEROP_TRANSMISSION_DOWNLOADS_PATH": str(transmission_downloads_root.resolve()),
        "INTEROP_TRANSMISSION_RPC_PORT": str(transmission_rpc_port),
        "INTEROP_TRANSMISSION_USER": transmission_user,
        "INTEROP_TRANSMISSION_PASS": transmission_pass,
    }

    compose = DockerCompose(harness_paths.compose_file, project_name, compose_env)
    seed = SuperseedrAdapter(compose, "superseedr_seed", seed_data_root / mode, seed_share_root)
    transmission = TransmissionAdapter(
        compose=compose,
        service_name="transmission",
        base_url=f"http://127.0.0.1:{transmission_rpc_port}/transmission/rpc",
        username=transmission_user,
        password=transmission_pass,
        auth_timeout_secs=120,
    )
    transmission_output_root = transmission_downloads_root / mode
    expected = build_expected_manifest(harness_paths.test_data_root, mode)

    snapshots: list[dict] = []
    last_signature = ""
    last_change = time.monotonic()

    try:
        compose.run(["build", "superseedr_seed"])
        compose.up(["tracker"], no_build=True)
        _wait_for_tracker(tracker_port)
        compose.up(["superseedr_seed"], no_build=True)
        transmission.start()

        for torrent_name in torrent_files:
            torrent_path = staged_fixtures_root / "torrents" / mode / torrent_name
            transmission.add_torrent(
                str(torrent_path),
                _transmission_savepath_for_torrent(mode, torrent_name),
            )

        deadline = time.monotonic() + timeout_secs
        while time.monotonic() < deadline:
            issues = validate_output(transmission_output_root, expected)
            seed_state = seed.read_status()
            transmission_state = transmission.read_status()

            snapshot = {
                "mode": mode,
                "timestamp": int(time.time()),
                "missing_count": len(issues["missing"]),
                "mismatched_count": len(issues["mismatched"]),
                "extra_count": len(issues["extra"]),
                "seed_status": seed_state.get("status"),
                "transmission_status": transmission_state.get("status"),
                "transmission_torrent_count": transmission_state.get("torrent_count", 0),
                "transmission_completed_count": transmission_state.get("completed_count", 0),
            }
            snapshots.append(snapshot)

            _write_json(raw_status_root / f"{mode}_seed_latest.json", seed_state)
            _write_json(raw_status_root / f"{mode}_transmission_latest.json", transmission_state)

            if not issues["missing"] and not issues["mismatched"]:
                _write_json(mode_run_root / "normalized_status.json", {"snapshots": snapshots})
                _write_json(
                    mode_run_root / "validator_report.json",
                    {"mode": mode, "issues": issues, "result": "pass"},
                )
                return ScenarioResult(
                    mode=mode,
                    ok=True,
                    duration_secs=time.monotonic() - start,
                    missing=[],
                    extra=issues["extra"],
                    mismatched=[],
                )

            signature = f"{len(issues['missing'])}:{len(issues['mismatched'])}:{len(issues['extra'])}"
            if signature != last_signature:
                last_signature = signature
                last_change = time.monotonic()

            if (time.monotonic() - last_change) <= defaults.stable_window_secs:
                poll = defaults.status_poll_active_secs
            else:
                poll = defaults.status_poll_idle_secs
            time.sleep(poll)

        issues = validate_output(transmission_output_root, expected)
        _write_json(mode_run_root / "normalized_status.json", {"snapshots": snapshots})
        _write_json(
            mode_run_root / "validator_report.json",
            {"mode": mode, "issues": issues, "result": "timeout"},
        )
        return ScenarioResult(
            mode=mode,
            ok=False,
            duration_secs=time.monotonic() - start,
            missing=issues["missing"],
            extra=issues["extra"],
            mismatched=issues["mismatched"],
        )
    finally:
        (logs_root / "compose_ps.txt").write_text(compose.ps(), encoding="utf-8")
        seed.collect_logs(logs_root)
        transmission.collect_logs(logs_root)
        (logs_root / "tracker.log").write_text(compose.logs("tracker", tail=1000), encoding="utf-8")
        compose.down()


def generate_fixtures_and_torrents(root: Path, announce_url: str) -> Path:
    generated_torrents = root / "integration_tests" / "artifacts" / "generated_torrents"
    subprocess.run(["python3", "local_scripts/generate_integration_bins.py"], cwd=root, check=True)
    subprocess.run(
        [
            "python3",
            "local_scripts/generate_integration_torrents.py",
            "--announce-url",
            announce_url,
            "--output-root",
            str(generated_torrents),
        ],
        cwd=root,
        check=True,
    )
    return generated_torrents
