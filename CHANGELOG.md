# Changelog

## Release v0.9.28 
### Performance
- Optimized bandwidth management with a new "Token Wallet" system, reducing global lock contention.

### Testing
- Added comprehensive performance tests for the peer session, including pipeline saturation and token wallet behavior.
- Added a high-throughput event loop test for the torrent manager.
- Added resource management tests for CPU hashing and disk backpressure.
- Introduced property-based testing with `proptest` for the token wallet.

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
