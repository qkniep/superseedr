from __future__ import annotations

from pathlib import Path

from integration_tests.harness.clients.base import ClientAdapter


class TransmissionAdapter(ClientAdapter):
    def start(self) -> None:
        raise NotImplementedError("TransmissionAdapter is planned but not implemented in phase 1")

    def stop(self) -> None:
        raise NotImplementedError("TransmissionAdapter is planned but not implemented in phase 1")

    def add_torrent(self, torrent_path: str, download_dir: str) -> None:
        _ = torrent_path
        _ = download_dir
        raise NotImplementedError("TransmissionAdapter is planned but not implemented in phase 1")

    def wait_for_download(self, expected_manifest: dict, timeout_secs: int) -> bool:
        _ = expected_manifest
        _ = timeout_secs
        raise NotImplementedError("TransmissionAdapter is planned but not implemented in phase 1")

    def collect_logs(self, dest_dir: Path) -> None:
        _ = dest_dir
        raise NotImplementedError("TransmissionAdapter is planned but not implemented in phase 1")
