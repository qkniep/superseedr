from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from urllib import error as url_error
from urllib import request as url_request

from integration_tests.harness.clients.qbittorrent import QBittorrentAdapter
from integration_tests.harness.clients.superseedr import SuperseedrAdapter
from integration_tests.harness.config import HarnessDefaults, HarnessPaths
from integration_tests.harness.docker_ctl import DockerCompose
from integration_tests.harness.manifest import ExpectedFile, build_expected_manifest, validate_output


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


def _qbit_savepath_for_torrent(mode: str, name: str) -> str:
    if name.startswith("single_"):
        return f"/downloads/{mode}/single"
    if name in {"multi_file.torrent", "nested.torrent"}:
        return f"/downloads/{mode}"
    raise ValueError(f"Unsupported torrent fixture: {name}")


def _torrent_order_key(name: str) -> tuple[int, str]:
    # Seed single-file torrents first to reduce cross-torrent interference while leech warms up.
    if name.startswith("single_"):
        return (0, name)
    if name == "multi_file.torrent":
        return (1, name)
    if name == "nested.torrent":
        return (2, name)
    return (3, name)


def _expected_subset(expected: dict[str, ExpectedFile], torrent_names: list[str]) -> dict[str, ExpectedFile]:
    include: set[str] = set()
    for name in torrent_names:
        if name.startswith("single_"):
            include.add(f"single/{name.removesuffix('.torrent')}")
            continue
        if name == "multi_file.torrent":
            include.update(rel for rel in expected if rel.startswith("multi_file/"))
            continue
        if name == "nested.torrent":
            include.update(rel for rel in expected if rel.startswith("nested/"))
            continue
        raise ValueError(f"Unsupported torrent fixture: {name}")
    return {rel: spec for rel, spec in expected.items() if rel in include}


