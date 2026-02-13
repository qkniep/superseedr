from __future__ import annotations

import json
import time
from pathlib import Path

from integration_tests.harness.clients.base import ClientAdapter
from integration_tests.harness.docker_ctl import DockerCompose
from integration_tests.harness.manifest import validate_output


class SuperseedrAdapter(ClientAdapter):
    def __init__(
        self,
        compose: DockerCompose,
        service_name: str,
        output_root: Path,
        share_root: Path,
    ) -> None:
        self.compose = compose
        self.service_name = service_name
        self.output_root = output_root
        self.share_root = share_root

    def start(self) -> None:
        self.compose.up([self.service_name])

    def stop(self) -> None:
        self.compose.down()

    def add_torrent(self, torrent_path: str, download_dir: str) -> None:
        # Primary scenario preloads torrents via settings.toml.
        # This path is kept for future adapter parity.
        _ = download_dir
        self.compose.exec(self.service_name, ["superseedr", "add", torrent_path], check=True)

    def wait_for_download(self, expected_manifest: dict, timeout_secs: int) -> bool:
        deadline = time.monotonic() + timeout_secs
        while time.monotonic() < deadline:
            issues = validate_output(self.output_root, expected_manifest)
            if not issues["missing"] and not issues["mismatched"]:
                return True
            time.sleep(1)
        return False

    def collect_logs(self, dest_dir: Path) -> None:
        dest_dir.mkdir(parents=True, exist_ok=True)
        logs = self.compose.logs(self.service_name, tail=1000)
        (dest_dir / f"{self.service_name}.log").write_text(logs, encoding="utf-8")

        state_file = self.share_root / "status_files" / "app_state.json"
        if state_file.exists():
            raw = state_file.read_text(encoding="utf-8")
            (dest_dir / f"{self.service_name}_app_state.json").write_text(raw, encoding="utf-8")

    def read_status(self) -> dict:
        state_file = self.share_root / "status_files" / "app_state.json"
        if not state_file.exists():
            return {"service": self.service_name, "status": "missing_state_json"}
        try:
            payload = json.loads(state_file.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            return {
                "service": self.service_name,
                "status": "invalid_state_json",
                "error": str(exc),
            }

        return {
            "service": self.service_name,
            "status": "ok",
            "observed_at": int(time.time()),
            "raw": payload,
        }
