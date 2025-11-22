// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::TorrentCommand;
use crate::networking::BlockInfo;
use crate::torrent_manager::ManagerEvent;

use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc::Sender;
use tokio::sync::Semaphore;

use std::mem::Discriminant;
use std::sync::Arc;

use crate::torrent_file::Torrent;
use crate::torrent_manager::piece_manager::PieceManager;
use crate::torrent_manager::piece_manager::PieceStatus;
use std::collections::{HashMap, HashSet};

use rand::prelude::IndexedRandom;

const MAX_TIMEOUT_COUNT: u32 = 10;
const BITS_PER_BYTE: u64 = 8;
const SMOOTHING_PERIOD_MS: f64 = 5000.0;
const PEER_UPLOAD_IN_FLIGHT_LIMIT: usize = 4;
const MAX_BLOCK_SIZE: u32 = 131_072;

pub type PeerAddr = (String, u16);

#[derive(Debug, Clone)]
pub enum Action {
    Tick {
        dt_ms: u64,
    },
    RecalculateChokes {
        upload_slots: usize,
        random_seed: u64,
    },
    CheckCompletion,
    AssignWork {
        peer_id: String,
    },
    PeerSuccessfullyConnected {
        peer_id: String,
    },
    PeerDisconnected {
        peer_id: String,
    },
    UpdatePeerId {
        peer_addr: String,
        new_id: Vec<u8>,
    },
    PeerBitfieldReceived {
        peer_id: String,
        bitfield: Vec<u8>,
    },
    PeerChoked {
        peer_id: String,
    },
    PeerUnchoked {
        peer_id: String,
    },
    PeerInterested {
        peer_id: String,
    },
    PeerHavePiece {
        peer_id: String,
        piece_index: u32,
    },
    IncomingBlock {
        peer_id: String,
        piece_index: u32,
        block_offset: u32,
        data: Vec<u8>,
    },
    PieceVerified {
        peer_id: String,
        piece_index: u32,
        valid: bool,
        data: Vec<u8>,
    },
    PieceWrittenToDisk {
        peer_id: String,
        piece_index: u32,
    },
    PieceWriteFailed {
        piece_index: u32,
    },
    RequestUpload {
        peer_id: String,
        piece_index: u32,
        block_offset: u32,
        length: u32,
    },
    TrackerResponse {
        url: String,
        peers: Vec<PeerAddr>,
        interval: u64,
        min_interval: Option<u64>,
    },
    TrackerError {
        url: String,
    },
    PeerConnectionFailed {
        peer_addr: String,
    },
    MetadataReceived {
        torrent: Box<Torrent>,
        metadata_length: i64,
    },
    ValidationComplete {
        completed_pieces: Vec<u32>,
    },

    BlockSentToPeer {
        peer_id: String,
        byte_count: u64,
    },

    CancelUpload {
        peer_id: String,
        piece_index: u32,
        block_offset: u32,
        length: u32,
    },

    Cleanup,
    Pause,
    Resume,
    Delete,
    UpdateListenPort,
    ValidationProgress {
        count: u32,
    },

    FatalError,
}

#[derive(Debug)]
#[must_use]
pub enum Effect {
    DoNothing,
    EmitMetrics {
        bytes_dl: u64,
        bytes_ul: u64,
    },
    EmitManagerEvent(ManagerEvent),
    SendToPeer {
        peer_id: String,
        cmd: Box<TorrentCommand>,
    },
    DisconnectPeer {
        peer_id: String,
    },
    AnnounceCompleted {
        url: String,
    },

    // --- New I/O & Work Effects ---
    VerifyPiece {
        peer_id: String,
        piece_index: u32,
        data: Vec<u8>,
    },
    WriteToDisk {
        peer_id: String,
        piece_index: u32,
        data: Vec<u8>,
    },
    ReadFromDisk {
        peer_id: String,
        block_info: BlockInfo,
    },
    BroadcastHave {
        piece_index: u32,
    },

    ConnectToPeer {
        ip: String,
        port: u16,
    },
    InitializeStorage,
    StartValidation,
    SendBitfieldToPeers,
    AnnounceToTracker {
        url: String,
    },

    ConnectToPeersFromTrackers,

    AbortUpload {
        peer_id: String,
        block_info: BlockInfo,
    },

    ClearAllUploads,
    DeleteFiles,
    TriggerDhtSearch,
}

#[derive(Debug)]
pub struct TrackerState {
    pub next_announce_time: Instant,
    pub leeching_interval: Option<Duration>,
    pub seeding_interval: Option<Duration>,
}

#[derive(Clone, Debug, Default)]
pub enum TorrentActivity {
    #[default]
    Initializing,
    Paused,
    ConnectingToPeers,
    DownloadingPiece(u32),
    SendingPiece(u32),
    VerifyingPiece(u32),
    AnnouncingToTracker,

    #[cfg(feature = "dht")]
    SearchingDht,
}

#[derive(PartialEq, Debug, Default)]
pub enum TorrentStatus {
    #[default]
    Validating,
    Standard,
    Endgame,
    Done,
}

#[derive(PartialEq, Debug)]
pub enum ChokeStatus {
    Choke,
    Unchoke,
}

#[derive(Debug)]
pub struct TorrentState {
    pub info_hash: Vec<u8>,
    pub torrent: Option<Torrent>,
    pub torrent_metadata_length: Option<i64>,
    pub is_paused: bool,
    pub torrent_status: TorrentStatus,
    pub torrent_validation_status: bool,
    pub last_activity: TorrentActivity,
    pub has_made_first_connection: bool,
    pub session_total_uploaded: u64,
    pub session_total_downloaded: u64,
    pub bytes_downloaded_in_interval: u64,
    pub bytes_uploaded_in_interval: u64,
    pub total_dl_prev_avg_ema: f64,
    pub total_ul_prev_avg_ema: f64,
    pub number_of_successfully_connected_peers: usize,
    pub peers: HashMap<String, PeerState>,
    pub piece_manager: PieceManager,
    pub trackers: HashMap<String, TrackerState>,
    pub timed_out_peers: HashMap<String, (u32, Instant)>,
    pub last_known_peers: HashSet<String>,
    pub optimistic_unchoke_timer: Option<Instant>,
    pub validation_pieces_found: u32,
    pub now: Instant,
}
impl Default for TorrentState {
    fn default() -> Self {
        Self {
            info_hash: Vec::new(),
            torrent: None,
            torrent_metadata_length: None,
            is_paused: false,
            torrent_status: TorrentStatus::default(),
            torrent_validation_status: false,
            last_activity: TorrentActivity::default(),
            has_made_first_connection: false,
            session_total_uploaded: 0,
            session_total_downloaded: 0,
            bytes_downloaded_in_interval: 0,
            bytes_uploaded_in_interval: 0,
            total_dl_prev_avg_ema: 0.0,
            total_ul_prev_avg_ema: 0.0,
            number_of_successfully_connected_peers: 0,
            peers: HashMap::new(),
            piece_manager: PieceManager::new(), 
            trackers: HashMap::new(),
            timed_out_peers: HashMap::new(),
            last_known_peers: HashSet::new(),
            optimistic_unchoke_timer: None,
            validation_pieces_found: 0,
            now: Instant::now(), // This is what fixes the error
        }
    }
}

impl TorrentState {
    pub fn new(
        info_hash: Vec<u8>,
        torrent: Option<Torrent>,
        torrent_metadata_length: Option<i64>,
        piece_manager: PieceManager,
        trackers: HashMap<String, TrackerState>,
        torrent_validation_status: bool,
    ) -> Self {
        Self {
            info_hash,
            torrent,
            torrent_metadata_length,
            piece_manager,
            trackers,
            torrent_validation_status,
            optimistic_unchoke_timer: Some(
                Instant::now()
                    .checked_sub(Duration::from_secs(31))
                    .unwrap_or(Instant::now()),
            ),
            now: Instant::now(),
            ..Default::default()
        }
    }

