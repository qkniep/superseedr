from __future__ import annotations

import pytest

from integration_tests.harness.clients.qbittorrent import QBittorrentAdapter
from integration_tests.harness.clients.transmission import TransmissionAdapter


def test_qbittorrent_stub_is_explicit() -> None:
    adapter = QBittorrentAdapter()
    with pytest.raises(NotImplementedError, match="planned but not implemented"):
        adapter.start()


def test_transmission_stub_is_explicit() -> None:
    adapter = TransmissionAdapter()
    with pytest.raises(NotImplementedError, match="planned but not implemented"):
        adapter.start()
