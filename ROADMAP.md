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

# Detailed Roadmap Steps

## Phase: 1 - v1.0
**Goal:** Core stability, basic usability, and integration testing.

### Integration Testing
- **phase: 1 - v1.0** | Containerized Test Environment - Docker setup for qBit/Transmission interoperability | [Issue #____]
- **phase: 1 - v1.0** | Interop Test Runner - script to verify transfer success between clients | [Issue #____]
- **phase: 1 - v1.0** | CI/CD Pipeline - auto-run integration tests on Pull Requests | [Issue #____]

---

## Phase: 1.5 - v1.5
**Goal:** TUI polish, RSS, and advanced management features.

### Log Levels and TUI
- **phase: 1 - v1.0** | Structured Logging - implement level-based logging (INFO, DEBUG, WARN) | [Issue #____]
- **phase: 1 - v1.0** | Log Widget - scrollable text view within TUI to tail logs | [Issue #____]
- **phase: 1 - v1.0** | Configurable Log Level - user setting to change verbosity at runtime | [Issue #____]

### Fully Asynchronous Validation
- **phase: 1 - v1.0** | Async Hashing - move file verification to dedicated thread pool to prevent UI freezing | [Issue #____]
- **phase: 1 - v1.0** | Validation Progress - granular progress events sent back to UI/Logger | [Issue #____]
- **phase: 1 - v1.0** | Revalidation Logic - handling forced re-checks on existing files | [Issue #____]

### RSS Feed Support (TBD Plugin System)
- **phase: 1.5 - v1.5** | RSS Parser implementation - parse standard RSS XML feeds | [Issue #____]
- **phase: 1.5 - v1.5** | Filter Engine - regex/text matching for auto-downloading items | [Issue #____]
- **phase: 1.5 - v1.5** | RSS Manager UI - TUI screen to add, remove, and view feed status | [Issue #____]
- **phase: 1.5 - v1.5** | Polling Scheduler - configurable interval for checking feed updates | [Issue #____]

### Config Screen Redesign
- **phase: 1.5 - v1.5** | Input Field Refactor - support for complex types (dropdowns, toggles) | [Issue #____]
- **phase: 1.5 - v1.5** | Categorized Views - tabbed or sectional layout for different config groups | [Issue #____]
- **phase: 1.5 - v1.5** | Field Validation - immediate visual feedback for invalid user inputs | [Issue #____]

### Advanced Torrent Management
- **phase: 1.5 - v1.5** | Multi-select State - internal logic to track multiple selected rows | [Issue #____]
- **phase: 1.5 - v1.5** | Bulk Actions - apply start, stop, delete to selection context | [Issue #____]
- **phase: 1.5 - v1.5** | Grouping/Tagging - data structure to associate torrents with arbitrary tags | [Issue #____]

### User Adjustables (Columns)
- **phase: 1.5 - v1.5** | Column Config Loader - read column visibility/order from config | [Issue #____]
- **phase: 1.5 - v1.5** | Auto-hiding Logic - dynamically hide columns based on terminal width | [Issue #____]
- **phase: 1.5 - v1.5** | Smart Units - toggle for raw bytes vs human-readable units | [Issue #____]

### Internal TUI Architecture Refactor
- **phase: 1.5 - v1.5** | View/State Decoupling - separate rendering logic from application state | [Issue #____]
- **phase: 1.5 - v1.5** | Event Bus Implementation - standardized event passing for UI updates | [Issue #____]
- **phase: 1.5 - v1.5** | Widget Componentization - create reusable UI components for consistency | [Issue #____]

### Alternative and Custom Themes
- **phase: 1.5 - v1.5** | Theme Schema - define JSON/TOML structure for TUI colors/styles | [Issue #____]
- **phase: 1.5 - v1.5** | Style Loader - logic to apply hex/ansi colors to TUI widgets at runtime | [Issue #____]
- **phase: 1.5 - v1.5** | Theme Selection UI - menu to swap themes live | [Issue #____]

### Peer Stream Redesign
- **phase: 1.5 - v1.5** | Data Visualization - map transfer rates to visual cues (color/size) | [Issue #____]
- **phase: 1.5 - v1.5** | Opacity Rendering - fade inactive peers visually | [Issue #____]

---

## Phase: 2 - v2.0+
**Goal:** Advanced networking, swarm observability, and custom layouts.

### Persistent Data Across Sessions
- **phase: 2 - v2.0+** | Stats Database - local storage (SQLite/KV store) for long-term metrics | [Issue #____]
- **phase: 2 - v2.0+** | Peer Reputation Logic - track peer ratios/behavior over time | [Issue #____]
- **phase: 2 - v2.0+** | Peer Blocklist - auto-block peers based on historical reputation | [Issue #____]

### Scriptability and CLI Enhancements
- **phase: 2 - v2.0+** | Headless Mode - run core without TUI rendering | [Issue #____]
- **phase: 2 - v2.0+** | CLI Control Surface - commands to pause/resume torrents via CLI args | [Issue #____]
- **phase: 2 - v2.0+** | JSON Output - ability to pipe torrent status as JSON for external scripts | [Issue #____]

### User Configurable Layouts
- **phase: 2 - v2.0+** | Layout Serialization - save panel positions/sizes to config | [Issue #____]
- **phase: 2 - v2.0+** | Interactive Resize - keybindings to resize panes | [Issue #____]
- **phase: 2 - v2.0+** | Drag-n-Drop Logic - (Simulated) movement of panes within the grid | [Issue #____]

### Peer Churn Overload Management
- **phase: 2 - v2.0+** | Connection Capping - hard limits on active vs. embryonic connections | [Issue #____]
- **phase: 2 - v2.0+** | Aggressive Pruning - heuristics to disconnect slow/useless peers under load | [Issue #____]
- **phase: 2 - v2.0+** | 10k Scale Test - performance profiling with massive peer lists | [Issue #____]