    // Helper to determine piece size based on torrent metadata
    fn get_piece_size(&self, piece_index: u32) -> usize {
        if let Some(torrent) = &self.torrent {
            let piece_len = torrent.info.piece_length as u64;
            let total_len: u64 = if !torrent.info.files.is_empty() {
                torrent.info.files.iter().map(|f| f.length as u64).sum()
            } else {
                torrent.info.length as u64
            };

            let offset = piece_index as u64 * piece_len;
            let remaining = total_len.saturating_sub(offset);
            std::cmp::min(piece_len, remaining) as usize
        } else {
            0
        }
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Tick { dt_ms } => {
                self.now += Duration::from_millis(dt_ms);
                let scaling_factor = if dt_ms > 0 {
                    1000.0 / dt_ms as f64
                } else {
                    1.0
                };
                let dt = dt_ms as f64;
                // Calculate Alpha for Exponential Moving Average
                let alpha = 1.0 - (-dt / SMOOTHING_PERIOD_MS).exp();

                // --- GLOBAL SPEED CALCULATION ---
                let inst_total_dl_speed =
                    (self.bytes_downloaded_in_interval as f64 * 8.0) * scaling_factor;
                let inst_total_ul_speed =
                    (self.bytes_uploaded_in_interval as f64 * 8.0) * scaling_factor;

                // Capture values for the EmitMetrics event
                let dl_tick = self.bytes_downloaded_in_interval;
                let ul_tick = self.bytes_uploaded_in_interval;

                // Reset interval counters
                self.bytes_downloaded_in_interval = 0;
                self.bytes_uploaded_in_interval = 0;

                // Update Global EMAs
                self.total_dl_prev_avg_ema =
                    (inst_total_dl_speed * alpha) + (self.total_dl_prev_avg_ema * (1.0 - alpha));
                self.total_ul_prev_avg_ema =
                    (inst_total_ul_speed * alpha) + (self.total_ul_prev_avg_ema * (1.0 - alpha));

                // --- PEER SPEED CALCULATION ---
                for peer in self.peers.values_mut() {
                    let inst_dl_speed =
                        (peer.bytes_downloaded_in_tick as f64 * 8.0) * scaling_factor;
                    let inst_ul_speed =
                        (peer.bytes_uploaded_in_tick as f64 * 8.0) * scaling_factor;

                    // Update Peer EMAs
                    peer.prev_avg_dl_ema =
                        (inst_dl_speed * alpha) + (peer.prev_avg_dl_ema * (1.0 - alpha));
                    peer.download_speed_bps = peer.prev_avg_dl_ema as u64;

                    peer.prev_avg_ul_ema =
                        (inst_ul_speed * alpha) + (peer.prev_avg_ul_ema * (1.0 - alpha));
                    peer.upload_speed_bps = peer.prev_avg_ul_ema as u64;

                    // Reset Peer tick counters
                    peer.bytes_downloaded_in_tick = 0;
                    peer.bytes_uploaded_in_tick = 0;
                }

                let mut effects = vec![Effect::EmitMetrics {
                    bytes_dl: dl_tick,
                    bytes_ul: ul_tick,
                }];

                if self.torrent_status == TorrentStatus::Validating || self.is_paused {
                    return effects;
                }

                // Tracker Announce Logic
                for (url, tracker) in self.trackers.iter_mut() {
                    if self.now >= tracker.next_announce_time {
                        self.last_activity = TorrentActivity::AnnouncingToTracker;
                        tracker.next_announce_time = self.now + Duration::from_secs(60);
                        effects.push(Effect::AnnounceToTracker { url: url.clone() });
                    }
                }

                effects
            }

            Action::RecalculateChokes { upload_slots, random_seed } => {
                let mut effects = Vec::new();

                let mut interested_peers: Vec<&mut PeerState> = self
                    .peers
                    .values_mut()
                    .filter(|p| p.peer_is_interested_in_us)
                    .collect();

                if self.torrent_status == TorrentStatus::Done {
                    interested_peers
                        .sort_by(|a, b| b.bytes_uploaded_to_peer.cmp(&a.bytes_uploaded_to_peer));
                } else {
                    interested_peers.sort_by(|a, b| {
                        b.bytes_downloaded_from_peer
                            .cmp(&a.bytes_downloaded_from_peer)
                    });
                }

                let mut unchoke_candidates: HashSet<String> = interested_peers
                    .iter()
                    .take(upload_slots)
                    .map(|p| p.ip_port.clone())
                    .collect();

                if self
                    .optimistic_unchoke_timer
                    .is_some_and(|t| self.now.saturating_duration_since(t) > Duration::from_secs(30))
                {
                    let optimistic_candidates: Vec<&mut PeerState> = interested_peers
                        .into_iter()
                        .filter(|p| !unchoke_candidates.contains(&p.ip_port))
                        .collect();

                    if !optimistic_candidates.is_empty() {
                        let idx = (random_seed as usize) % optimistic_candidates.len();
                        let chosen_id = optimistic_candidates[idx].ip_port.clone();
                        unchoke_candidates.insert(chosen_id);
                    }

                    self.optimistic_unchoke_timer = Some(self.now);
                }

                for peer in self.peers.values_mut() {
                    if unchoke_candidates.contains(&peer.ip_port) {
                        if peer.am_choking == ChokeStatus::Choke {
                            peer.am_choking = ChokeStatus::Unchoke;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: Box::new(TorrentCommand::PeerUnchoke),
                            });
                        }
                    } else if peer.am_choking == ChokeStatus::Unchoke {
                        peer.am_choking = ChokeStatus::Choke;
                        effects.push(Effect::SendToPeer {
                            peer_id: peer.ip_port.clone(),
                            cmd: Box::new(TorrentCommand::PeerChoke),
                        });
                    }

                    peer.bytes_downloaded_from_peer = 0;
                    peer.bytes_uploaded_to_peer = 0;
                }

