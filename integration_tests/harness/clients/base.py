from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path


class ClientAdapter(ABC):
    @abstractmethod
    def start(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def stop(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def add_torrent(self, torrent_path: str, download_dir: str) -> None:
        raise NotImplementedError

    @abstractmethod
    def wait_for_download(self, expected_manifest: dict, timeout_secs: int) -> bool:
        raise NotImplementedError

    @abstractmethod
    def collect_logs(self, dest_dir: Path) -> None:
        raise NotImplementedError
