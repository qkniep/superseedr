// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::dht_actor_monitor_enabled;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::dht::service) struct DhtActionEffectSnapshot {
    pub(in crate::dht::service) domain: &'static str,
    pub(in crate::dht::service) action: &'static str,
    pub(in crate::dht::service) effect_count: usize,
    pub(in crate::dht::service) effects: Vec<&'static str>,
}

pub(in crate::dht::service) fn action_effect_snapshot(
    domain: &'static str,
    action: &'static str,
    effects: Vec<&'static str>,
) -> DhtActionEffectSnapshot {
    DhtActionEffectSnapshot {
        domain,
        action,
        effect_count: effects.len(),
        effects,
    }
}

pub(in crate::dht::service) fn observe_action_effect_reduction<I>(
    domain: &'static str,
    action: &'static str,
    effects: I,
) where
    I: IntoIterator<Item = &'static str>,
{
    if !dht_actor_monitor_enabled() {
        return;
    }

    let snapshot = action_effect_snapshot(domain, action, effects.into_iter().collect());
    tracing::info!(
        target: "superseedr::dht_actor",
        event = "reduce",
        domain = snapshot.domain,
        action = snapshot.action,
        effect_count = snapshot.effect_count,
        effects = %snapshot.effects.join(","),
        "DHT action/effect reduction observed",
    );
}
