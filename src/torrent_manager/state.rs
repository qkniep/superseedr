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
use std::collections::{HashMap, HashSet};


const PEER_UPLOAD_IN_FLIGHT_LIMIT: usize = 4;

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
            optimistic_unchoke_timer: Some(Instant::now()),
            ..Default::default()
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
