// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::time::Instant;

use super::{
    DemandPlannerModel, DemandSliceMetrics, DemandSubscriberRegistry, DhtServiceConfig,
    RecentUniquePeers, DHT_UNIQUE_PEERS_FOUND_WINDOW,
};

#[derive(Debug)]
pub(super) enum DhtServiceAction {
    ReconfigureSucceeded {
        config: DhtServiceConfig,
        warning: Option<String>,
    },
    ReconfigureFailed {
        warning: String,
    },
    RuntimeWarning {
        warning: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DhtServiceEffect {
    ResetDemandPlanner,
    PublishStatus,
    StartDueDemands,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DhtServiceReduction {
    pub(super) effects: Vec<DhtServiceEffect>,
}

#[derive(Debug)]
pub(super) struct DhtServiceModel {
    config: DhtServiceConfig,
    generation: u64,
    warning: Option<String>,
}

impl DhtServiceModel {
    pub(super) fn new(config: DhtServiceConfig, generation: u64, warning: Option<String>) -> Self {
        Self {
            config,
            generation,
            warning,
        }
    }

    pub(super) fn config(&self) -> &DhtServiceConfig {
        &self.config
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn warning_owned(&self) -> Option<String> {
        self.warning.clone()
    }

    pub(super) fn update(&mut self, action: DhtServiceAction) -> DhtServiceReduction {
        match action {
            DhtServiceAction::ReconfigureSucceeded { config, warning } => {
                self.config = config;
                self.generation = self.generation.saturating_add(1);
                self.warning = warning;
                DhtServiceReduction {
                    effects: vec![
                        DhtServiceEffect::ResetDemandPlanner,
                        DhtServiceEffect::PublishStatus,
                        DhtServiceEffect::StartDueDemands,
                    ],
                }
            }
            DhtServiceAction::ReconfigureFailed { warning } => {
                self.warning = Some(warning);
                DhtServiceReduction {
                    effects: vec![
                        DhtServiceEffect::ResetDemandPlanner,
                        DhtServiceEffect::PublishStatus,
                        DhtServiceEffect::StartDueDemands,
                    ],
                }
            }
            DhtServiceAction::RuntimeWarning { warning } => {
                self.warning = Some(warning);
                DhtServiceReduction {
                    effects: vec![DhtServiceEffect::PublishStatus],
                }
            }
        }
    }
}

pub(super) struct DhtServiceState {
    pub(super) service: DhtServiceModel,
    pub(super) demand_planner: DemandPlannerModel,
    pub(super) demand_subscribers: DemandSubscriberRegistry,
    pub(super) slice_metrics: DemandSliceMetrics,
    pub(super) recent_unique_peers: RecentUniquePeers,
}

impl DhtServiceState {
    pub(super) fn new(config: DhtServiceConfig, generation: u64, warning: Option<String>) -> Self {
        Self {
            service: DhtServiceModel::new(config, generation, warning),
            demand_planner: DemandPlannerModel::new(Instant::now()),
            demand_subscribers: DemandSubscriberRegistry::new(),
            slice_metrics: DemandSliceMetrics::default(),
            recent_unique_peers: RecentUniquePeers::new(DHT_UNIQUE_PEERS_FOUND_WINDOW),
        }
    }

    pub(super) fn has_draining_demands(&self) -> bool {
        self.demand_planner.has_draining_demands()
    }

    pub(super) fn record_recent_peers(&mut self, peers: &[SocketAddr]) {
        self.recent_unique_peers.record_batch(Instant::now(), peers);
    }

    pub(super) fn expire_recent_peers(&mut self) {
        let _ = self.recent_unique_peers.unique_count(Instant::now());
    }
}
