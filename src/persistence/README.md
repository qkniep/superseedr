# Persistence Module

This folder owns non-settings persisted state.

For RSS implementation:
- `settings.toml` keeps durable user config (`Settings.rss`).
- `persistence/rss.toml` keeps mutable RSS runtime state (history, sync metadata, per-feed errors).
- RSS history is retention-capped at 1000 entries; oldest entries are pruned first on persist.

The runtime should treat missing/corrupt `persistence/rss.toml` as recoverable and fall back to empty RSS state.
