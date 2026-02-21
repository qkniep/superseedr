from __future__ import annotations

import base64
import json
import time
import urllib.request
from pathlib import Path
from typing import Any
from urllib import error as url_error

from integration_tests.harness.clients.base import ClientAdapter
from integration_tests.harness.docker_ctl import DockerCompose


class TransmissionAdapter(ClientAdapter):
    def __init__(
        self,
        compose: DockerCompose | None = None,
        service_name: str = "transmission",
        base_url: str = "http://127.0.0.1:19091/transmission/rpc",
        username: str = "interop",
        password: str = "interop",
        auth_timeout_secs: int = 60,
    ) -> None:
        self.compose = compose
        self.service_name = service_name
        self.base_url = base_url
        self.username = username
        self.password = password
        self.auth_timeout_secs = auth_timeout_secs
        self.poll_interval_secs = 1.0
        self._session_id: str | None = None

    def start(self) -> None:
        if self.compose is not None:
            self.compose.up([self.service_name], no_build=True)
        self.wait_until_ready()

    def stop(self) -> None:
        if self.compose is not None:
            self.compose.run(["stop", self.service_name], check=False)

    def _headers(self) -> dict[str, str]:
        token = base64.b64encode(f"{self.username}:{self.password}".encode("utf-8")).decode("ascii")
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Basic {token}",
        }
        if self._session_id:
            headers["X-Transmission-Session-Id"] = self._session_id
        return headers

    def _rpc(self, method: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        payload = json.dumps({"method": method, "arguments": arguments or {}}).encode("utf-8")

        for _ in range(2):
            request = urllib.request.Request(self.base_url, data=payload, method="POST", headers=self._headers())
            try:
                with urllib.request.urlopen(request, timeout=10) as response:
                    body = response.read().decode("utf-8", errors="replace")
                parsed = json.loads(body)
                if parsed.get("result") != "success":
                    raise RuntimeError(
                        f"Transmission RPC failed method={method} result={parsed.get('result')!r}"
                    )
                args = parsed.get("arguments", {})
                if not isinstance(args, dict):
                    raise RuntimeError(
                        f"Transmission RPC returned invalid arguments for method={method}: {type(args)}"
                    )
                return args
            except url_error.HTTPError as exc:
                if exc.code != 409:
                    raise RuntimeError(f"Transmission HTTP error method={method} code={exc.code}") from exc
                session_id = exc.headers.get("X-Transmission-Session-Id")
                if not session_id:
                    raise RuntimeError("Transmission missing X-Transmission-Session-Id on 409 response") from exc
                self._session_id = session_id

        raise RuntimeError(f"Transmission RPC did not succeed after session handshake method={method}")

    def wait_until_ready(self) -> None:
        deadline = time.monotonic() + self.auth_timeout_secs
        last_error: Exception | None = None

        while time.monotonic() < deadline:
            try:
                self._rpc("session-get")
                return
            except Exception as exc:
                last_error = exc
                time.sleep(1)

        raise RuntimeError(f"Failed to connect/authenticate Transmission at {self.base_url}") from last_error

    def add_torrent(self, torrent_path: str, download_dir: str) -> None:
        path = Path(torrent_path)
        if not path.exists():
            raise FileNotFoundError(f"Torrent file not found: {torrent_path}")

        metainfo = base64.b64encode(path.read_bytes()).decode("ascii")
        self._rpc(
            "torrent-add",
            {
                "metainfo": metainfo,
                "download-dir": download_dir,
                "paused": False,
            },
        )

    def _list_torrents(self) -> list[dict[str, Any]]:
        args = self._rpc(
            "torrent-get",
            {
                "fields": [
                    "id",
                    "hashString",
                    "name",
                    "percentDone",
                    "status",
                    "error",
                    "errorString",
                    "leftUntilDone",
                    "isFinished",
                ]
            },
        )
        torrents = args.get("torrents", [])
        if not isinstance(torrents, list):
            raise RuntimeError("Transmission torrent-get returned invalid torrents payload")
        return torrents

    def wait_for_download(self, expected_manifest: dict, timeout_secs: int) -> bool:
        _ = expected_manifest
        deadline = time.monotonic() + timeout_secs
        while time.monotonic() < deadline:
            torrents = self._list_torrents()
            if torrents:
                has_error = any(int(t.get("error", 0)) != 0 for t in torrents)
                if has_error:
                    return False

                all_complete = all(int(t.get("leftUntilDone", 1)) == 0 for t in torrents)
                if all_complete:
                    return True
            time.sleep(self.poll_interval_secs)
        return False

    def collect_logs(self, dest_dir: Path) -> None:
        if self.compose is None:
            return
        dest_dir.mkdir(parents=True, exist_ok=True)
        logs = self.compose.logs(self.service_name, tail=1000)
        (dest_dir / f"{self.service_name}.log").write_text(logs, encoding="utf-8")

    def read_status(self) -> dict[str, Any]:
        try:
            torrents = self._list_torrents()
        except Exception as exc:
            return {
                "service": self.service_name,
                "status": "api_error",
                "error": str(exc),
                "observed_at": int(time.time()),
            }

        completed = sum(1 for t in torrents if int(t.get("leftUntilDone", 1)) == 0)
        return {
            "service": self.service_name,
            "status": "ok",
            "observed_at": int(time.time()),
            "torrent_count": len(torrents),
            "completed_count": completed,
            "raw": torrents,
        }
