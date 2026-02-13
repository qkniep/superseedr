#!/usr/bin/env python3
"""Minimal BitTorrent HTTP tracker for local integration tests."""

from __future__ import annotations

import os
import socket
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlparse

INTERVAL_SECS = int(os.environ.get("TRACKER_INTERVAL_SECS", "10"))


def bencode(value: object) -> bytes:
    if isinstance(value, int):
        return f"i{value}e".encode("ascii")
    if isinstance(value, bytes):
        return f"{len(value)}:".encode("ascii") + value
    if isinstance(value, str):
        data = value.encode("utf-8")
        return f"{len(data)}:".encode("ascii") + data
    if isinstance(value, list):
        return b"l" + b"".join(bencode(v) for v in value) + b"e"
    if isinstance(value, dict):
        items: list[bytes] = []
        for key in sorted(value.keys()):
            key_bytes = key if isinstance(key, bytes) else str(key).encode("utf-8")
            items.append(bencode(key_bytes))
            items.append(bencode(value[key]))
        return b"d" + b"".join(items) + b"e"
    raise TypeError(f"Unsupported bencode type: {type(value)!r}")


class PeerStore:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._by_info_hash: dict[bytes, dict[bytes, tuple[str, int, float]]] = {}

    def update(self, info_hash: bytes, peer_id: bytes, ip: str, port: int, left: int) -> None:
        now = time.time()
        with self._lock:
            peers = self._by_info_hash.setdefault(info_hash, {})
            if left == 0:
                peers[peer_id] = (ip, port, now)
            else:
                peers[peer_id] = (ip, port, now)

    def list_peers(self, info_hash: bytes, requester_peer_id: bytes) -> bytes:
        now = time.time()
        compact = bytearray()
        with self._lock:
            peers = self._by_info_hash.get(info_hash, {})
            stale = [pid for pid, (_, _, ts) in peers.items() if now - ts > 600]
            for pid in stale:
                peers.pop(pid, None)
            for pid, (ip, port, _) in peers.items():
                if pid == requester_peer_id:
                    continue
                try:
                    compact.extend(socket.inet_aton(ip))
                except OSError:
                    continue
                compact.extend(port.to_bytes(2, "big", signed=False))
        return bytes(compact)


STORE = PeerStore()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send_bencoded(self, payload: dict[str, object], status: int = 200) -> None:
        body = bencode(payload)
        self.send_response(status)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        if parsed.path != "/announce":
            self._send_bencoded({"failure reason": "unsupported path"}, status=404)
            return

        params = dict(parse_qsl(parsed.query, keep_blank_values=True, encoding="latin-1"))
        info_hash = params.get("info_hash", "").encode("latin-1")
        peer_id = params.get("peer_id", "").encode("latin-1")
        port = int(params.get("port", "0") or "0")
        left = int(params.get("left", "0") or "0")

        if not info_hash or not peer_id or port <= 0:
            self._send_bencoded({"failure reason": "missing info_hash/peer_id/port"}, status=400)
            return

        remote_ip = self.client_address[0]
        STORE.update(info_hash=info_hash, peer_id=peer_id, ip=remote_ip, port=port, left=left)
        peers = STORE.list_peers(info_hash=info_hash, requester_peer_id=peer_id)
        print(
            f"announce info_hash={info_hash.hex()} peer_id={peer_id.decode('latin-1', errors='ignore')} "
            f"ip={remote_ip} port={port} left={left} peers_out={len(peers) // 6}",
            flush=True,
        )

        self._send_bencoded(
            {
                "interval": INTERVAL_SECS,
                "min interval": 3,
                "complete": 0,
                "incomplete": 0,
                "peers": peers,
            }
        )

    def log_message(self, fmt: str, *args: object) -> None:
        return


if __name__ == "__main__":
    host = "0.0.0.0"
    port = 6969
    server = ThreadingHTTPServer((host, port), Handler)
    print(f"Tracker listening on {host}:{port}", flush=True)
    server.serve_forever()
