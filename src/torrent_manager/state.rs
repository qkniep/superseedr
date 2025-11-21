// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::TorrentCommand;
use crate::torrent_manager::ManagerEvent;
use crate::torrent_manager::ManagerCommand;
use crate::networking::BlockInfo;

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

use crate::tracker::TrackerEvent;

use rand::prelude::IndexedRandom;

const BITS_PER_BYTE: u64 = 8;
const SMOOTHING_PERIOD_MS: f64 = 5000.0;
const PEER_UPLOAD_IN_FLIGHT_LIMIT: usize = 4;

pub type PeerAddr = (String, u16);

#[derive(Debug)]
pub enum Action {
    Tick { dt_ms: u64 },
    RecalculateChokes { upload_slots: usize },
    PeerEvent(TorrentCommand),
    ManagerEvent(ManagerCommand),
    CheckCompletion,
    AssignWork { peer_id: String },
    PeerSuccessfullyConnected { peer_id: String },
    PeerDisconnected { peer_id: String },
    UpdatePeerId { peer_addr: String, new_id: Vec<u8> },
    PeerBitfieldReceived { peer_id: String, bitfield: Vec<u8> },
    PeerChoked { peer_id: String },
    PeerUnchoked { peer_id: String },
    PeerInterested { peer_id: String },
    PeerHavePiece { peer_id: String, piece_index: u32 },
    IncomingBlock { 
        peer_id: String, 
        piece_index: u32, 
        block_offset: u32, 
        data: Vec<u8> 
    },
    PieceVerified { 
        peer_id: String, 
        piece_index: u32, 
        valid: bool,
        data: Vec<u8>
    },
    PieceWrittenToDisk { 
        peer_id: String, 
        piece_index: u32 
    },
    PieceWriteFailed { piece_index: u32 },
    RequestUpload { 
        peer_id: String, 
        piece_index: u32, 
        block_offset: u32, 
        length: u32 
    },
    BlockSentToPeer {
        peer_id: String,
        byte_count: u64
    },
    CancelUpload { 
        peer_id: String, 
        piece_index: u32, 
        block_offset: u32, 
        length: u32 
    },
    TrackerResponse { 
        url: String, 
        peers: Vec<PeerAddr>, 
        interval: u64, 
        min_interval: Option<u64> 
    },
    TrackerError { url: String, error: String },
    PeerConnectionFailed { peer_addr: String },
    MetadataReceived { torrent: Torrent, metadata_length: i64 },
    ValidationComplete { completed_pieces: Vec<u32> },
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
    SendToPeer { peer_id: String, cmd: TorrentCommand },
    DisconnectPeer { peer_id: String },
    AnnounceCompleted { url: String },
    
    // --- New I/O & Work Effects ---
    VerifyPiece { peer_id: String, piece_index: u32, data: Vec<u8> },
    WriteToDisk { peer_id: String, piece_index: u32, data: Vec<u8> },
    ReadFromDisk { peer_id: String, block_info: BlockInfo },
    BroadcastHave { piece_index: u32 },

    ConnectToPeer { ip: String, port: u16 },
    InitializeStorage,
    StartValidation,
    SendBitfieldToPeers, 
    AnnounceToTracker { 
        url: String,
        event: TrackerEvent,
    },

    ConnectToPeersFromTrackers,
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
    Standard,
    Endgame,
    Done,
}

#[derive(PartialEq, Debug)]
pub enum ChokeStatus {
    Choke,
    Unchoke,
    Pending,
}

