// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{mpsc, oneshot};

use super::{
    AddressFamily, DemandSliceClass, DemandSliceStopReason, InfoHash, LookupId, StartedLookup,
};

pub(super) struct DhtRuntimeLookupFamilyRequest {
    pub(super) info_hash: InfoHash,
    pub(super) family: AddressFamily,
    pub(super) slice_class: DemandSliceClass,
    pub(super) record_metrics: bool,
    pub(super) merged_tx: mpsc::UnboundedSender<Vec<SocketAddr>>,
    pub(super) lookup_ids: Arc<StdMutex<Vec<LookupId>>>,
    pub(super) first_batch_seen: Arc<AtomicBool>,
    pub(super) accepting_families: Arc<AtomicBool>,
}

pub(super) enum DhtRuntimeCommandAction {
    StartGetPeers {
        info_hash: InfoHash,
        response_tx: oneshot::Sender<Result<StartedLookup, String>>,
    },
    StartGetPeersFamily(DhtRuntimeLookupFamilyRequest),
    CancelLookups {
        lookup_ids: Vec<LookupId>,
    },
    ParkDemandLookups {
        info_hash: InfoHash,
        slice_class: DemandSliceClass,
        stop_reason: DemandSliceStopReason,
        total_peers: usize,
        unique_peers: HashSet<SocketAddr>,
        lookup_ids: Arc<StdMutex<Vec<LookupId>>>,
    },
    FinalizeDrainedDemandLookups {
        info_hash: InfoHash,
    },
    AnnouncePeer {
        info_hash: InfoHash,
        port: Option<u16>,
        response_tx: oneshot::Sender<bool>,
    },
}

pub(super) enum DhtRuntimeCommandEffect {
    StartGetPeers {
        info_hash: InfoHash,
        response_tx: oneshot::Sender<Result<StartedLookup, String>>,
    },
    AttachLookupFamily(DhtRuntimeLookupFamilyRequest),
    CancelLookups {
        lookup_ids: Vec<LookupId>,
    },
    ParkDemandLookups {
        info_hash: InfoHash,
        slice_class: DemandSliceClass,
        stop_reason: DemandSliceStopReason,
        total_peers: usize,
        unique_peers: HashSet<SocketAddr>,
        lookup_ids: Arc<StdMutex<Vec<LookupId>>>,
    },
    FinalizeDrainedDemandLookups {
        info_hash: InfoHash,
    },
    AnnouncePeer {
        info_hash: InfoHash,
        port: Option<u16>,
        response_tx: oneshot::Sender<bool>,
    },
    StartDueDemands,
}

#[derive(Default)]
pub(super) struct DhtRuntimeCommandReduction {
    pub(super) effects: Vec<DhtRuntimeCommandEffect>,
}

pub(super) struct DhtRuntimeCommandModel;

impl DhtRuntimeCommandModel {
    pub(super) fn update(action: DhtRuntimeCommandAction) -> DhtRuntimeCommandReduction {
        let effects = match action {
            DhtRuntimeCommandAction::StartGetPeers {
                info_hash,
                response_tx,
            } => {
                vec![DhtRuntimeCommandEffect::StartGetPeers {
                    info_hash,
                    response_tx,
                }]
            }
            DhtRuntimeCommandAction::StartGetPeersFamily(request) => {
                vec![DhtRuntimeCommandEffect::AttachLookupFamily(request)]
            }
            DhtRuntimeCommandAction::CancelLookups { lookup_ids } => {
                vec![DhtRuntimeCommandEffect::CancelLookups { lookup_ids }]
            }
            DhtRuntimeCommandAction::ParkDemandLookups {
                info_hash,
                slice_class,
                stop_reason,
                total_peers,
                unique_peers,
                lookup_ids,
            } => {
                vec![
                    DhtRuntimeCommandEffect::ParkDemandLookups {
                        info_hash,
                        slice_class,
                        stop_reason,
                        total_peers,
                        unique_peers,
                        lookup_ids,
                    },
                    DhtRuntimeCommandEffect::StartDueDemands,
                ]
            }
            DhtRuntimeCommandAction::FinalizeDrainedDemandLookups { info_hash } => {
                vec![
                    DhtRuntimeCommandEffect::FinalizeDrainedDemandLookups { info_hash },
                    DhtRuntimeCommandEffect::StartDueDemands,
                ]
            }
            DhtRuntimeCommandAction::AnnouncePeer {
                info_hash,
                port,
                response_tx,
            } => {
                vec![DhtRuntimeCommandEffect::AnnouncePeer {
                    info_hash,
                    port,
                    response_tx,
                }]
            }
        };
        DhtRuntimeCommandReduction { effects }
    }
}
