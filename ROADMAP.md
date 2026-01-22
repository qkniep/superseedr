# Roadmap
This document will serve as a high level guide to the direction of superseedr.
Consider this roadmap as mostly stable, but flexible to minor changes in terms of features and timing.
For specific tracking details visit the issues page and filter via labels.


# Big Features
- **RSS Feed Support:** Automatically monitor RSS feeds and download new torrents matching user-defined filters.
- **Config Screen Redesign:** Modernize and refactor the config screen management for more complex user inputs.
- **Advanced Torrent Management:** TUI Screen to manage the torrent list. Full features such as multi-select and grouping.
- **User Adjustables:** Configs to allow user to select which columns are visible on tables, ability to set it to auto (dl/ul aware).
- **Alternative and Custom Themes:** Extensible themes defined by superseedr and the users. Fancy screen to config this, saved as json/toml.
- **Internal TUI architecture refactor:** Proper architecture for the TUI layer, modern best practices for UI management.
- **Persistent Data Across Sessions:** Network graphs, system data, and peering stats persist across sessions. Allows to block malicious peers based on ratio.
- **Scriptability and CLI Enhancements:** Torrent management and period stats made available.
- **Log Levels and TUI:** Allow users to set log levels (INFO, DEBUG, WARN...) and to allow log access within the TUI.
- **User Configurable Layouts:** Drag-n-Drop layouting of the main TUI.
- **Peer Stream Redesign:** Enable more visualizations in the peer stream focusing more on size, color, and opacity.
- **Fully asynchronous validation:** Refactor for handling torrent validation and revalidations async.
- **Peer Churn Overload Management:** End-to-End peer management in scales of over 10k potential peers.
- **Integration Testing:** Automatic testing against existing clients (qbit nox, transmission).

## Roadmap to V1.0
This release will be focused on stability in the torrent core and basic usability.
Many big items were already addressed such as BitTorrent V2, selective downloading, and performance.
An additional requirement is to have the codebase in a place for maximum extensibility for the big features above.
Technical and non-technical documentation will also be updated to the latest versions.

## Roadmap to V1.5
The bulk of this release will be focused on the TUI. 
Features such as user customizable columns, RSS, and torrent list management.
This includes further polishing of current components in terms of sizing, layouts, and dynamic content.
TUI code architecture will be included here as the foundation of this release.
Heavy regression testing will take up a lot of time in this release, possibly with a beta program.

## Future (V2.0 and Beyond)
Although very far away, these releases will be where superseedr can begin to ship features that can push TUIs and BitTorrent forward.
As a new BitTorrent client, superseedr has tons of catching up to do (uTP, IPv6, UPnP, hole punch, DHT search, ..etc)
These include: custom layout engines, advance swarm observability, peering management, world map view, torrent creation, remote TUI.