#[derive(Debug, Default)]
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
            optimistic_unchoke_timer: Some(Instant::now().checked_sub(Duration::from_secs(31)).unwrap_or(Instant::now())),
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
                let scaling_factor = if dt_ms > 0 {
                    1000.0 / dt_ms as f64
                } else {
                    1.0
                };
                let dt = dt_ms as f64;
                let alpha = 1.0 - (-dt / SMOOTHING_PERIOD_MS).exp();

                let inst_total_dl_speed = (self.bytes_downloaded_in_interval * BITS_PER_BYTE) as f64 * scaling_factor;
                let inst_total_ul_speed = (self.bytes_uploaded_in_interval * BITS_PER_BYTE) as f64 * scaling_factor;
                
                let dl_tick = self.bytes_downloaded_in_interval;
                let ul_tick = self.bytes_uploaded_in_interval;

                self.bytes_downloaded_in_interval = 0;
                self.bytes_uploaded_in_interval = 0;

                self.total_dl_prev_avg_ema = (inst_total_dl_speed * alpha) + (self.total_dl_prev_avg_ema * (1.0 - alpha));
                self.total_ul_prev_avg_ema = (inst_total_ul_speed * alpha) + (self.total_ul_prev_avg_ema * (1.0 - alpha));

                for peer in self.peers.values_mut() {
                    let inst_dl_speed = (peer.bytes_downloaded_in_tick * BITS_PER_BYTE) as f64 * scaling_factor;
                    let inst_ul_speed = (peer.bytes_uploaded_in_tick * BITS_PER_BYTE) as f64 * scaling_factor;

                    peer.prev_avg_dl_ema = (inst_dl_speed * alpha) + (peer.prev_avg_dl_ema * (1.0 - alpha));
                    peer.download_speed_bps = peer.prev_avg_dl_ema as u64;

                    peer.prev_avg_ul_ema = (inst_ul_speed * alpha) + (peer.prev_avg_ul_ema * (1.0 - alpha));
                    peer.upload_speed_bps = peer.prev_avg_ul_ema as u64;

                    peer.bytes_downloaded_in_tick = 0;
                    peer.bytes_uploaded_in_tick = 0;
                }

                vec![Effect::EmitMetrics {
                    bytes_dl: dl_tick,
                    bytes_ul: ul_tick,
                }]
            }

            Action::RecalculateChokes { upload_slots } => {
                let mut effects = Vec::new();

                let mut interested_peers: Vec<&mut PeerState> = self
                    .peers
                    .values_mut()
                    .filter(|p| p.peer_is_interested_in_us)
                    .collect();

                if self.torrent_status == TorrentStatus::Done {
                    interested_peers.sort_by(|a, b| b.bytes_uploaded_to_peer.cmp(&a.bytes_uploaded_to_peer));
                } else {
                    interested_peers.sort_by(|a, b| b.bytes_downloaded_from_peer.cmp(&a.bytes_downloaded_from_peer));
                }

                let mut unchoke_candidates: HashSet<String> = interested_peers
                    .iter()
                    .take(upload_slots)
                    .map(|p| p.ip_port.clone())
                    .collect();

                if self.optimistic_unchoke_timer.map_or(false, |t| t.elapsed() > Duration::from_secs(30)) {
                    let optimistic_candidates: Vec<&mut PeerState> = interested_peers
                        .into_iter()
                        .filter(|p| !unchoke_candidates.contains(&p.ip_port))
                        .collect();

                    if let Some(optimistic_peer) = optimistic_candidates.choose(&mut rand::rng()) {
                        unchoke_candidates.insert(optimistic_peer.ip_port.clone());
                    }
                    
                    self.optimistic_unchoke_timer = Some(Instant::now());
                }

                for peer in self.peers.values_mut() {
                    if unchoke_candidates.contains(&peer.ip_port) {
                        if peer.am_choking == ChokeStatus::Choke {
                            peer.am_choking = ChokeStatus::Unchoke;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: TorrentCommand::PeerUnchoke
                            });
                        }
                    } else {
                        if peer.am_choking == ChokeStatus::Unchoke {
                            peer.am_choking = ChokeStatus::Choke;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: TorrentCommand::PeerChoke
                            });
                        }
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

                let all_done = self.piece_manager.bitfield.iter().all(|s| *s == PieceStatus::Done);
                
                if all_done {
                    let mut effects = Vec::new();
                    self.torrent_status = TorrentStatus::Done;

                    for (url, tracker) in self.trackers.iter_mut() {
                        tracker.next_announce_time = Instant::now();
                        effects.push(Effect::AnnounceCompleted { url: url.clone() });
                    }

                    for peer in self.peers.values_mut() {
                        if peer.am_interested {
                            peer.am_interested = false;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: TorrentCommand::NotInterested
                            });
                        }
                    }
                    return effects;
                }

                vec![Effect::DoNothing]
            }

            // --- Work Assignment ---

            Action::AssignWork { peer_id } => {
                if self.piece_manager.need_queue.is_empty() && self.piece_manager.pending_queue.is_empty() {
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
                            && self.piece_manager.need_queue.iter().any(|&p| peer.bitfield.get(p as usize) == Some(&true)) 
                        {
                            peer.am_interested = true;
                            effects.push(Effect::SendToPeer {
                                peer_id: peer_id.clone(),
                                cmd: TorrentCommand::ClientInterested
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
                        self.piece_manager.mark_as_pending(piece_index, peer_id.clone());

                        if self.piece_manager.need_queue.is_empty() && self.torrent_status != TorrentStatus::Endgame {
                            self.torrent_status = TorrentStatus::Endgame;
                        }

                        effects.push(Effect::SendToPeer {
                            peer_id: peer_id.clone(),
                            cmd: TorrentCommand::RequestDownload(
                                piece_index,
                                torrent.info.piece_length,
                                total_size
                            )
                        });
                    }
                }
                effects
            }

            // --- Peer Lifecycle Actions ---

            Action::PeerSuccessfullyConnected { peer_id } => {
                self.timed_out_peers.remove(&peer_id);
                self.number_of_successfully_connected_peers += 1;

                if !self.has_made_first_connection {
                    self.has_made_first_connection = true;
                }
                
                vec![
                    Effect::EmitManagerEvent(ManagerEvent::PeerConnected { info_hash: self.info_hash.clone() }),
                ]
            }

            Action::PeerDisconnected { peer_id } => {
                let mut effects = Vec::new();
                if let Some(removed_peer) = self.peers.remove(&peer_id) {
                    for piece_index in removed_peer.pending_requests {
                        if self.piece_manager.bitfield.get(piece_index as usize) != Some(&PieceStatus::Done) {
                            self.piece_manager.requeue_pending_to_need(piece_index);
                        }
                    }

                    if self.number_of_successfully_connected_peers > 0 {
                        self.number_of_successfully_connected_peers -= 1;
                    }
                    effects.push(Effect::EmitManagerEvent(ManagerEvent::PeerDisconnected { info_hash: self.info_hash.clone() }));
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
                    peer.bitfield = bitfield.iter()
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

            Action::PeerHavePiece { peer_id, piece_index } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    if (piece_index as usize) < peer.bitfield.len() {
                        peer.bitfield[piece_index as usize] = true;
                    }
                }
                self.update(Action::AssignWork { peer_id })
            }

            // --- Data Flow (The Core Logic) ---

            Action::IncomingBlock { peer_id, piece_index, block_offset, data } => {
                let mut effects = Vec::new();
                self.last_activity = TorrentActivity::DownloadingPiece(piece_index);

                let len = data.len() as u64;
                self.bytes_downloaded_in_interval += len;
                self.session_total_downloaded += len;

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.bytes_downloaded_from_peer += len;
                    peer.bytes_downloaded_in_tick += len;
                    peer.total_bytes_downloaded += len;
                }

                effects.push(Effect::EmitManagerEvent(ManagerEvent::BlockReceived { info_hash: self.info_hash.clone() }));

                let piece_size = self.get_piece_size(piece_index);
                if let Some(complete_data) = self.piece_manager.handle_block(piece_index, block_offset, &data, piece_size) {
                    self.last_activity = TorrentActivity::VerifyingPiece(piece_index);
                    effects.push(Effect::VerifyPiece { peer_id: peer_id.clone(), piece_index, data: complete_data });
                }

                effects
            }

            Action::PieceVerified { peer_id, piece_index, valid, data } => {
                let mut effects = Vec::new();
                if valid {
                    if self.piece_manager.bitfield.get(piece_index as usize) == Some(&PieceStatus::Done) {
                        if let Some(peer) = self.peers.get_mut(&peer_id) {
                            peer.pending_requests.remove(&piece_index);
                        }
                        // Redundant piece; we already have it. Discard data and assign new work.
                        effects.extend(self.update(Action::AssignWork { peer_id }));
                    } else {
                        // Valid and needed piece. Request write to disk.
                        // The data payload is now properly passed from the Action.
                        effects.push(Effect::WriteToDisk { peer_id: peer_id.clone(), piece_index, data });
                    }
                } else {
                    self.piece_manager.reset_piece_assembly(piece_index);
                    effects.push(Effect::DisconnectPeer { peer_id });
                }
                effects
            }

            Action::PieceWrittenToDisk { peer_id, piece_index } => {
                let mut effects = Vec::new();
                let peers_to_cancel = self.piece_manager.mark_as_complete(piece_index);
                
                effects.push(Effect::EmitManagerEvent(ManagerEvent::DiskWriteFinished));

                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.pending_requests.remove(&piece_index);
                }

                effects.extend(self.update(Action::AssignWork { peer_id: peer_id.clone() }));

                for other_peer in peers_to_cancel {
                    if other_peer != peer_id {
                        if let Some(peer) = self.peers.get_mut(&other_peer) {
                            peer.pending_requests.remove(&piece_index);
                            effects.push(Effect::SendToPeer {
                                peer_id: other_peer.clone(),
                                cmd: TorrentCommand::Cancel(piece_index)
                            });
                        }
                        effects.extend(self.update(Action::AssignWork { peer_id: other_peer }));
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

            // --- Upload Actions ---

            Action::RequestUpload { peer_id, piece_index, block_offset, length } => {
                self.last_activity = TorrentActivity::SendingPiece(piece_index);
                self.session_total_uploaded += length as u64;
                self.bytes_uploaded_in_interval += length as u64;

                let mut allowed = false;
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.total_bytes_uploaded += length as u64;
                    peer.bytes_uploaded_in_tick += length as u64;
                    
                    if peer.am_choking == ChokeStatus::Unchoke 
                       && self.piece_manager.bitfield.get(piece_index as usize) == Some(&PieceStatus::Done) {
                        allowed = true;
                    }
                }

                if allowed {
                    vec![Effect::ReadFromDisk { 
                        peer_id, 
                        block_info: BlockInfo { piece_index, offset: block_offset, length } 
                    }]
                } else {
                    vec![Effect::DoNothing]
                }
            }

            Action::BlockSentToPeer { peer_id, byte_count } => {
                if let Some(peer) = self.peers.get_mut(&peer_id) {
                    peer.bytes_uploaded_to_peer += byte_count;
                }
                vec![Effect::EmitManagerEvent(ManagerEvent::BlockSent { info_hash: self.info_hash.clone() })]
            }

            Action::CancelUpload { .. } => {
                vec![Effect::DoNothing]
            }

            Action::TrackerResponse { url, peers, interval, min_interval } => {
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
                    tracker.next_announce_time = Instant::now() + next_interval;
                }

                for (ip, port) in peers {
                    effects.push(Effect::ConnectToPeer { ip, port });
                }
                
                effects
            }

            Action::TrackerError { url, error: _ } => {
                if let Some(tracker) = self.trackers.get_mut(&url) {
                    let current_interval = if self.torrent_status != TorrentStatus::Done {
                        tracker.leeching_interval.unwrap_or(Duration::from_secs(60))
                    } else {
                        tracker.seeding_interval.unwrap_or(Duration::from_secs(1800))
                    };
                    
                    let backoff = current_interval.mul_f32(2.0).min(Duration::from_secs(3600));
                    tracker.next_announce_time = Instant::now() + backoff;
                }
                vec![Effect::DoNothing]
            }

            Action::PeerConnectionFailed { peer_addr } => {
                let now = Instant::now();
                let (count, _) = self.timed_out_peers.get(&peer_addr).cloned().unwrap_or((0, now));
                let new_count = (count + 1).min(10);
                let backoff_secs = (15 * 2u64.pow(new_count - 1)).min(1800);
                let next_attempt = now + Duration::from_secs(backoff_secs);
                
                self.timed_out_peers.insert(peer_addr, (new_count, next_attempt));
                vec![Effect::DoNothing]
            }

            Action::MetadataReceived { torrent, metadata_length } => {
                if self.torrent.is_some() {
                    return vec![Effect::DoNothing];
                }
                
                self.torrent = Some(torrent.clone());
                self.torrent_metadata_length = Some(metadata_length);
                
                let pieces = torrent.info.pieces.len() / 20;
                self.piece_manager.set_initial_fields(pieces, self.torrent_validation_status);
                
                if let Some(announce) = &torrent.announce {
                    self.trackers.insert(announce.clone(), TrackerState {
                        next_announce_time: Instant::now(),
                        leeching_interval: None,
                        seeding_interval: None,
                    });
                }

                vec![
                    Effect::InitializeStorage, 
                    Effect::StartValidation,
                    Effect::SendBitfieldToPeers,
                    Effect::ConnectToPeersFromTrackers,
                ]
            }

            Action::ValidationComplete { completed_pieces } => {
                for piece_index in completed_pieces {
                    self.piece_manager.mark_as_complete(piece_index);
                }
                self.update(Action::CheckCompletion)
            }

            _ => vec![Effect::DoNothing],
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
}

impl PeerState {
    pub fn new(ip_port: String, peer_tx: Sender<TorrentCommand>) -> Self {
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
        }
    }
}