                effects
            }

            Action::CheckCompletion => {
                if self.torrent_status == TorrentStatus::Done {
                    return vec![Effect::DoNothing];
                }

                let all_done = self
                    .piece_manager
                    .bitfield
                    .iter()
                    .all(|s| *s == PieceStatus::Done);

                if all_done {
                    let mut effects = Vec::new();
                    self.torrent_status = TorrentStatus::Done;

                    for (url, tracker) in self.trackers.iter_mut() {
                        tracker.next_announce_time = self.now;
                        effects.push(Effect::AnnounceCompleted { url: url.clone() });
                    }

                    for peer in self.peers.values_mut() {
                        if peer.am_interested {
                            peer.am_interested = false;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: Box::new(TorrentCommand::NotInterested),
                            });
                        }
                    }
                    return effects;
                }

                vec![Effect::DoNothing]
            }

            // --- Work Assignment ---
            Action::AssignWork { peer_id } => {
                if self.torrent_status == TorrentStatus::Validating {
                    return vec![Effect::DoNothing];
                }

                if self.piece_manager.need_queue.is_empty()
                    && self.piece_manager.pending_queue.is_empty()
                {
                    return vec![Effect::DoNothing];
                }

                let torrent = match &self.torrent {
                    Some(t) => t,
                    None => return vec![Effect::DoNothing],
                };

                let total_size: i64 = if !torrent.info.files.is_empty() {
                    torrent.info.files.iter().map(|f| f.length).sum()
                } else {
                    torrent.info.length
                };

                let mut effects = Vec::new();

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    if peer.bitfield.is_empty() || peer.peer_choking == ChokeStatus::Choke {
                        if peer.peer_choking == ChokeStatus::Choke
                            && !peer.am_interested
                            && self
                                .piece_manager
                                .need_queue
                                .iter()
                                .any(|&p| peer.bitfield.get(p as usize) == Some(&true))
                        {
                            peer.am_interested = true;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer_id.clone(),
                                cmd: Box::new(TorrentCommand::ClientInterested),
                            });
                        }
                        return effects;
                    }

                    let piece_to_assign = self.piece_manager.choose_piece_for_peer(
                        &peer.bitfield,
                        &peer.pending_requests,
                        &self.torrent_status,
                    );

                    if let Some(piece_index) = piece_to_assign {
                        peer.pending_requests.insert(piece_index);
                        self.piece_manager
                            .mark_as_pending(piece_index, peer_id.clone());

                        if self.piece_manager.need_queue.is_empty()
                            && self.torrent_status != TorrentStatus::Endgame
                        {
                            self.torrent_status = TorrentStatus::Endgame;
                        }

                        effects.push(Effect::SendToPeer {
                            peer_id: peer_id.clone(),
                            cmd: Box::new(TorrentCommand::RequestDownload(
                                piece_index,
                                torrent.info.piece_length,
                                total_size,
                            )),
                        });
                    }
                }
                effects
            }

            // --- Peer Lifecycle Actions ---
            Action::PeerSuccessfullyConnected { peer_id } => {
                self.timed_out_peers.remove(&peer_id);

                self.number_of_successfully_connected_peers = self.peers.len();

                if !self.has_made_first_connection {
                    self.has_made_first_connection = true;
                }

                vec![Effect::EmitManagerEvent(ManagerEvent::PeerConnected {
                    info_hash: self.info_hash.clone(),
                })]
            }

            Action::PeerDisconnected { peer_id } => {
                let mut effects = Vec::new();
                if let Some(removed_peer) = self.peers.remove(&peer_id) {
                    self.number_of_successfully_connected_peers = self.peers.len();

                    for piece_index in removed_peer.pending_requests {
                        if self.piece_manager.bitfield.get(piece_index as usize)
                            != Some(&PieceStatus::Done)
                        {
                            self.piece_manager.requeue_pending_to_need(piece_index);
                        }
                    }

                    effects.push(Effect::DisconnectPeer {
                        peer_id: peer_id.clone(),
                    });
                    effects.push(Effect::EmitManagerEvent(ManagerEvent::PeerDisconnected {
                        info_hash: self.info_hash.clone(),
                    }));
                }

                effects
            }

            Action::UpdatePeerId { peer_addr, new_id } => {
                if let Some(peer) = self.peers.get_mut(&peer_addr) {
                    peer.peer_id = new_id;
                }
                vec![Effect::DoNothing]
            }

            Action::PeerBitfieldReceived { peer_id, bitfield } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.bitfield = bitfield
                        .iter()
                        .flat_map(|&byte| (0..8).map(move |i| (byte >> (7 - i)) & 1 == 1))
                        .collect();

                    if let Some(torrent) = &self.torrent {
                        let total_pieces = torrent.info.pieces.len() / 20;
                        peer.bitfield.resize(total_pieces, false);
                    }
                }
                self.update(Action::AssignWork { peer_id })
            }

            Action::PeerChoked { peer_id } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.peer_choking = ChokeStatus::Choke;
                }
                vec![Effect::DoNothing]
            }

            Action::PeerUnchoked { peer_id } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.peer_choking = ChokeStatus::Unchoke;
                }
                self.update(Action::AssignWork { peer_id })
            }

            Action::PeerInterested { peer_id } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.peer_is_interested_in_us = true;
                }
                vec![Effect::DoNothing]
            }

            Action::PeerHavePiece {
                peer_id,
                piece_index,
            } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    if (piece_index as usize) < peer.bitfield.len() {
                        peer.bitfield[piece_index as usize] = true;
                    }
                }
                self.update(Action::AssignWork { peer_id })
            }

            // --- Data Flow (The Core Logic) ---
            Action::IncomingBlock {
                peer_id,
                piece_index,
                block_offset,
                data,
            } => {
                let mut effects = Vec::new();

                if self.piece_manager.bitfield.get(piece_index as usize) == Some(&PieceStatus::Done) {
                    return effects; 
                }

                if self.torrent_status == TorrentStatus::Validating {
                    return effects;
                }

                self.last_activity = TorrentActivity::DownloadingPiece(piece_index);

                let len = data.len() as u64;
                self.bytes_downloaded_in_interval = self.bytes_downloaded_in_interval.saturating_add(len);
                self.session_total_downloaded = self.session_total_downloaded.saturating_add(len);

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.bytes_downloaded_from_peer += len;
                    peer.bytes_downloaded_in_tick += len;
                    peer.total_bytes_downloaded += len;
                }

                effects.push(Effect::EmitManagerEvent(ManagerEvent::BlockReceived {
                    info_hash: self.info_hash.clone(),
                }));

                let piece_size = self.get_piece_size(piece_index);
                if let Some(complete_data) =
                    self.piece_manager
                        .handle_block(piece_index, block_offset, &data, piece_size)
                {
                    self.last_activity = TorrentActivity::VerifyingPiece(piece_index);
                    effects.push(Effect::VerifyPiece {
                        peer_id: peer_id.clone(),
                        piece_index,
                        data: complete_data,
                    });
                }

                effects
            }

            Action::PieceVerified {
                peer_id,
                piece_index,
                valid,
                data,
            } => {
                let mut effects = Vec::new();
                if valid {

                    if self.piece_manager.bitfield.get(piece_index as usize)
                        == Some(&PieceStatus::Done)
                    {
                        if let Some(peer) = self.peers.get_mut(&peer_id) {
                            peer.pending_requests.remove(&piece_index);
                        }
                        
                        // Redundant piece; we already have it. Discard data and assign new work.
                        effects.extend(self.update(Action::AssignWork { peer_id }));
                    } else {
                        // Valid and needed piece. Request write to disk.
                        // The data payload is now properly passed from the Action.
                        effects.push(Effect::WriteToDisk {
                            peer_id: peer_id.clone(),
                            piece_index,
                            data,
                        });
                    }
                } else {
                    self.piece_manager.reset_piece_assembly(piece_index);
                    effects.push(Effect::DisconnectPeer { peer_id });
                }
                effects
            }

            Action::PieceWrittenToDisk {
                peer_id,
                piece_index,
            } => {
                let mut effects = Vec::new();

                if self.piece_manager.bitfield.get(piece_index as usize) == Some(&PieceStatus::Done) {
                    if let Some(peer) = self.peers.get_mut(&peer_id) {
                        peer.pending_requests.remove(&piece_index);
                    }
                    effects.extend(self.update(Action::AssignWork { peer_id }));
                    return effects;
                }

                let peers_to_cancel = self.piece_manager.mark_as_complete(piece_index);

                effects.push(Effect::EmitManagerEvent(ManagerEvent::DiskWriteFinished));

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.pending_requests.remove(&piece_index);
                }

                effects.extend(self.update(Action::AssignWork {
                    peer_id: peer_id.clone(),
                }));

                for other_peer in peers_to_cancel {
                    if other_peer != peer_id {
                        if let Some(peer) = self.peers.get_mut(&other_peer) {
                            peer.pending_requests.remove(&piece_index);
                            effects.push(Effect::SendToPeer {
                                peer_id: other_peer.clone(),
                                cmd: Box::new(TorrentCommand::Cancel(piece_index)),
                            });
                        }
                        effects.extend(self.update(Action::AssignWork {
                            peer_id: other_peer,
                        }));
                    }
                }

                effects.push(Effect::BroadcastHave { piece_index });

                effects.extend(self.update(Action::CheckCompletion));

                effects
            }
            Action::PieceWriteFailed { piece_index } => {
                self.piece_manager.requeue_pending_to_need(piece_index);
                vec![Effect::EmitManagerEvent(ManagerEvent::DiskWriteFinished)]
            }

            Action::RequestUpload {
                peer_id,
                piece_index,
                block_offset,
                length,
            } => {
                if self.torrent.is_none() {
                    return vec![Effect::DoNothing];
                }

                if length > MAX_BLOCK_SIZE {
                    return vec![Effect::DoNothing];
                }

                self.last_activity = TorrentActivity::SendingPiece(piece_index);

                let mut allowed = false;
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    if peer.am_choking == ChokeStatus::Unchoke
                        && self.piece_manager.bitfield.get(piece_index as usize)
                            == Some(&PieceStatus::Done)
                    {
                        allowed = true;
                    }
                }

                if allowed {
                    vec![Effect::ReadFromDisk {
                        peer_id,
                        block_info: BlockInfo {
                            piece_index,
                            offset: block_offset,
                            length,
                        },
                    }]
                } else {
                    vec![Effect::DoNothing]
                }
            }

            Action::TrackerResponse {
                url,
                peers,
                interval,
                min_interval,
            } => {
                let mut effects = Vec::new();

                if let Some(tracker) = self.trackers.get_mut(&url) {
                    let seeding_secs = if interval > 0 { interval + 1 } else { 1800 };
                    tracker.seeding_interval = Some(Duration::from_secs(seeding_secs));

                    let leeching_secs = min_interval.map(|m| m + 1).unwrap_or(60);
                    tracker.leeching_interval = Some(Duration::from_secs(leeching_secs));

                    let next_interval = if self.torrent_status != TorrentStatus::Done {
                        tracker.leeching_interval.unwrap()
                    } else {
                        tracker.seeding_interval.unwrap()
                    };
                    tracker.next_announce_time = self.now + next_interval;
                }

                for (ip, port) in peers {
                    let peer_addr = format!("{}:{}", ip, port);
                    if let Some((_, next_attempt)) = self.timed_out_peers.get(&peer_addr) {
                        if self.now < *next_attempt {
                            continue;
                        }
                    }
                    effects.push(Effect::ConnectToPeer { ip, port });
                }

                effects
            }

            Action::TrackerError { url } => {
                if let Some(tracker) = self.trackers.get_mut(&url) {
                    let current_interval = if self.torrent_status != TorrentStatus::Done {
                        tracker.leeching_interval.unwrap_or(Duration::from_secs(60))
                    } else {
                        tracker
                            .seeding_interval
                            .unwrap_or(Duration::from_secs(1800))
                    };

                    let backoff = current_interval.mul_f32(2.0).min(Duration::from_secs(3600));
                    tracker.next_announce_time = self.now + backoff;
                }
                vec![Effect::DoNothing]
            }

            Action::PeerConnectionFailed { peer_addr } => {
                let (count, _) = self
                    .timed_out_peers
                    .get(&peer_addr)
                    .cloned()
                    .unwrap_or((0, self.now));
                let new_count = (count + 1).min(10);
                let backoff_secs = (15 * 2u64.pow(new_count - 1)).min(1800);
                let next_attempt = self.now + Duration::from_secs(backoff_secs);

                self.timed_out_peers
                    .insert(peer_addr, (new_count, next_attempt));
                vec![Effect::DoNothing]
            }

            Action::MetadataReceived {
                torrent,
                metadata_length,
            } => {
                if self.torrent.is_some() {
                    return vec![Effect::DoNothing];
                }

                self.torrent = Some(*torrent.clone());
                self.torrent_metadata_length = Some(metadata_length);

                let num_pieces = torrent.info.pieces.len() / 20;
                self.piece_manager
                    .set_initial_fields(num_pieces, self.torrent_validation_status);

                // Retroactive bitfield resize for non-compliant clients
                for peer in self.peers.values_mut() {
                    if peer.bitfield.len() > num_pieces {
                        peer.bitfield.truncate(num_pieces);
                    } else if peer.bitfield.len() < num_pieces {
                        peer.bitfield.resize(num_pieces, false);
                    }
                }

                if let Some(announce) = &torrent.announce {
                    self.trackers.insert(
                        announce.clone(),
                        TrackerState {
                            next_announce_time: self.now,
                            leeching_interval: None,
                            seeding_interval: None,
                        },
                    );
                }

                self.validation_pieces_found = 0;
                self.torrent_status = TorrentStatus::Validating;

                vec![Effect::InitializeStorage, Effect::StartValidation]
            }

            Action::ValidationComplete { completed_pieces } => {
                let mut effects = Vec::new();

                for piece_index in completed_pieces {
                    let peers_to_cancel = self.piece_manager.mark_as_complete(piece_index);
                    
                    for peer_id in peers_to_cancel {
                        if let Some(peer) = self.peers.get_mut(&peer_id) {
                            if peer.pending_requests.remove(&piece_index) {
                                effects.push(Effect::SendToPeer {
                                    peer_id: peer_id.clone(),
                                    cmd: Box::new(TorrentCommand::Cancel(piece_index)),
                                });
                            }
                        }
                        effects.extend(self.update(Action::AssignWork { peer_id }));
                    }
                }

                self.torrent_status = TorrentStatus::Standard;

                if !self.is_paused {
                    effects.push(Effect::SendBitfieldToPeers);
                    effects.push(Effect::ConnectToPeersFromTrackers);
                }
                effects.extend(self.update(Action::CheckCompletion));

                for peer_id in self.peers.keys().cloned().collect::<Vec<_>>() {
                    effects.extend(self.update(Action::AssignWork { peer_id }));
                }

                effects
            }

            Action::CancelUpload {
                peer_id,
                piece_index,
                block_offset,
                length,
            } => {
                vec![Effect::AbortUpload {
                    peer_id,
                    block_info: BlockInfo {
                        piece_index,
                        offset: block_offset,
                        length,
                    },
                }]
            }

            Action::BlockSentToPeer {
                peer_id,
                byte_count,
            } => {
                self.session_total_uploaded = self.session_total_uploaded.saturating_add(byte_count);
                self.bytes_uploaded_in_interval = self.bytes_uploaded_in_interval.saturating_add(byte_count);

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.bytes_uploaded_to_peer += byte_count;
                    peer.total_bytes_uploaded += byte_count;
                    peer.bytes_uploaded_in_tick += byte_count;
                }

                vec![Effect::EmitManagerEvent(ManagerEvent::BlockSent {
                    info_hash: self.info_hash.clone(),
                })]
            }

            Action::Cleanup => {
                let mut effects = Vec::new();

                self.timed_out_peers
                    .retain(|_, (retry_count, _)| *retry_count < MAX_TIMEOUT_COUNT);

                let mut stuck_peers = Vec::new();
                for (id, peer) in &self.peers {
                    if peer.peer_id.is_empty()
                        && self.now.saturating_duration_since(peer.created_at) > Duration::from_secs(5)
                    {
                        stuck_peers.push(id.clone());
                    }
                }

                for peer_id in stuck_peers {
                    effects.push(Effect::DisconnectPeer { peer_id });
                }

                let am_seeding = !self.piece_manager.bitfield.is_empty()
                    && self
                        .piece_manager
                        .bitfield
                        .iter()
                        .all(|&s| s == PieceStatus::Done);

                if am_seeding && self.torrent_status != TorrentStatus::Done {
                    self.torrent_status = TorrentStatus::Done;
                    effects.extend(self.update(Action::CheckCompletion));
                }

                if am_seeding {
                    let mut peers_to_disconnect = Vec::new();
                    for (peer_id, peer) in &self.peers {
                        let peer_is_seed = !peer.bitfield.is_empty()
                            && peer.bitfield.iter().all(|&has_piece| has_piece);
                        if peer_is_seed {
                            peers_to_disconnect.push(peer_id.clone());
                        }
                    }
                    for peer_id in peers_to_disconnect {
                        effects.push(Effect::DisconnectPeer { peer_id });
                    }
                }
                effects
            }

            Action::Pause => {
                self.last_activity = TorrentActivity::Paused;
                self.is_paused = true;

                self.last_known_peers = self.peers.keys().cloned().collect();
                
                for (piece_index, _) in self.piece_manager.pending_queue.drain() {
                    self.piece_manager.need_queue.push(piece_index);
                }


                self.peers.clear();
                
                self.number_of_successfully_connected_peers = 0;

                self.bytes_downloaded_in_interval = 0;
                self.bytes_uploaded_in_interval = 0;

                vec![
                    Effect::EmitMetrics {
                        bytes_dl: self.bytes_downloaded_in_interval,
                        bytes_ul: self.bytes_uploaded_in_interval,
                    },
                    Effect::ClearAllUploads,
                    Effect::EmitManagerEvent(ManagerEvent::PeerDisconnected {
                        info_hash: self.info_hash.clone(),
                    }),
                ]
            }

            Action::Resume => {
                self.last_activity = TorrentActivity::ConnectingToPeers;
                self.is_paused = false;

                if self.torrent_status == TorrentStatus::Validating {
                    return vec![Effect::DoNothing];
                }

                let mut effects = vec![Effect::TriggerDhtSearch];

                for (url, tracker) in self.trackers.iter_mut() {
                    tracker.next_announce_time = self.now + Duration::from_secs(60);
                    effects.push(Effect::AnnounceToTracker { url: url.clone() });
                }

                let peers_to_connect: Vec<String> = std::mem::take(&mut self.last_known_peers)
                    .into_iter()
                    .collect();
                for peer_addr in peers_to_connect {
                    if let Ok(std::net::SocketAddr::V4(v4)) =
                        peer_addr.parse::<std::net::SocketAddr>()
                    {
                        effects.push(Effect::ConnectToPeer {
                            ip: v4.ip().to_string(),
                            port: v4.port(),
                        });
                    }
                }

                effects
            }

            Action::Delete => {
                self.peers.clear();
                self.last_known_peers.clear();
                self.timed_out_peers.clear();

                let num_pieces = self.piece_manager.bitfield.len();
                self.piece_manager = PieceManager::new();
                if num_pieces > 0 {
                    self.piece_manager.set_initial_fields(num_pieces, false);
                }
                self.piece_manager.pending_queue.clear();
                self.piece_manager.need_queue.clear();
                
                for status in self.piece_manager.bitfield.iter_mut() {
                    *status = PieceStatus::Need; 
                }

                self.number_of_successfully_connected_peers = 0;
                
                self.session_total_downloaded = 0;
                self.session_total_uploaded = 0;

                // These must be cleared, otherwise they remain > 0 while total is 0
                self.bytes_downloaded_in_interval = 0;
                self.bytes_uploaded_in_interval = 0;
                // -----------------------------------------------

                self.is_paused = true;
                self.torrent_status = TorrentStatus::Validating;
                self.last_activity = TorrentActivity::Initializing;

                vec![Effect::DeleteFiles]
            }

            Action::UpdateListenPort => {
                let mut effects = Vec::new();

                for (url, tracker) in self.trackers.iter_mut() {
                    tracker.next_announce_time = self.now + Duration::from_secs(60);
                    effects.push(Effect::AnnounceToTracker { url: url.clone() });
                }

                effects
            }

            Action::ValidationProgress { count } => {
                self.validation_pieces_found = count;
                vec![Effect::DoNothing]
            }

            Action::FatalError => self.update(Action::Pause),
        }
    }
}

