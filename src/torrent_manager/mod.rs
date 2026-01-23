// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod block_manager;
pub mod manager;
pub mod merkle;
pub mod piece_manager;
pub mod state;

use crate::Settings;

use std::collections::HashMap;

use crate::token_bucket::TokenBucket;

use crate::torrent_file::Torrent;

use crate::app::FilePriority;
use crate::app::TorrentMetrics;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::Duration;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpStream;

use tokio::sync::broadcast;

#[cfg(feature = "dht")]
use mainline::async_dht::AsyncDht;
#[cfg(not(feature = "dht"))]
type AsyncDht = ();

use crate::resource_manager::ResourceManagerClient;

pub struct TorrentParameters {
    pub dht_handle: AsyncDht,
    pub incoming_peer_rx: Receiver<(TcpStream, Vec<u8>)>,
    pub metrics_tx: broadcast::Sender<TorrentMetrics>,
    pub torrent_validation_status: bool,
    pub torrent_data_path: Option<PathBuf>,
    pub container_name: Option<String>,
    pub manager_command_rx: Receiver<ManagerCommand>,
    pub manager_event_tx: Sender<ManagerEvent>,
    pub settings: Arc<Settings>,
    pub resource_manager: ResourceManagerClient,
    pub global_dl_bucket: Arc<TokenBucket>,
    pub global_ul_bucket: Arc<TokenBucket>,
    pub file_priorities: HashMap<usize, FilePriority>,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct DiskIoOperation {
    pub piece_index: u32,
    pub offset: u64,
    pub length: usize,
}

#[derive(Debug)]
pub enum ManagerEvent {
    DeletionComplete(Vec<u8>, Result<(), String>),
    DiskReadStarted {
        info_hash: Vec<u8>,
        op: DiskIoOperation,
    },
    DiskReadFinished,
    DiskWriteStarted {
        info_hash: Vec<u8>,
        op: DiskIoOperation,
    },
    DiskWriteFinished,
    DiskIoBackoff {
        duration: Duration,
    },
    PeerDiscovered {
        info_hash: Vec<u8>,
    },
    PeerConnected {
        info_hash: Vec<u8>,
    },
    PeerDisconnected {
        info_hash: Vec<u8>,
    },

    BlockReceived {
        info_hash: Vec<u8>,
    },
    BlockSent {
        info_hash: Vec<u8>,
    },
    MetadataLoaded {
        info_hash: Vec<u8>,
        torrent: Box<Torrent>,
    },
}

#[derive(Debug, Clone)]
pub enum ManagerCommand {
    Pause,
    Resume,
    Shutdown,
    DeleteFile,
    SetDataRate(u64),
    UpdateListenPort(u16),
    SetUserTorrentConfig {
        torrent_data_path: PathBuf,
        file_priorities: HashMap<usize, FilePriority>,
        container_name: Option<String>,
    },

    #[cfg(feature = "dht")]
    UpdateDhtHandle(AsyncDht),
}

pub use manager::TorrentManager;
