// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::net::SocketAddr;

use tokio::sync::mpsc;

use super::{DhtDemandState, InfoHash};

pub(super) struct DemandSubscriberDelivery {
    pub(super) subscriber_id: u64,
    pub(super) subscriber_tx: mpsc::UnboundedSender<Vec<SocketAddr>>,
}

pub(super) enum DemandSubscriberAction {
    Register {
        info_hash: InfoHash,
        demand: DhtDemandState,
        subscriber_tx: mpsc::UnboundedSender<Vec<SocketAddr>>,
    },
    Unregister {
        info_hash: InfoHash,
        subscriber_id: u64,
    },
    DeliverPeers {
        info_hash: InfoHash,
        peers: Vec<SocketAddr>,
    },
    PruneDeadSubscribers {
        info_hash: InfoHash,
        subscriber_ids: Vec<u64>,
    },
}

pub(super) enum DemandSubscriberEffect {
    Registered {
        info_hash: InfoHash,
        demand: DhtDemandState,
        subscriber_id: u64,
    },
    SubscriberRemoved {
        info_hash: InfoHash,
    },
    DeliverPeers {
        info_hash: InfoHash,
        peers: Vec<SocketAddr>,
        deliveries: Vec<DemandSubscriberDelivery>,
    },
}

#[derive(Default)]
pub(super) struct DemandSubscriberReduction {
    pub(super) subscriber_id: Option<u64>,
    pub(super) effects: Vec<DemandSubscriberEffect>,
}

pub(super) struct DemandSubscriberRegistry {
    pub(super) subscribers: HashMap<InfoHash, HashMap<u64, mpsc::UnboundedSender<Vec<SocketAddr>>>>,
    next_subscriber_id: u64,
}

impl DemandSubscriberRegistry {
    pub(super) fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            next_subscriber_id: 1,
        }
    }

    pub(super) fn update(&mut self, action: DemandSubscriberAction) -> DemandSubscriberReduction {
        match action {
            DemandSubscriberAction::Register {
                info_hash,
                demand,
                subscriber_tx,
            } => {
                let subscriber_id = self.next_subscriber_id;
                self.next_subscriber_id = self.next_subscriber_id.saturating_add(1);
                self.subscribers
                    .entry(info_hash)
                    .or_default()
                    .insert(subscriber_id, subscriber_tx);
                DemandSubscriberReduction {
                    subscriber_id: Some(subscriber_id),
                    effects: vec![DemandSubscriberEffect::Registered {
                        info_hash,
                        demand,
                        subscriber_id,
                    }],
                }
            }
            DemandSubscriberAction::Unregister {
                info_hash,
                subscriber_id,
            } => {
                let removed = self.remove_subscriber(info_hash, subscriber_id);
                DemandSubscriberReduction {
                    subscriber_id: None,
                    effects: removed
                        .then_some(DemandSubscriberEffect::SubscriberRemoved { info_hash })
                        .into_iter()
                        .collect(),
                }
            }
            DemandSubscriberAction::DeliverPeers { info_hash, peers } => {
                let deliveries = self
                    .subscribers
                    .get(&info_hash)
                    .map(|subscribers| {
                        subscribers
                            .iter()
                            .map(|(&subscriber_id, subscriber_tx)| DemandSubscriberDelivery {
                                subscriber_id,
                                subscriber_tx: subscriber_tx.clone(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                DemandSubscriberReduction {
                    subscriber_id: None,
                    effects: (!deliveries.is_empty())
                        .then_some(DemandSubscriberEffect::DeliverPeers {
                            info_hash,
                            peers,
                            deliveries,
                        })
                        .into_iter()
                        .collect(),
                }
            }
            DemandSubscriberAction::PruneDeadSubscribers {
                info_hash,
                subscriber_ids,
            } => {
                let removed = subscriber_ids
                    .into_iter()
                    .filter(|&subscriber_id| self.remove_subscriber(info_hash, subscriber_id))
                    .count();
                DemandSubscriberReduction {
                    subscriber_id: None,
                    effects: (0..removed)
                        .map(|_| DemandSubscriberEffect::SubscriberRemoved { info_hash })
                        .collect(),
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn subscriber_count(&self, info_hash: InfoHash) -> usize {
        self.subscribers.get(&info_hash).map_or(0, HashMap::len)
    }

    fn remove_subscriber(&mut self, info_hash: InfoHash, subscriber_id: u64) -> bool {
        let Some(subscribers) = self.subscribers.get_mut(&info_hash) else {
            return false;
        };
        let removed = subscribers.remove(&subscriber_id).is_some();
        if subscribers.is_empty() {
            self.subscribers.remove(&info_hash);
        }
        removed
    }
}