#[derive(Debug)]
pub struct PeerState {
    pub ip_port: String,
    pub peer_id: Vec<u8>,
    pub bitfield: Vec<bool>,
    pub am_choking: ChokeStatus,
    pub peer_choking: ChokeStatus,
    pub peer_tx: Sender<TorrentCommand>,
    pub am_interested: bool,
    pub pending_requests: HashSet<u32>,
    pub peer_is_interested_in_us: bool,
    pub bytes_downloaded_from_peer: u64,
    pub bytes_uploaded_to_peer: u64,
    pub bytes_downloaded_in_tick: u64,
    pub bytes_uploaded_in_tick: u64,
    pub prev_avg_dl_ema: f64,
    pub prev_avg_ul_ema: f64,
    pub total_bytes_downloaded: u64,
    pub total_bytes_uploaded: u64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub upload_slots_semaphore: Arc<Semaphore>,
    pub last_action: TorrentCommand,
    pub action_counts: HashMap<Discriminant<TorrentCommand>, u64>,
    pub created_at: Instant,
}

impl PeerState {
    pub fn new(ip_port: String, peer_tx: Sender<TorrentCommand>, created_at: Instant) -> Self {
        Self {
            ip_port,
            peer_id: Vec::new(),
            bitfield: Vec::new(),
            am_choking: ChokeStatus::Choke,
            peer_choking: ChokeStatus::Choke,
            peer_tx,
            am_interested: false,
            pending_requests: HashSet::new(),
            peer_is_interested_in_us: false,
            bytes_downloaded_from_peer: 0,
            bytes_uploaded_to_peer: 0,
            bytes_downloaded_in_tick: 0,
            bytes_uploaded_in_tick: 0,
            total_bytes_downloaded: 0,
            total_bytes_uploaded: 0,
            prev_avg_dl_ema: 0.0,
            prev_avg_ul_ema: 0.0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            upload_slots_semaphore: Arc::new(Semaphore::new(PEER_UPLOAD_IN_FLIGHT_LIMIT)),
            last_action: TorrentCommand::SuccessfullyConnected(String::new()),
            action_counts: HashMap::new(),
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::TorrentCommand;
    use crate::torrent_manager::piece_manager::PieceManager;
    use tokio::sync::mpsc;

    // --- Test Helpers ---

    pub(crate) fn create_empty_state() -> TorrentState {
        TorrentState {
            info_hash: vec![0; 20],
            peers: HashMap::new(),
            piece_manager: PieceManager::new(),
            trackers: HashMap::new(),
            ..Default::default()
        }
    }

    pub(crate) fn create_dummy_torrent(piece_count: usize) -> Torrent {
        // Construct a minimal Torrent struct for testing
        // Note: You might need to adjust this based on your actual Torrent struct visibility
        use crate::torrent_file::Info;

        Torrent {
            announce: Some("http://tracker.test".to_string()),
            announce_list: None,
            info: Info {
                name: "test_torrent".to_string(),
                piece_length: 16384,                 // 16KB
                pieces: vec![0u8; 20 * piece_count], // 20 bytes per piece hash
                length: (16384 * piece_count) as i64,
                files: vec![],
                private: None,
                md5sum: None,
            },
            info_dict_bencode: vec![],
            created_by: None,
            creation_date: None,
            encoding: None,
            comment: None,
        }
    }

    fn add_peer(state: &mut TorrentState, id: &str) {
        let (tx, _) = mpsc::channel(1);
        let mut peer = PeerState::new(id.to_string(), tx, state.now);
        // Assume peer has handshake
        peer.peer_id = id.as_bytes().to_vec();
        state.peers.insert(id.to_string(), peer);
    }

    // --- SCENARIO 1: Initialization ---

    #[test]
    fn test_metadata_received_triggers_initialization_flow() {
        // GIVEN: An empty state
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(5); // 5 pieces

        // WHEN: Metadata is received
        let action = Action::MetadataReceived {
            torrent: Box::new(torrent),
            metadata_length: 123,
        };
        let effects = state.update(action);

        // THEN: It should transition to Validating and trigger storage init + validation
        assert_eq!(state.torrent_status, TorrentStatus::Validating);
        assert!(state.torrent.is_some());

        // Verify specific order of effects
        assert!(matches!(effects[0], Effect::InitializeStorage));
        assert!(matches!(effects[1], Effect::StartValidation));
    }

    // --- SCENARIO 2: Choking Logic (Leeching) ---

    #[test]
    fn test_recalculate_chokes_unchokes_fastest_downloader() {
        // GIVEN: A state with 2 interested peers
        let mut state = create_empty_state();
        state.torrent_status = TorrentStatus::Standard; // Leeching

        add_peer(&mut state, "fast_peer");
        add_peer(&mut state, "slow_peer");

        // Setup Fast Peer: High download rate, interested in us
        let fast_peer = state.peers.get_mut("fast_peer").unwrap();
        fast_peer.peer_is_interested_in_us = true;
        fast_peer.bytes_downloaded_from_peer = 10_000; // 10KB

        // Setup Slow Peer: Low download rate, interested in us
        let slow_peer = state.peers.get_mut("slow_peer").unwrap();
        slow_peer.peer_is_interested_in_us = true;
        slow_peer.bytes_downloaded_from_peer = 100; // 100 bytes
        slow_peer.am_choking = ChokeStatus::Unchoke;

        // WHEN: We recalculate chokes with only 1 upload slot
        let effects = state.update(Action::RecalculateChokes { upload_slots: 1, random_seed: 0 });

        // THEN: Fast peer is Unchoked, Slow peer is Choked
        let fast_peer_state = state.peers.get("fast_peer").unwrap();
        let slow_peer_state = state.peers.get("slow_peer").unwrap();

        assert_eq!(fast_peer_state.am_choking, ChokeStatus::Unchoke);
        assert_eq!(slow_peer_state.am_choking, ChokeStatus::Choke);

        // Check emitted effects contain the commands
        let sent_unchoke = effects.iter().any(|e| {
            matches!(e, Effect::SendToPeer { peer_id, cmd }
            if peer_id == "fast_peer" && matches!(**cmd, TorrentCommand::PeerUnchoke))
        });

        let sent_choke = effects.iter().any(|e| {
            matches!(e, Effect::SendToPeer { peer_id, cmd }
            if peer_id == "slow_peer" && matches!(**cmd, TorrentCommand::PeerChoke))
        });

        assert!(sent_unchoke, "Should send Unchoke to fast peer");
        assert!(sent_choke, "Should send Choke to slow peer");
    }

    // --- SCENARIO 3: Choking Logic (Seeding) ---

    #[test]
    fn test_recalculate_chokes_unchokes_fastest_uploader_when_seeding() {
        // GIVEN: A state that is DONE (Seeding)
        let mut state = create_empty_state();
        state.torrent_status = TorrentStatus::Done;

        add_peer(&mut state, "leecher_A"); // Fast uptake
        add_peer(&mut state, "leecher_B"); // Slow uptake

        let p1 = state.peers.get_mut("leecher_A").unwrap();
        p1.peer_is_interested_in_us = true;
        p1.bytes_uploaded_to_peer = 50_000; // We sent them 50KB

        let p2 = state.peers.get_mut("leecher_B").unwrap();
        p2.peer_is_interested_in_us = true;
        p2.bytes_uploaded_to_peer = 1_000; // We sent them 1KB

        // WHEN: Recalculate with 1 slot
        state.update(Action::RecalculateChokes { upload_slots: 1, random_seed: 0 });

        // THEN: The peer we uploaded MORE to (higher throughput) gets the slot
        // Note: This assumes the logic sorts by bytes_uploaded_to_peer descending for Done status
        assert_eq!(state.peers["leecher_A"].am_choking, ChokeStatus::Unchoke);
        assert_eq!(state.peers["leecher_B"].am_choking, ChokeStatus::Choke);
    }

    // --- SCENARIO 4: Work Assignment ---

    #[test]
    fn test_assign_work_requests_piece_peer_has() {
        // GIVEN: Initialized state, Peer A has piece #0
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(10);
        state.piece_manager.set_initial_fields(10, false);
        state.torrent = Some(torrent);
        state.torrent_status = TorrentStatus::Standard;

        add_peer(&mut state, "peer_A");

        // Setup Peer A: Unchoked us, has Piece 0
        let peer = state.peers.get_mut("peer_A").unwrap();
        peer.peer_choking = ChokeStatus::Unchoke;
        peer.bitfield = vec![false; 10];
        peer.bitfield[0] = true; // Peer has piece 0

        // Setup Manager: We need piece 0
        state.piece_manager.need_queue.push(0);

        // WHEN: We assign work
        let effects = state.update(Action::AssignWork {
            peer_id: "peer_A".to_string(),
        });

        // THEN: We should see a RequestDownload effect for piece 0
        let request = effects.iter().find(|e| {
            matches!(e, Effect::SendToPeer { cmd, .. }
            if matches!(**cmd, TorrentCommand::RequestDownload(0, _, _)))
        });

        assert!(request.is_some(), "Should request piece 0 from peer_A");

        // And piece 0 should be in pending requests
        assert!(state.peers["peer_A"].pending_requests.contains(&0));
    }

    // --- SCENARIO 5: Piece Verification Success ---

    #[test]
    fn test_piece_verified_valid_trigger_write() {
        // GIVEN: State waiting for verification of piece 1
        let mut state = create_empty_state();
        state.piece_manager.set_initial_fields(5, false);
        // Mark piece 1 as needed/pending in piece manager context
        // (Assuming default state allows this transition)

        let data = vec![1, 2, 3, 4];

        // WHEN: Piece 1 is verified successfully
        let effects = state.update(Action::PieceVerified {
            peer_id: "peer_1".into(),
            piece_index: 1,
            valid: true,
            data: data.clone(),
        });

        // THEN: Effect::WriteToDisk is emitted
        let write_effect = effects
            .iter()
            .find(|e| matches!(e, Effect::WriteToDisk { piece_index: 1, .. }));
        assert!(write_effect.is_some());
    }

    #[test]
    fn test_piece_verified_invalid_disconnects_peer() {
        // GIVEN: State
        let mut state = create_empty_state();
        state.piece_manager.set_initial_fields(5, false);

        // WHEN: Piece 1 fails verification
        let effects = state.update(Action::PieceVerified {
            peer_id: "bad_peer".into(),
            piece_index: 1,
            valid: false,
            data: vec![],
        });

        // THEN: Peer is disconnected
        let disconnect = effects
            .iter()
            .any(|e| matches!(e, Effect::DisconnectPeer { peer_id } if peer_id == "bad_peer"));
        assert!(disconnect);
    }

    // --- SCENARIO 6: Completion ---

    #[test]
    fn test_check_completion_transitions_to_done() {
        // GIVEN: All pieces are marked as Done
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(3);
        state.torrent = Some(torrent);
        state.piece_manager.set_initial_fields(3, false);
        state.trackers.insert(
            "http://tracker".into(),
            TrackerState {
                next_announce_time: Instant::now(),
                leeching_interval: None,
                seeding_interval: None,
            },
        );

        // Manually mark all pieces as Done (simulating write success)
        for i in 0..3 {
            state.piece_manager.bitfield[i] = PieceStatus::Done;
        }

        // WHEN: CheckCompletion is called
        let effects = state.update(Action::CheckCompletion);

        // THEN: Status becomes Done, AnnounceCompleted emitted
        assert_eq!(state.torrent_status, TorrentStatus::Done);

        let announce_completed = effects
            .iter()
            .any(|e| matches!(e, Effect::AnnounceCompleted { .. }));
        assert!(announce_completed);
    }

    // --- SCENARIO 7: Cleanup / Disconnect ---

    #[test]
    fn test_peer_disconnect_decrements_count() {
        // GIVEN: A connected peer
        let mut state = create_empty_state();
        add_peer(&mut state, "peer_X");
        state.number_of_successfully_connected_peers = 1;

        // WHEN: Peer disconnects
        let effects = state.update(Action::PeerDisconnected {
            peer_id: "peer_X".to_string(),
        });

        // THEN: Peer removed, count decremented, Disconnect effect emitted
        assert!(!state.peers.contains_key("peer_X"));
        assert_eq!(state.number_of_successfully_connected_peers, 0);

        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::DisconnectPeer { .. })));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::EmitManagerEvent(ManagerEvent::PeerDisconnected { .. })
        )));
    }

    #[test]
    fn test_enter_endgame_mode() {
        // GIVEN: A torrent with 2 pieces, 1 already pending
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(2);
        state.torrent = Some(torrent);
        state.piece_manager.set_initial_fields(2, false);
        state.torrent_status = TorrentStatus::Standard;

        add_peer(&mut state, "peer_A");
        let peer = state.peers.get_mut("peer_A").unwrap();
        peer.bitfield = vec![true, true];
        peer.peer_choking = ChokeStatus::Unchoke;

        // Piece 0 is already pending (assigned to someone else, theoretically)
        state.piece_manager.mark_as_pending(0, "other_peer".into());

        // Only Piece 1 is left in need_queue
        state.piece_manager.need_queue.clear();
        state.piece_manager.need_queue.push(1);

        // WHEN: We assign the LAST needed piece to peer_A
        state.update(Action::AssignWork {
            peer_id: "peer_A".into(),
        });

        // THEN:
        // 1. Need queue should be empty
        assert!(state.piece_manager.need_queue.is_empty());
        // 2. Status should transition to ENDGAME
        assert_eq!(state.torrent_status, TorrentStatus::Endgame);
    }

    #[test]
    fn test_peer_chokes_us_mid_download() {
        // GIVEN: Peer A is unchoked and we have pending requests
        let mut state = create_empty_state();
        add_peer(&mut state, "peer_A");
        let peer = state.peers.get_mut("peer_A").unwrap();
        peer.peer_choking = ChokeStatus::Unchoke;
        peer.pending_requests.insert(5); // We asked for piece 5

        // WHEN: Peer A chokes us
        let effects = state.update(Action::PeerChoked {
            peer_id: "peer_A".into(),
        });

        // THEN:
        // 1. Internal state must update
        assert_eq!(state.peers["peer_A"].peer_choking, ChokeStatus::Choke);
        // 2. We might optionally clear pending requests depending on your strategy
        // (Strict clients cancel immediately; lenient ones wait. Verify YOUR logic here.)
    }

    #[test]
    fn test_optimistic_unchoke_rotates() {
        // GIVEN: 3 peers, 1 slot. Peer A is fast (regular unchoke). B & C are slow.
        let mut state = create_empty_state();
        add_peer(&mut state, "fast_A");
        add_peer(&mut state, "slow_B");
        add_peer(&mut state, "slow_C");

        for p in state.peers.values_mut() {
            p.peer_is_interested_in_us = true;
        }
        state
            .peers
            .get_mut("fast_A")
            .unwrap()
            .bytes_downloaded_from_peer = 1000;

        // Force timer expiration
        state.optimistic_unchoke_timer = Some(Instant::now() - Duration::from_secs(31));

        // WHEN: Recalculate
        let effects = state.update(Action::RecalculateChokes { upload_slots: 1, random_seed: 0 });

        // THEN:
        // 1. Fast A should be unchoked (Regular slot)
        // 2. EITHER B or C should be unchoked (Optimistic slot)
        // 3. Total unchoked count should be 2
        let unchoked_count = state
            .peers
            .values()
            .filter(|p| p.am_choking == ChokeStatus::Unchoke)
            .count();
        assert_eq!(unchoked_count, 2);

        assert_eq!(state.peers["fast_A"].am_choking, ChokeStatus::Unchoke);
        assert!(
            state.peers["slow_B"].am_choking == ChokeStatus::Unchoke
                || state.peers["slow_C"].am_choking == ChokeStatus::Unchoke
        );
    }

    #[test]
    fn test_peer_have_updates_bitfield_and_triggers_work() {
        // GIVEN: Peer A connected with empty bitfield
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(10);
        state.torrent = Some(torrent);
        state.piece_manager.set_initial_fields(10, false);

        state.torrent_status = TorrentStatus::Standard;

        add_peer(&mut state, "peer_A");
        state.peers.get_mut("peer_A").unwrap().bitfield = vec![false; 10];

        // We need piece 5
        // Note: If need_queue is a VecDeque, use .push_back(5) instead of .push(5)
        state.piece_manager.need_queue.push(5);

        // WHEN: Peer sends "Have(5)"
        let effects = state.update(Action::PeerHavePiece {
            peer_id: "peer_A".into(),
            piece_index: 5,
        });

        // THEN:
        // 1. Bitfield updated
        assert!(state.peers["peer_A"].bitfield[5]);

        // 2. Triggers Interest
        let interest = effects.iter().any(|e| {
            matches!(e, Effect::SendToPeer { cmd, .. } 
        if matches!(**cmd, TorrentCommand::ClientInterested))
        });

        assert!(interest, "Should send Interested message");
    }

    #[test]
    fn test_cancel_upload_aborts_task() {
        // GIVEN: We are seeding
        let mut state = create_empty_state();
        add_peer(&mut state, "leecher");

        // WHEN: Peer cancels request for piece 0, block 0
        let effects = state.update(Action::CancelUpload {
            peer_id: "leecher".into(),
            piece_index: 0,
            block_offset: 0,
            length: 16384,
        });

        // THEN: Effect::AbortUpload is emitted
        let abort = effects.iter().any(|e| {
            matches!(e, Effect::AbortUpload { peer_id, block_info }
        if peer_id == "leecher" && block_info.piece_index == 0)
        });

        assert!(abort);
    }

    #[test]
    fn test_invariant_pending_removed_on_disk_write() {
        // 1. Setup State
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(20);
        state.torrent = Some(torrent);
        state.piece_manager.set_initial_fields(20, false);
        state.torrent_status = TorrentStatus::Standard;

        // 2. Setup Peer and Need Queue
        add_peer(&mut state, "peer_A");
        let peer = state.peers.get_mut("peer_A").unwrap();
        peer.bitfield = vec![true; 20]; // Peer has everything
        peer.peer_choking = ChokeStatus::Unchoke;
        
        // We need piece 0
        state.piece_manager.need_queue.push(0); 

        // 3. Trigger Work Assignment
        // This moves Piece 0 from Need -> Pending and adds to Peer's pending_requests
        state.update(Action::AssignWork { peer_id: "peer_A".into() });

        // VERIFY SETUP: Piece 0 must be pending now
        assert!(state.peers["peer_A"].pending_requests.contains(&0), "Setup failed: Piece 0 should be pending");

        // 4. Simulate the Disk Write (The action containing your sabotage)
        state.update(Action::PieceWrittenToDisk { 
            peer_id: "peer_A".into(), 
            piece_index: 0 
        });

        // 5. ASSERTION
        // If the code is correct, piece 0 is removed from the peer.
        // If sabotaged, piece 0 remains, and this assert will panic.
        let is_still_pending = state.peers["peer_A"].pending_requests.contains(&0);
        
        assert!(!is_still_pending, 
            "INVARIANT VIOLATION: Piece 0 is marked DONE globally, but still exists in peer_A's pending_requests!");
            
        // Double check global status is actually done (to ensure test validity)
        assert_eq!(state.piece_manager.bitfield[0], PieceStatus::Done);
    }

    #[test]
    fn regression_delete_clears_piece_manager_state() {
        // BUG CONTEXT: Previously, Action::Delete cleared queues but left 'partial blocks' 
        // inside PieceManager. When a new peer connected and sent data for that piece, 
        // PieceManager panicked with "subtract with overflow" because it compared 
        // new offsets against old, stale buffer state.
        
        let mut state = create_empty_state();
        let torrent = create_dummy_torrent(5);
        state.torrent = Some(torrent);
        state.piece_manager.set_initial_fields(5, false);
        state.torrent_status = TorrentStatus::Standard;
        state.piece_manager.need_queue = vec![0];

        // 1. Connect Peer A and start downloading Piece 0
        add_peer(&mut state, "peer_A");
        let _ = state.update(Action::PeerUnchoked { peer_id: "peer_A".into() });
        let _ = state.update(Action::PeerHavePiece { peer_id: "peer_A".into(), piece_index: 0 });
        let _ = state.update(Action::AssignWork { peer_id: "peer_A".into() });

        // 2. Simulate partial download (polluting PieceManager internal buffer)
        let data = vec![1; 100];
        let _ = state.update(Action::IncomingBlock { 
            peer_id: "peer_A".into(), 
            piece_index: 0, 
            block_offset: 0, 
            data: data.clone() 
        });

        // 3. DELETE! (This must wipe PieceManager clean)
        let _ = state.update(Action::Delete);

        // 4. Connect Peer B and try downloading Piece 0 again
        // If state wasn't wiped, this causes "subtract with overflow" or "ghost queue" panic
        add_peer(&mut state, "peer_B");
        
        // We must reset status to Standard manually as Delete sets it to Validating
        state.torrent_status = TorrentStatus::Standard; 
        state.piece_manager.need_queue = vec![0];

        let _ = state.update(Action::PeerUnchoked { peer_id: "peer_B".into() });
        let _ = state.update(Action::PeerHavePiece { peer_id: "peer_B".into(), piece_index: 0 });
        
        // CRITICAL STEP: Sending data for the same piece index as before.
        // If the old partial buffer exists, this crashes.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut s = state; // Move state in
            s.update(Action::IncomingBlock { 
                peer_id: "peer_B".into(), 
                piece_index: 0, 
                block_offset: 0, 
                data: data 
            });
        }));

        assert!(result.is_ok(), "Regression: Action::Delete failed to wipe PieceManager state!");
    }

    #[test]
    fn regression_redundant_disk_write_completion() {
        // BUG CONTEXT: The fuzzer found that if 'PieceWrittenToDisk' fires twice 
        // (race condition), the PieceManager would panic trying to mark a 'Done' piece as done.
        
        let mut state = create_empty_state();
        state.piece_manager.set_initial_fields(1, false);
        add_peer(&mut state, "peer_A");
        state.peers.get_mut("peer_A").unwrap().pending_requests.insert(0);

        // 1. First Write Confirmation (Valid)
        state.update(Action::PieceWrittenToDisk { 
            peer_id: "peer_A".into(), 
            piece_index: 0 
        });
        
        assert_eq!(state.piece_manager.bitfield[0], PieceStatus::Done);

        // 2. Second Write Confirmation (The Bug Trigger)
        // Should be ignored gracefully, not panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut s = state;
            s.update(Action::PieceWrittenToDisk { 
                peer_id: "peer_A".into(), 
                piece_index: 0 
            });
        }));

        assert!(result.is_ok(), "Regression: Double PieceWrittenToDisk caused a panic!");
    }

    #[test]
    fn regression_metric_integer_overflow() {
        // BUG CONTEXT: Sending huge byte counts caused u64 overflow panics.
        let mut state = create_empty_state();
        add_peer(&mut state, "peer_A");

        let huge_val = u64::MAX - 100;
        
        // 1. Add huge value (should be fine)
        state.update(Action::BlockSentToPeer { 
            peer_id: "peer_A".into(), 
            byte_count: huge_val 
        });

        // 2. Add more (should saturate, not panic)
        state.update(Action::BlockSentToPeer { 
            peer_id: "peer_A".into(), 
            byte_count: 200 
        });

        assert_eq!(state.session_total_uploaded, u64::MAX);
        assert_eq!(state.peers["peer_A"].total_bytes_uploaded, u64::MAX);
    }

    #[test]
    fn regression_peer_count_sync() {
        // BUG CONTEXT: Connecting the same peer twice incremented the counter twice,
        // but the map only held 1 entry. Disconnecting then left the counter at 1.
        
        let mut state = create_empty_state();
        
        // 1. Connect Peer A
        state.update(Action::PeerSuccessfullyConnected { peer_id: "peer_A".into() });
        add_peer(&mut state, "peer_A"); // Inject the peer object as the runner would
        assert_eq!(state.number_of_successfully_connected_peers, 1);

        // 2. Connect Peer A AGAIN (Duplicate event)
        state.update(Action::PeerSuccessfullyConnected { peer_id: "peer_A".into() });
        assert_eq!(state.number_of_successfully_connected_peers, 1, "Counter should not increment on duplicate");

        // 3. Disconnect Peer A
        state.update(Action::PeerDisconnected { peer_id: "peer_A".into() });
        assert_eq!(state.number_of_successfully_connected_peers, 0, "Counter should be zero");
    }
}