def _write_leech_settings(mode: str, config_path: Path, torrent_files: list[str]) -> None:
    role_root = f"/superseedr-data/leech/{mode}"
    lines = [
        'client_id = "-SS1000-LEECHCLIENT1"',
        "client_port = 16882",
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
    start = time.monotonic()

    mode_run_root = run_root / mode
    seed_data_root = mode_run_root / "seed_data"
    leech_data_root = mode_run_root / "leech_data"
    leech_config_root = mode_run_root / "leech_config"
    leech_share_root = mode_run_root / "leech_share"
    seed_config_root = mode_run_root / "seed_config_unused"
    seed_share_root = mode_run_root / "seed_share_unused"
    qbit_config_root = mode_run_root / "qbit_config"
    qbit_downloads_root = mode_run_root / "qbit_downloads"
    logs_root = mode_run_root / "logs"
    raw_status_root = mode_run_root / "raw_client_status"
    staged_fixtures_root = mode_run_root / "fixtures"

    _ensure_clean_dir(mode_run_root)
    seed_data_root.mkdir(parents=True, exist_ok=True)
    leech_data_root.mkdir(parents=True, exist_ok=True)
    leech_config_root.mkdir(parents=True, exist_ok=True)
    leech_share_root.mkdir(parents=True, exist_ok=True)
    seed_config_root.mkdir(parents=True, exist_ok=True)
    seed_share_root.mkdir(parents=True, exist_ok=True)
    qbit_config_root.mkdir(parents=True, exist_ok=True)
    qbit_downloads_root.mkdir(parents=True, exist_ok=True)
    logs_root.mkdir(parents=True, exist_ok=True)
    (staged_fixtures_root / "torrents").mkdir(parents=True, exist_ok=True)

    torrents_mode_root = torrents_root / mode
    torrent_files = sorted(p.name for p in torrents_mode_root.glob("*.torrent"))
    if not torrent_files:
        raise RuntimeError(f"No torrent fixtures found for mode={mode} under {torrents_mode_root}")

    _prepare_seed_data(qbit_downloads_root / mode, harness_paths.test_data_root)
    shutil.copytree(torrents_root, staged_fixtures_root / "torrents", dirs_exist_ok=True)
    _write_leech_settings(mode, leech_config_root / "settings.toml", torrent_files)

    project_name = f"interop_qbit_rev_{mode}_{int(time.time())}"
    tracker_port = _reserve_local_port()
    qbit_web_port = _reserve_local_port()
    compose_env = {
        "INTEROP_PROJECT_NAME": project_name,
        "INTEROP_UID": str(os.getuid()),
        "INTEROP_GID": str(os.getgid()),
        "INTEROP_TRACKER_PORT": str(tracker_port),
        "INTEROP_TRACKER_SCRIPT_PATH": str(harness_paths.tracker_script.resolve()),
        "INTEROP_FIXTURES_PATH": str(staged_fixtures_root.resolve()),
        "INTEROP_SEED_DATA_PATH": str(seed_data_root.resolve()),
        "INTEROP_SEED_CONFIG_PATH": str(seed_config_root.resolve()),
        "INTEROP_SEED_SHARE_PATH": str(seed_share_root.resolve()),
        "INTEROP_LEECH_DATA_PATH": str(leech_data_root.resolve()),
        "INTEROP_LEECH_CONFIG_PATH": str(leech_config_root.resolve()),
        "INTEROP_LEECH_SHARE_PATH": str(leech_share_root.resolve()),
        "INTEROP_QBIT_CONFIG_PATH": str(qbit_config_root.resolve()),
        "INTEROP_QBIT_DOWNLOADS_PATH": str(qbit_downloads_root.resolve()),
        "INTEROP_QBIT_WEBUI_PORT": str(qbit_web_port),
    }

    compose = DockerCompose(harness_paths.compose_file, project_name, compose_env)
    qbit = QBittorrentAdapter(
        compose=compose,
        service_name="qbittorrent",
        base_url=f"http://127.0.0.1:{qbit_web_port}",
        auth_timeout_secs=120,
    )
    leech_output_root = leech_data_root / mode
    leech = SuperseedrAdapter(compose, "superseedr_leech", leech_output_root, leech_share_root)
    expected = build_expected_manifest(harness_paths.test_data_root, mode)
    ordered_torrents = sorted(torrent_files, key=_torrent_order_key)
    added_torrents: list[str] = []
    next_torrent_idx = 0

    snapshots: list[dict] = []
    last_signature = ""
    last_change = time.monotonic()

    try:
        compose.run(["build", "superseedr_leech"])
        compose.up(["tracker"], no_build=True)
        _wait_for_tracker(tracker_port)
        compose.up(["superseedr_leech"], no_build=True)
        qbit.start()

        if ordered_torrents:
            torrent_name = ordered_torrents[0]
            torrent_path = staged_fixtures_root / "torrents" / mode / torrent_name
            qbit.add_torrent(str(torrent_path), _qbit_savepath_for_torrent(mode, torrent_name))
            qbit.set_force_start("all", enabled=True)
            added_torrents.append(torrent_name)
            next_torrent_idx = 1

        deadline = time.monotonic() + timeout_secs
        while time.monotonic() < deadline:
            issues = validate_output(leech_output_root, expected)
            progress_expected = _expected_subset(expected, added_torrents)
            progress_issues = validate_output(leech_output_root, progress_expected)
            qbit_state = qbit.read_status()
            leech_state = leech.read_status()

            snapshot = {
                "mode": mode,
                "timestamp": int(time.time()),
                "missing_count": len(issues["missing"]),
                "mismatched_count": len(issues["mismatched"]),
                "extra_count": len(issues["extra"]),
                "qbit_status": qbit_state.get("status"),
                "qbit_torrent_count": qbit_state.get("torrent_count", 0),
                "qbit_completed_count": qbit_state.get("completed_count", 0),
                "leech_status": leech_state.get("status"),
                "added_torrent_count": len(added_torrents),
                "progress_missing_count": len(progress_issues["missing"]),
                "progress_mismatched_count": len(progress_issues["mismatched"]),
            }
            snapshots.append(snapshot)

            _write_json(raw_status_root / f"{mode}_qbit_latest.json", qbit_state)
            _write_json(raw_status_root / f"{mode}_leech_latest.json", leech_state)

            if (
                not progress_issues["missing"]
                and not progress_issues["mismatched"]
                and next_torrent_idx < len(ordered_torrents)
            ):
                torrent_name = ordered_torrents[next_torrent_idx]
                torrent_path = staged_fixtures_root / "torrents" / mode / torrent_name
                qbit.add_torrent(str(torrent_path), _qbit_savepath_for_torrent(mode, torrent_name))
                qbit.set_force_start("all", enabled=True)
                added_torrents.append(torrent_name)
                next_torrent_idx += 1

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

        issues = validate_output(leech_output_root, expected)
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
        qbit.collect_logs(logs_root)
        leech.collect_logs(logs_root)
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
