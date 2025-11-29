# Frequently Asked Questions (FAQ)

## General

### What is Superseedr?

Superseedr is a command-line BitTorrent client written in Rust. It is designed to be a lightweight, efficient, and reliable client for downloading and seeding torrents.

### How does Superseedr work?

Superseedr follows the BitTorrent protocol to download and upload files. It connects to a tracker to find other peers who are sharing the same file. It then connects to these peers to download pieces of the file and upload pieces that it has already downloaded.

### Is Superseedr legal?

The Superseedr software is legal. However, the legality of downloading and sharing files using BitTorrent depends on the content being shared. It is your responsibility to ensure that you are not infringing on any copyrights.

### Is Superseedr safe?

For users concerned with privacy, superseedr does provide a docker compose solution using gluetun.

Superseedr does not provide SOCKS5 or proxies. SOCKS5 is designed to be unencrypted, so any client with this feature will leak clear text data (tcp, udp, http) without specialized local server setups. Its is recommended to use network isolation such as docker to solve this.

### How can I improve my download speed?

*   **Remove all limits:** If you have set an upload limit, remove it. Peers are more willing to send you data if you upload to them.
*   **Choose a torrent with many seeds:** The more seeds a torrent has, the more sources you can download from, which can increase your download speed.
*   **Check your internet connection:** A slow internet connection will result in slow download speeds.

## Terminology

### What is a torrent?

A torrent is a small file that contains metadata about the files to be downloaded. This metadata includes the names of the files, their sizes, and the address of the tracker.

### What is a seed?

A seed is a peer who has a complete copy of the file and is sharing it with others.

### What is a peer?

A peer is a user who is downloading or uploading a file.

### What is a tracker?

A tracker is a server that keeps track of the peers who are sharing a file. When you start downloading a torrent, your client connects to the tracker to get a list of peers.

### What is a magnet link?

A magnet link is a type of hyperlink that contains all the information needed to start downloading a torrent. It is an alternative to downloading a torrent file.