// -----------------------------------------------------------------------------
// Property-Based Tests (Fuzzing Logic)
// -----------------------------------------------------------------------------
#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;
    use crate::torrent_file::Torrent;

    // =========================================================================
    // 1. STRATEGIES: Atomic Actions (The "Chaos" Monkey)
    // =========================================================================

    // A. Network & Tracker Events
    fn network_action_strategy() -> impl Strategy<Value = Action> {
        prop_oneof![
            any::<String>().prop_map(|id| Action::PeerSuccessfullyConnected { peer_id: id }),
            any::<String>().prop_map(|id| Action::PeerDisconnected { peer_id: id }),
            any::<String>().prop_map(|addr| Action::PeerConnectionFailed { peer_addr: addr }),
            (any::<String>(), proptest::collection::vec(any::<u8>(), 20))
                .prop_map(|(addr, id)| Action::UpdatePeerId { peer_addr: addr, new_id: id }),
            
            // Tracker responses (simulating periodic announces)
            (any::<String>(), any::<u64>()).prop_map(|(url, interval)| {
                Action::TrackerResponse { 
                    url, 
                    peers: vec![], // In chaos mode, we don't care about the payload much
                    interval, 
                    min_interval: Some(60) 
                }
            }),
            any::<String>().prop_map(|url| Action::TrackerError { url }),
            Just(Action::UpdateListenPort),
        ]
    }

    // B. Peer Protocol Messages
    fn protocol_action_strategy() -> impl Strategy<Value = Action> {
        let peer_id_strat = any::<String>(); 
        
        prop_oneof![
            peer_id_strat.clone().prop_map(|id| Action::PeerChoked { peer_id: id }),
            peer_id_strat.clone().prop_map(|id| Action::PeerUnchoked { peer_id: id }),
            peer_id_strat.clone().prop_map(|id| Action::PeerInterested { peer_id: id }),
            
            // Random Bitfields
            (peer_id_strat.clone(), proptest::collection::vec(any::<u8>(), 1..10))
                .prop_map(|(id, bf)| Action::PeerBitfieldReceived { peer_id: id, bitfield: bf }),
            // Random Have messages
            (peer_id_strat.clone(), any::<u32>())
                .prop_map(|(id, idx)| Action::PeerHavePiece { peer_id: id, piece_index: idx }),
                
            // Internal logic trigger
            peer_id_strat.clone().prop_map(|id| Action::AssignWork { peer_id: id }),
        ]
    }

    // C. Data Transfer (Upload/Download)
    fn data_action_strategy() -> impl Strategy<Value = Action> {
        let peer_id_strat = any::<String>();
        
        prop_oneof![
            // Incoming Blocks (Download)
            (peer_id_strat.clone(), 0..20u32, any::<u32>(), proptest::collection::vec(any::<u8>(), 1..1024))
                .prop_map(|(id, idx, off, data)| Action::IncomingBlock { 
                    peer_id: id, piece_index: idx, block_offset: off, data 
                }),
            
            // Outgoing Requests (Upload)
            (peer_id_strat.clone(), 0..20u32, any::<u32>(), 1..16384u32)
                .prop_map(|(id, idx, off, len)| Action::RequestUpload { 
                    peer_id: id, piece_index: idx, block_offset: off, length: len 
                }),
            (peer_id_strat.clone(), 0..20u32, any::<u32>(), 1..16384u32)
                .prop_map(|(id, idx, off, len)| Action::CancelUpload { 
                    peer_id: id, piece_index: idx, block_offset: off, length: len 
                }),
            // Confirmation of sent data
            (peer_id_strat.clone(), any::<u64>())
                .prop_map(|(id, count)| Action::BlockSentToPeer { peer_id: id, byte_count: count }),
        ]
    }

    // D. System / Disk Responses (The "Reactor" Inputs)
    fn system_response_strategy() -> impl Strategy<Value = Action> {
        prop_oneof![
            // Verification Results
            (any::<String>(), 0..20u32, any::<bool>())
                .prop_map(|(id, idx, valid)| Action::PieceVerified { 
                    peer_id: id, piece_index: idx, valid, data: vec![] 
                }),

            // Disk Write Completions
            (any::<String>(), 0..20u32).prop_map(|(id, idx)| Action::PieceWrittenToDisk { 
                peer_id: id, piece_index: idx 
            }),
            any::<u32>().prop_map(|idx| Action::PieceWriteFailed { piece_index: idx }),
            
            // Initialization
            any::<u32>().prop_map(|c| Action::ValidationProgress { count: c }),
            proptest::collection::vec(0..20u32, 0..5)
                .prop_map(|pieces| Action::ValidationComplete { completed_pieces: pieces }),
        ]
    }

    // E. Global Lifecycle
    fn lifecycle_strategy() -> impl Strategy<Value = Action> {
        prop_oneof![
            Just(Action::Tick { dt_ms: 100 }), 
            Just(Action::Tick { dt_ms: 50000 }), // Lag simulation
            Just(Action::CheckCompletion),
            Just(Action::Cleanup),
            Just(Action::Pause),
            Just(Action::Resume),
            Just(Action::Delete),
            Just(Action::FatalError),
            (0..50usize, any::<u64>()).prop_map(|(s, seed)| Action::RecalculateChokes { 
                upload_slots: s, 
                random_seed: seed 
            }),
            
            // Metadata Reset / Late Magnet Link Resolution
            ((1..20usize).prop_map(super::tests::create_dummy_torrent), any::<i64>())
                .prop_map(|(t, len)| Action::MetadataReceived { 
                    torrent: Box::new(t), metadata_length: len 
                }),
        ]
    }

    // F. Combined Chaos
    fn chaos_strategy() -> impl Strategy<Value = Action> {
        prop_oneof![
            network_action_strategy(),
            protocol_action_strategy(),
            data_action_strategy(),
            system_response_strategy(),
            lifecycle_strategy(),
        ]
    }

    // =========================================================================
    // 2. STRATEGIES: Complex Logical Stories
    // =========================================================================

    // 1. Connection Story
    fn connection_story_strategy() -> impl Strategy<Value = Vec<Action>> {
        (1..255u8, 1000..9999u16).prop_flat_map(|(ip, port)| {
            let peer_id = format!("127.0.0.{}:{}", ip, port);
            let actions = vec![
                Action::PeerSuccessfullyConnected { peer_id: peer_id.clone() },
                Action::PeerBitfieldReceived { 
                    peer_id: peer_id.clone(), 
                    bitfield: vec![255; 4] 
                },
                Action::PeerUnchoked { peer_id: peer_id.clone() },
            ];
            Just(actions)
        })
    }

    // 2. Download Story (With Disk Write Closure)
    fn successful_download_story() -> impl Strategy<Value = Vec<Action>> {
        let peer_gen = (1..255u8, 1000..9999u16);
        let piece_gen = 0..20u32;
        
        (peer_gen, piece_gen).prop_flat_map(|((ip, port), piece_index)| {
            let peer_id = format!("127.0.0.{}:{}", ip, port);
            let data = vec![1, 2, 3, 4]; 
            
            let actions = vec![
                Action::PeerSuccessfullyConnected { peer_id: peer_id.clone() },
                Action::PeerBitfieldReceived { peer_id: peer_id.clone(), bitfield: vec![] },
                Action::PeerHavePiece { peer_id: peer_id.clone(), piece_index },
                Action::PeerUnchoked { peer_id: peer_id.clone() },
                Action::IncomingBlock { 
                    peer_id: peer_id.clone(), 
                    piece_index, 
                    block_offset: 0, 
                    data: data.clone() 
                },
                Action::PieceVerified { 
                    peer_id: peer_id.clone(), 
                    piece_index, 
                    valid: true, 
                    data 
                },
                // IMPORTANT: This triggers the bug check.
                Action::PieceWrittenToDisk { 
                    peer_id: peer_id.clone(), 
                    piece_index 
                }
            ];
            Just(actions)
        })
    }

    // 3. Upload Story (Seeding Simulation)
    fn successful_upload_story() -> impl Strategy<Value = Vec<Action>> {
        let peer_gen = (1..255u8, 1000..9999u16);
        let piece_gen = 0..20u32;
        
        (peer_gen, piece_gen).prop_flat_map(|((ip, port), piece_index)| {
            let peer_id = format!("127.0.0.{}:{}", ip, port);
            let length = 16384; 
            
            let actions = vec![
                // PRE-CONDITION: "Seeding"
                // We simulate a disk write happening *before* the peer connects.
                // This legitimately marks the piece as Done in the state machine,
                // allowing the subsequent RequestUpload to succeed without "cheating".
                Action::PieceWrittenToDisk { 
                    peer_id: "disk_init".to_string(), 
                    piece_index 
                },
                
                // PEER INTERACTION
                Action::PeerSuccessfullyConnected { peer_id: peer_id.clone() },
                Action::PeerInterested { peer_id: peer_id.clone() }, 
                Action::PeerUnchoked { peer_id: peer_id.clone() }, // We unchoke them
                
                Action::RequestUpload { 
                    peer_id: peer_id.clone(), 
                    piece_index, 
                    block_offset: 0, 
                    length 
                },
                Action::BlockSentToPeer { 
                    peer_id: peer_id.clone(), 
                    byte_count: length as u64 
                }
            ];
            Just(actions)
        })
    }

    // 4. Relay Story (Integration: Download -> Upload)
    fn relay_story_strategy() -> impl Strategy<Value = Vec<Action>> {
        let peer_a_gen = Just(("127.0.0.1".to_string(), 1001u16));
        let peer_b_gen = Just(("127.0.0.1".to_string(), 2002u16));
        let piece_gen = 0..20u32;
        
        (peer_a_gen, peer_b_gen, piece_gen).prop_flat_map(|((ip_a, port_a), (ip_b, port_b), piece_index)| {
            let id_a = format!("{}:{}", ip_a, port_a);
            let id_b = format!("{}:{}", ip_b, port_b);
            let data = vec![1, 2, 3, 4]; 
            let len = 16384; 

            let actions = vec![
                // --- PHASE 1: DOWNLOAD FROM PEER A ---
                Action::PeerSuccessfullyConnected { peer_id: id_a.clone() },
                Action::PeerBitfieldReceived { peer_id: id_a.clone(), bitfield: vec![] },
                Action::PeerHavePiece { peer_id: id_a.clone(), piece_index },
                Action::PeerUnchoked { peer_id: id_a.clone() },
                Action::IncomingBlock { 
                    peer_id: id_a.clone(), 
                    piece_index, 
                    block_offset: 0, 
                    data: data.clone() 
                },
                Action::PieceVerified { 
                    peer_id: id_a.clone(), 
                    piece_index, 
                    valid: true, 
                    data 
                },
                Action::PieceWrittenToDisk { 
                    peer_id: id_a.clone(), 
                    piece_index 
                },

                // --- PHASE 2: UPLOAD TO PEER B ---
                // Peer B asks for the same piece. 
                // This ONLY succeeds if Phase 1 correctly updated the bitfield.
                Action::PeerSuccessfullyConnected { peer_id: id_b.clone() },
                Action::PeerInterested { peer_id: id_b.clone() }, 
                Action::PeerUnchoked { peer_id: id_b.clone() },
                Action::RequestUpload { 
                    peer_id: id_b.clone(), 
                    piece_index, 
                    block_offset: 0, 
                    length: len 
                },
                Action::BlockSentToPeer { 
                    peer_id: id_b.clone(), 
                    byte_count: len as u64 
                }
            ];
            Just(actions)
        })
    }

    fn griefing_story_strategy() -> impl Strategy<Value = Vec<Action>> {
        (1..255u8).prop_map(|ip| {
            let pid = format!("127.0.0.{}", ip);
            vec![
                Action::PeerSuccessfullyConnected { peer_id: pid.clone() },
                Action::PeerChoked { peer_id: pid.clone() },
                Action::PeerDisconnected { peer_id: pid }
            ]
        })
    }

    // 5. The Master Weighted Strategy
    fn mixed_behavior_strategy() -> impl Strategy<Value = Vec<Action>> {
        prop_oneof![
            // Weight 2: Chaos (Random single actions)
            2 => chaos_strategy().prop_map(|a| vec![a]),
            
            // Weight 1: Simple Connections
            1 => connection_story_strategy(),
            
            // Weight 4: Full Download Cycles
            4 => successful_download_story(),

            // Weight 4: Full Upload Cycles
            4 => successful_upload_story(),
            
            // Weight 4: Relay (Integration)
            4 => relay_story_strategy(),
            
            // Weight 1: Griefing
            1 => griefing_story_strategy(),
        ]
    }

    // =========================================================================
    // 3. INVARIANTS
    // =========================================================================

    fn check_invariants(state: &TorrentState) {
        // 1. Queue Consistency
        for piece in &state.piece_manager.need_queue {
            assert!(!state.piece_manager.pending_queue.contains_key(piece), 
                "Piece {} is both Needed and Pending!", piece);
        }

        // 2. Timer Safety
        if let Some(timer) = state.optimistic_unchoke_timer {
            assert!(timer <= Instant::now() + Duration::from_secs(35), 
                "Optimistic timer is weirdly far in the future");
        }

        // 3. Stats Consistency
        assert!(state.bytes_downloaded_in_interval <= state.session_total_downloaded, 
            "Interval bytes > Session total!");

        // 4. Peer Map Key Consistency
        for (id, peer) in &state.peers {
            assert_eq!(&peer.ip_port, id, "Peer Map Key mismatch");
        }
        
        // 5. Completion Logic
        if state.torrent_status == TorrentStatus::Done {
            assert!(state.piece_manager.need_queue.is_empty(), 
                "Status is Done but Need Queue is not empty");
        }

        // 6. Pending Consistency (The Sabotage Catcher)
        // If a piece is marked DONE globally, no peer should have it in 'pending_requests'
        for (peer_id, peer) in &state.peers {
            for &pending_piece in &peer.pending_requests {
                if let Some(status) = state.piece_manager.bitfield.get(pending_piece as usize) {
                    assert_ne!(status, &PieceStatus::Done, 
                        "Peer {} is requesting Piece {} which is already DONE!", peer_id, pending_piece);
                }
            }
        }
        
        // 7. Global vs Peer Pending Sync
        if !state.peers.is_empty() {
            for &piece_idx in state.piece_manager.pending_queue.keys() {
                let is_tracked_by_peer = state.peers.values()
                    .any(|p| p.pending_requests.contains(&piece_idx));
                
                assert!(is_tracked_by_peer, 
                    "Piece {} is globally pending but no peer is tracking it!", piece_idx);
            }
        }

        // 8. Upload Stats Consistency
        let sum_peer_upload: u64 = state.peers.values()
            .map(|p| p.total_bytes_uploaded)
            .sum();
        
        assert!(sum_peer_upload <= state.session_total_uploaded,
            "Sum of peer uploads ({}) exceeds session total ({})",
            sum_peer_upload, state.session_total_uploaded);

        // 9. Peer Count Synchronization
        assert_eq!(state.number_of_successfully_connected_peers, state.peers.len(),
            "Peer count metric mismatch! Counter: {}, Map: {}", 
            state.number_of_successfully_connected_peers, state.peers.len());

        // 10. Math/Float Stability
        assert!(!state.total_dl_prev_avg_ema.is_nan(), "DL EMA is NaN");
        assert!(state.total_dl_prev_avg_ema.is_finite(), "DL EMA is Infinite");
        // Check for explosion (e.g. > 100 GB/s)
        assert!(state.total_dl_prev_avg_ema < 100_000_000_000.0, "DL EMA exploded!");
        
        for (id, peer) in &state.peers {
            assert!(!peer.prev_avg_dl_ema.is_nan(), "Peer {} DL EMA is NaN", id);
            assert!(!peer.prev_avg_ul_ema.is_nan(), "Peer {} UL EMA is NaN", id);
        }

        // 11. Timer Sanity
        if !state.is_paused {
            if let Some(timer) = state.optimistic_unchoke_timer {
                let now = Instant::now();
                // Check Future bounds (allow 60s buffer)
                assert!(timer <= now + Duration::from_secs(60), "Timer too far in future");
                // Check Past bounds (allow 1 hour buffer for stuck logic)
                // We use checked_sub to avoid panics at t=0
                if let Some(limit) = now.checked_sub(Duration::from_secs(3600)) {
                    assert!(timer >= limit, "Timer is stale (stuck in past)");
                }
            }
        }

        // 12. Completion Consistency
        if state.torrent_status == TorrentStatus::Done {
            let all_done = state.piece_manager.bitfield.iter().all(|&p| p == PieceStatus::Done);
            // Ignore empty bitfields (initialization state)
            if !state.piece_manager.bitfield.is_empty() {
                assert!(all_done, "Status is Done but bitfield is incomplete!");
            }
        }
    }

    // =========================================================================
    // 4. THE RUNNER
    // =========================================================================

    const TOTAL_CASES: usize = 256;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(TOTAL_CASES as u32))]

        #[test]
        fn test_stateful_robustness(
            story_batches in proptest::collection::vec(mixed_behavior_strategy(), 1..30)
        ) {

        use std::sync::atomic::{AtomicUsize, Ordering};
        // This is where we track the current iteration number across all runs
        static COUNTER: AtomicUsize = AtomicUsize::new(1);
        
        let iteration = COUNTER.fetch_add(1, Ordering::SeqCst);

        if iteration <= TOTAL_CASES {
            let percentage = (iteration as f64 / TOTAL_CASES as f64) * 100.0;

            eprintln!(
                "🧪 **Progress**: {:.2}% complete ({}/{})",
                percentage, iteration - 1, TOTAL_CASES
            );
        } else {
            // Once the iteration count exceeds TOTAL_CASES, we assume the test has 
            // failed and Proptest is attempting to shrink the input.
            let shrinking_count = iteration - TOTAL_CASES;
            eprintln!(
                "🔥 **Regressions/Shrinking**: Running old saved seeds and new found errors (Executions: {})",
                shrinking_count
            );
        }
        // ------------------------------------------
            let mut state = super::tests::create_empty_state();
            
            // 1. Setup Torrent
            let torrent = super::tests::create_dummy_torrent(20);
            state.torrent = Some(torrent);
            state.piece_manager.set_initial_fields(20, false);
            
            // 2. POPULATE NEED QUEUE
            // Essential for download tests to actually trigger work assignment
            state.piece_manager.need_queue.clear();
            for i in 0..20 {
                state.piece_manager.need_queue.push(i);
            }

            state.torrent_status = TorrentStatus::Standard;

            for story in story_batches {
                for action in story {
                    
                    // 3. INJECT PEER (Simulation Adapter)
                    // The PeerSuccessfullyConnected action signifies a successful handshake.
                    // We must create the PeerState here because there is no network layer 
                    // in this unit test to do it for us.
                    if let Action::PeerSuccessfullyConnected { peer_id } = &action {
                        if !state.peers.contains_key(peer_id) {
                            let (tx, _) = tokio::sync::mpsc::channel(1);
                            let mut peer = PeerState::new(peer_id.clone(), tx, state.now);
                            peer.peer_id = peer_id.as_bytes().to_vec(); 
                            state.peers.insert(peer_id.clone(), peer);
                        }
                    }


                    let _ = state.update(action);
                    check_invariants(&state);
                }
            }
        }
    }
}
