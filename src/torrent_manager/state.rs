// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::TorrentCommand;

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

use crate::torrent_manager::ManagerCommand;

use rand::prelude::IndexedRandom;


const BITS_PER_BYTE: u64 = 8;
const SMOOTHING_PERIOD_MS: f64 = 5000.0;
const PEER_UPLOAD_IN_FLIGHT_LIMIT: usize = 4;

#[derive(Debug)]
pub enum Action {
    Tick { dt_ms: u64 },
    RecalculateChokes { upload_slots: usize },
    PeerEvent(TorrentCommand),
    ManagerEvent(ManagerCommand),
    CheckCompletion,
}

#[derive(Debug)]
#[must_use]
pub enum Effect {
    DoNothing,
    EmitMetrics {
        bytes_dl: u64,
        bytes_ul: u64,
    },
    SendToPeer { peer_id: String, cmd: TorrentCommand },
    AnnounceCompleted { url: String }
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

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Tick { dt_ms } => {
                // 1. Calculate Scaling Factors
                let scaling_factor = if dt_ms > 0 {
                    1000.0 / dt_ms as f64
                } else {
                    1.0
                };
                let dt = dt_ms as f64;
                let alpha = 1.0 - (-dt / SMOOTHING_PERIOD_MS).exp();

                // 2. Update Global Session Speeds
                // Note: We use the counters accumulated in self
                let inst_total_dl_speed = (self.bytes_downloaded_in_interval * BITS_PER_BYTE) as f64 * scaling_factor;
                let inst_total_ul_speed = (self.bytes_uploaded_in_interval * BITS_PER_BYTE) as f64 * scaling_factor;
                
                let dl_tick = self.bytes_downloaded_in_interval;
                let ul_tick = self.bytes_uploaded_in_interval;

                // Reset the interval counters immediately
                self.bytes_downloaded_in_interval = 0;
                self.bytes_uploaded_in_interval = 0;

                // Apply Smoothing (EMA)
                self.total_dl_prev_avg_ema = (inst_total_dl_speed * alpha) + (self.total_dl_prev_avg_ema * (1.0 - alpha));
                self.total_ul_prev_avg_ema = (inst_total_ul_speed * alpha) + (self.total_ul_prev_avg_ema * (1.0 - alpha));

                // 3. Update Per-Peer Speeds
                for peer in self.peers.values_mut() {
                    let inst_dl_speed = (peer.bytes_downloaded_in_tick * BITS_PER_BYTE) as f64 * scaling_factor;
                    let inst_ul_speed = (peer.bytes_uploaded_in_tick * BITS_PER_BYTE) as f64 * scaling_factor;

                    // Update Peer EMA
                    peer.prev_avg_dl_ema = (inst_dl_speed * alpha) + (peer.prev_avg_dl_ema * (1.0 - alpha));
                    peer.download_speed_bps = peer.prev_avg_dl_ema as u64;

                    peer.prev_avg_ul_ema = (inst_ul_speed * alpha) + (peer.prev_avg_ul_ema * (1.0 - alpha));
                    peer.upload_speed_bps = peer.prev_avg_ul_ema as u64;

                    // Reset peer tick counters
                    peer.bytes_downloaded_in_tick = 0;
                    peer.bytes_uploaded_in_tick = 0;
                }

                // 4. Return the output effect: "Go update the UI"
                vec![Effect::EmitMetrics {
                    bytes_dl: dl_tick,
                    bytes_ul: ul_tick,
                }]
            }

            Action::RecalculateChokes { upload_slots } => {
                let mut effects = Vec::new();

                // 1. Identify Interested Peers
                // We collect mutable references so we can modify them later
                let mut interested_peers: Vec<&mut PeerState> = self
                    .peers
                    .values_mut()
                    .filter(|p| p.peer_is_interested_in_us)
                    .collect();

                // 2. Sort by Speed
                // If we are seeding (Done), sort by Upload to us.
                // If we are leeching, sort by Download from us (Reciprocity).
                if self.torrent_status == TorrentStatus::Done {
                    interested_peers.sort_by(|a, b| b.bytes_uploaded_to_peer.cmp(&a.bytes_uploaded_to_peer));
                } else {
                    interested_peers.sort_by(|a, b| b.bytes_downloaded_from_peer.cmp(&a.bytes_downloaded_from_peer));
                }

                // 3. Select Top N Peers (Standard Unchoke)
                let mut unchoke_candidates: HashSet<String> = interested_peers
                    .iter()
                    .take(upload_slots)
                    .map(|p| p.ip_port.clone())
                    .collect();

                // 4. Optimistic Unchoke (Random)
                // Logic: Every 30 seconds, pick a random interested peer who isn't already unchoked
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


                // 5. Apply Changes & Generate Effects
                // We iterate over ALL peers to ensure we choke those who didn't make the cut
                for peer in self.peers.values_mut() {
                    if unchoke_candidates.contains(&peer.ip_port) {
                        // If they should be unchoked...
                        if peer.am_choking == ChokeStatus::Choke {
                            peer.am_choking = ChokeStatus::Unchoke;
                            // EFFECT: Tell the networking layer to send "Unchoke"
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: TorrentCommand::PeerUnchoke
                            });
                        }
                    } else {
                        // If they should be choked...
                        if peer.am_choking == ChokeStatus::Unchoke {
                            peer.am_choking = ChokeStatus::Choke;
                            // EFFECT: Tell the networking layer to send "Choke"
                            effects.push(Effect::SendToPeer {
                                peer_id: peer.ip_port.clone(),
                                cmd: TorrentCommand::PeerChoke
                            });
                        }
                    }
                    
                    // 6. Reset Interval Counters (Reciprocity logic relies on "bytes since last choke calc")
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
                        effects.push(Effect::AnnounceCompleted { 
                            url: url.clone() 
                        });
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
