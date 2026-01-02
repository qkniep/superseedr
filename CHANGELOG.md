# Changelog

## Release v0.9.30
### Performance
- Optimized BitTorrent v2 verification with small-file root lookup bypassing.
- Implemented memory-aware cleanup logic for v2 pending data buffers.
- Improved piece request pipelining with deterministic rarity-first selection.

### Refactoring
- Introduced BitTorrent v2 and Hybrid torrent support (BEP 52).
- Implemented Merkle tree verification engine for v2 data integrity.
- Refactored torrent parser to handle v2 file trees and synthetic padding files (BEP 47).
- Decoupled piece geometry from contiguous streams to support file-aligned pieces.
- Enhanced TUI with an "Add Torrent" file picker and improved watch folder management.

### Testing
- Added comprehensive v2/hybrid integration tests covering boundary alignment and proof verification.
- Introduced scale tests for 1000-piece torrents to verify pipeline stability.
- Added proptest-based network fault injection for the state machine.

## Release v0.9.29
### Performance
- Introduced "Smart Table" logic to dynamically hide columns based on priority and width.
- Optimized TUI event listener to use non-blocking polls for better shutdown responsiveness.

### Refactoring
- Major TUI refactor: decoupled layout calculation from rendering logic.
- Modularized TUI components into `src/tui/` directory.
- Introduced `LayoutContext` and `LayoutPlan` for structured UI management.

### Testing
- Added unit tests for new TUI navigation logic.
- Enhanced `Settings` parsing tests with comprehensive coverage.

## Release v0.9.28 
### Performance
- Implemented a dynamic request window size in `PeerSession` to improve download throughput.
- Optimized `TokenBucket` to reduce lock contention for unlimited rates.
- Improved network writer performance by batching messages to reduce syscalls.

### Refactoring
- Replaced single block requests with a `BulkRequest` system for better pipelining.
- Updated `web_seed_worker` to use the new bulk request system.
- Refactored `TorrentManager` and its state machine to support bulk commands.

### Testing
- Added extensive tests for the new dynamic window sizing logic in `PeerSession`.
- Added a proptest regression file to save and re-run failure cases.


## Release v0.9.27
### Features
- Added block manager to improve download performance.

### Bug Fixes
- Updated torrent sorting weight for better prioritization.
- Added more tests and fixed tolerance issues.

### Refactoring
- Consolidated and adjusted TUI components.
- Added testing and integration via composition.

### Performance
- Increased in-flight request limits for better throughput.


## Release v0.9.26
### Features
- **Advanced Networking**: Implemented `web-seed-workers` for improved seeding, and an "effect pattern" for more resilient network communication. Added network simulations for robust testing.
- **Core Refactoring**: Major refactoring of the codebase for better performance and maintainability, including the implementation of a resource manager and an adaptive seek penalty.
### Bugs
- **Comprehensive Testing**: Introduced a wide range of testing strategies, including chaos engineering, fuzz testing, and state machine-based tests to ensure stability and reliability.

## Initial Features
- **Cross-Platform Support**: Added robust support for major operating systems, including Windows (Wix installer), macOS (notarized builds), and Linux (MUSL builds).
- **Dynamic TUI**: Overhauled the Text User Interface (TUI) with new features like a swarm heatmap, peer activity lanes, and dynamic resizing, providing a more informative and user-friendly experience.
- **Docker Integration**: Full Docker support with examples for docker-compose, multi-architecture builds (ARM), and integrated VPN (Gluetun) support for enhanced privacy.
- **CI/CD Pipeline**: Established a comprehensive CI/CD pipeline using GitHub Actions for automated testing, linting, and releases.
