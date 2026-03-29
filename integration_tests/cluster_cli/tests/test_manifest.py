from __future__ import annotations

from integration_tests.cluster_cli.manifest import (
    load_fixture_manifest,
    magnet_info_hash_hex,
    torrent_info_hash_hex,
)


def test_declared_cluster_fixtures_exist_and_match_hashes() -> None:
    fixtures = load_fixture_manifest()
    assert fixtures, "cluster CLI fixture manifest should not be empty"
    for fixture in fixtures:
        assert fixture.torrent_path.exists(), f"missing torrent fixture: {fixture.torrent_path}"
        assert fixture.payload_path.exists(), f"missing payload fixture: {fixture.payload_path}"
        assert torrent_info_hash_hex(fixture.torrent_path) == fixture.info_hash_hex
        assert magnet_info_hash_hex(fixture.magnet_uri) == fixture.info_hash_hex
