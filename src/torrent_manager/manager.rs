// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::PeerInfo;
use crate::app::TorrentMetrics;

use crate::resource_manager::ResourceManagerClient;
use crate::resource_manager::ResourceManagerError;

use crate::networking::ConnectionType;

use crate::token_bucket::TokenBucket;

use crate::torrent_manager::DiskIoOperation;

use crate::config::Settings;

use crate::torrent_manager::piece_manager::PieceStatus;



use crate::torrent_manager::state::Effect;
use crate::torrent_manager::state::ChokeStatus;
use crate::torrent_manager::state::PeerState;
use crate::torrent_manager::state::TorrentActivity;
use crate::torrent_manager::state::TorrentState;
use crate::torrent_manager::state::Action;


use crate::torrent_manager::state::TorrentStatus;
use crate::torrent_manager::state::TrackerState;
use crate::torrent_manager::ManagerCommand;
use crate::torrent_manager::ManagerEvent;
use crate::torrent_manager::piece_manager::PieceManager;

use crate::errors::StorageError;
use crate::storage::create_and_allocate_files;
use crate::storage::read_data_from_disk;
use crate::storage::write_data_to_disk;
use crate::storage::MultiFileInfo;

use crate::command::TorrentCommand;
use crate::command::TorrentCommandSummary;

use crate::networking::session::PeerSessionParameters;
use crate::networking::BlockInfo;
use crate::networking::PeerSession;

use crate::tracker::client::{
    announce_completed, announce_periodic, announce_started, announce_stopped,
};

use rand::Rng;

use crate::torrent_file::Torrent;

use std::error::Error;

use tracing::{event, Level};

#[cfg(feature = "dht")]
use mainline::async_dht::AsyncDht;
#[cfg(feature = "dht")]
use mainline::Id;
#[cfg(not(feature = "dht"))]
type AsyncDht = ();

use std::time::Duration;
use std::time::Instant;

use magnet_url::Magnet;

use urlencoding::decode;

use data_encoding::BASE32;

use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::net::TcpStream;
use tokio::signal;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::watch;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_stream::StreamExt;

use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::sync::Arc;

use crate::torrent_manager::TorrentParameters;

const HASH_LENGTH: usize = 20;
const MAX_BLOCK_SIZE: u32 = 131_072;
const CLIENT_LEECHING_FALLBACK_INTERVAL: u64 = 60;
const FALLBACK_ANNOUNCE_INTERVAL: u64 = 1800;

const BASE_COOLDOWN_SECS: u64 = 15;
const MAX_COOLDOWN_SECS: u64 = 1800;
const MAX_TIMEOUT_COUNT: u32 = 10;

const MAX_UPLOAD_REQUEST_ATTEMPTS: u32 = 7;
const MAX_PIECE_WRITE_ATTEMPTS: u32 = 12;
const MAX_VALIDATION_ATTEMPTS: u32 = MAX_PIECE_WRITE_ATTEMPTS;

const BASE_BACKOFF_MS: u64 = 1000;
const JITTER_MS: u64 = 100;

pub struct TorrentManager {
    state: TorrentState,

    root_download_path: PathBuf,
    multi_file_info: Option<MultiFileInfo>,

    torrent_manager_tx: Sender<TorrentCommand>,
    torrent_manager_rx: Receiver<TorrentCommand>,

    #[cfg(feature = "dht")]
    dht_tx: Sender<Vec<SocketAddrV4>>,
    #[cfg(not(feature = "dht"))]
    dht_tx: Sender<()>,

    metrics_tx: broadcast::Sender<TorrentMetrics>,
    manager_event_tx: Sender<ManagerEvent>,
    shutdown_tx: broadcast::Sender<()>,

    #[cfg(feature = "dht")]
    dht_rx: Receiver<Vec<SocketAddrV4>>,
    #[cfg(not(feature = "dht"))]
    dht_rx: Receiver<()>,

    incoming_peer_rx: Receiver<(TcpStream, Vec<u8>)>,
    manager_command_rx: Receiver<ManagerCommand>,

    in_flight_uploads: HashMap<String, HashMap<BlockInfo, JoinHandle<()>>>,

    #[cfg(feature = "dht")]
    dht_trigger_tx: watch::Sender<()>,
    #[cfg(feature = "dht")]
    dht_task_handle: Option<JoinHandle<()>>,

    #[cfg(not(feature = "dht"))]
    dht_trigger_tx: (),
    #[cfg(not(feature = "dht"))]
    dht_task_handle: (),

    dht_handle: AsyncDht,
    settings: Arc<Settings>,
    resource_manager: ResourceManagerClient,

    global_dl_bucket: Arc<Mutex<TokenBucket>>,
    global_ul_bucket: Arc<Mutex<TokenBucket>>,
}

impl TorrentManager {

    pub fn from_torrent(
        torrent_parameters: TorrentParameters,
        torrent: Torrent,
    ) -> Result<Self, String> {
        let TorrentParameters {
            dht_handle,
            incoming_peer_rx,
            metrics_tx,
            torrent_validation_status,
            download_dir,
            manager_command_rx,
            manager_event_tx,
            settings,
            resource_manager,
            global_dl_bucket,
            global_ul_bucket,
        } = torrent_parameters;

        let bencoded_data = serde_bencode::to_bytes(&torrent)
            .map_err(|e| format!("Failed to re-encode torrent struct: {}", e))?;

        let torrent_length = bencoded_data.len();

        let mut trackers = HashMap::new();
        if let Some(ref announce) = torrent.announce {
            trackers.insert(
                announce.clone(),
                TrackerState {
                    next_announce_time: Instant::now(),
                    leeching_interval: None,
                    seeding_interval: None,
                },
            );
        }

        let mut info_dict_hasher = Sha1::new();
        info_dict_hasher.update(&torrent.info_dict_bencode);
        let info_hash = info_dict_hasher.finalize();

        let (torrent_manager_tx, torrent_manager_rx) = mpsc::channel::<TorrentCommand>(100);
        let (shutdown_tx, _) = broadcast::channel(1);

        #[cfg(feature = "dht")]
        let (dht_tx, dht_rx) = mpsc::channel::<Vec<SocketAddrV4>>(10);
        #[cfg(not(feature = "dht"))]
        let (dht_tx, dht_rx) = mpsc::channel::<()>(1);

        #[cfg(feature = "dht")]
        let dht_task_handle = None;
        #[cfg(not(feature = "dht"))]
        let dht_task_handle = ();

        #[cfg(feature = "dht")]
        let (dht_trigger_tx, _) = watch::channel(());
        #[cfg(not(feature = "dht"))]
        let dht_trigger_tx = ();

        let pieces_len = torrent.info.pieces.len();

        let mut piece_manager = PieceManager::new();
        piece_manager.set_initial_fields(pieces_len / 20, torrent_validation_status);

        let multi_file_info = MultiFileInfo::new(
            &download_dir,
            &torrent.info.name,
            if torrent.info.files.is_empty() {
                None
            } else {
                Some(&torrent.info.files)
            },
            if torrent.info.files.is_empty() {
                Some(torrent.info.length as u64)
            } else {
                None
            },
        )
        .map_err(|e| format!("Failed to initialize file manager: {}", e))?;

        let state = TorrentState::new(
            info_hash.to_vec(),
            Some(torrent),
            Some(torrent_length as i64),
            piece_manager,
            trackers,
            torrent_validation_status,
        );

        Ok(Self {
            state,
            root_download_path: download_dir,
            multi_file_info: Some(multi_file_info),
            torrent_manager_tx,
            torrent_manager_rx,
            dht_handle,
            dht_tx,
            dht_rx,
            dht_task_handle,
            incoming_peer_rx,
            metrics_tx,
            shutdown_tx,
            manager_command_rx,
            manager_event_tx,
            in_flight_uploads: HashMap::new(),
            dht_trigger_tx,
            settings,
            resource_manager,
            global_dl_bucket,
            global_ul_bucket,
        })
    }

    pub fn from_magnet(
        torrent_parameters: TorrentParameters,
        magnet: Magnet,
    ) -> Result<Self, String> {
        assert_eq!(magnet.hash_type(), Some("btih"));

        let TorrentParameters {
            dht_handle,
            incoming_peer_rx,
            metrics_tx,
            torrent_validation_status,
            download_dir,
            manager_command_rx,
            manager_event_tx,
            settings,
            resource_manager,
            global_dl_bucket,
            global_ul_bucket,
        } = torrent_parameters;

        let hash_string = magnet
            .hash()
            .ok_or_else(|| "Magnet link does not contain info hash".to_string())?;

        let info_hash = if hash_string.len() == 40 {
            hex::decode(hash_string).map_err(|e| e.to_string())
        } else if hash_string.len() == 32 {
            BASE32
                .decode(hash_string.to_uppercase().as_bytes())
                .map_err(|e| e.to_string())
        } else {
            Err(format!("Invalid info_hash length: {}", hash_string.len()))
        }?;
        event!(Level::DEBUG, "INFO HASH {:?}", info_hash);

        let trackers_set: HashSet<String> = magnet
            .trackers()
            .iter()
            .filter(|t| t.starts_with("http"))
            .filter_map(|t| {
                match decode(t) {
                    Ok(decoded_url) => Some(decoded_url.into_owned()),
                    Err(e) => {
                        event!(Level::DEBUG, tracker_url = %t, error = %e, "Failed to decode tracker URL from magnet link, skipping.");
                        None
                    }
                }
            })
            .collect();
        let mut trackers = HashMap::new();
        for url in trackers_set {
            trackers.insert(
                url.clone(),
                TrackerState {
                    next_announce_time: Instant::now(),
                    leeching_interval: None,
                    seeding_interval: None,
                },
            );
        }

        let (torrent_manager_tx, torrent_manager_rx) = mpsc::channel::<TorrentCommand>(100);
        let (shutdown_tx, _) = broadcast::channel(1);

        #[cfg(feature = "dht")]
        let (dht_tx, dht_rx) = mpsc::channel::<Vec<SocketAddrV4>>(10);
        #[cfg(not(feature = "dht"))]
        let (dht_tx, dht_rx) = mpsc::channel::<()>(1);

        #[cfg(feature = "dht")]
        let dht_task_handle = None;
        #[cfg(not(feature = "dht"))]
        let dht_task_handle = ();

        #[cfg(feature = "dht")]
        let (dht_trigger_tx, _) = watch::channel(());
        #[cfg(not(feature = "dht"))]
        let dht_trigger_tx = ();

        let state = TorrentState::new(
            info_hash,
            None,
            None,
            PieceManager::new(),
            trackers,
            torrent_validation_status,
        );

        Ok(Self {
            state,
            root_download_path: download_dir,
            multi_file_info: None,
            torrent_manager_tx,
            torrent_manager_rx,
            dht_handle,
            dht_tx,
            dht_rx,
            dht_task_handle,
            shutdown_tx,
            incoming_peer_rx,
            metrics_tx,
            manager_command_rx,
            manager_event_tx,
            in_flight_uploads: HashMap::new(),
            dht_trigger_tx,
            settings,
            resource_manager,
            global_dl_bucket,
            global_ul_bucket,
        })
    }

    // Apply actions to update state and get effects resulting from the mutate.
    fn apply_action(&mut self, action: Action) {
        let effects = self.state.update(action);
        for effect in effects {
            self.handle_effect(effect);
        }
    }

    // Handles the aftermath of the mutate effects
    fn handle_effect(&mut self, effect: Effect) {
        match effect {
            Effect::DoNothing => {}

            Effect::EmitManagerEvent(event) => {
                let _ = self.manager_event_tx.try_send(event);
            }

            Effect::EmitMetrics { bytes_dl, bytes_ul } => {
                self.send_metrics(0, bytes_dl, bytes_ul);
            }
            
            Effect::SendToPeer { peer_id, cmd } => {
                if let Some(peer) = self.state.peers.get(&peer_id) {
                    let _ = peer.peer_tx.try_send(cmd);
                }
            }

            Effect::AnnounceCompleted { url } => {
                let info_hash = self.state.info_hash.clone();
                let client_id = self.settings.client_id.clone();
                let client_port = self.settings.client_port;
                let uploaded = self.state.session_total_uploaded as usize;
                let downloaded = self.state.session_total_downloaded as usize;

                tokio::spawn(async move {
                    let _ = announce_completed(
                        url,
                        &info_hash,
                        client_id,
                        client_port,
                        uploaded,
                        downloaded,
                    ).await;
                });
            }

            Effect::DisconnectPeer { peer_id } => {
                if let Some(peer) = self.state.peers.get(&peer_id) {
                    let _ = peer.peer_tx.try_send(TorrentCommand::Disconnect(peer_id.clone()));
                }
                if let Some(handles) = self.in_flight_uploads.remove(&peer_id) {
                    for handle in handles.values() {
                        handle.abort();
                    }
                }
            }

            Effect::BroadcastHave { piece_index } => {
                for peer in self.state.peers.values() {
                    let _ = peer.peer_tx.try_send(TorrentCommand::Have(
                        peer.ip_port.clone(), 
                        piece_index
                    ));
                }
            }

            Effect::VerifyPiece { peer_id, piece_index, data } => {
                let torrent = self.state.torrent.clone().expect("Metadata missing during verify");
                let start = piece_index as usize * HASH_LENGTH;
                let end = start + HASH_LENGTH;
                let expected_hash = torrent.info.pieces.get(start..end).map(|s| s.to_vec());
                
                let tx = self.torrent_manager_tx.clone();
                let peer_id_for_msg = peer_id.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                
                tokio::spawn(async move {
                    let verification_task = tokio::task::spawn_blocking(move || {
                        if let Some(expected) = expected_hash {
                            let hash = sha1::Sha1::digest(&data);
                            if hash.as_slice() == expected.as_slice() {
                                return Ok(data); 
                            }
                        }
                        Err(()) 
                    });

                    let result = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => return, 
                        res = verification_task => res.unwrap_or(Err(())),
                    };

                    match result {
                        Ok(verified_data) => {
                            let _ = tx.send(TorrentCommand::PieceVerified {
                                piece_index,
                                peer_id: peer_id_for_msg, 
                                verification_result: Ok(verified_data) 
                            }).await;
                        }
                        _ => {
                            let _ = tx.send(TorrentCommand::PieceVerified {
                                piece_index,
                                peer_id: peer_id_for_msg, 
                                verification_result: Err(()) 
                            }).await;
                        }
                    }
                });
            }

            Effect::VerifyPiece { peer_id, piece_index, data } => {
                let torrent = self.state.torrent.clone().expect("Metadata missing during verify");
                let start = piece_index as usize * HASH_LENGTH;
                let end = start + HASH_LENGTH;
                let expected_hash = torrent.info.pieces.get(start..end).map(|s| s.to_vec());
                
                let tx = self.torrent_manager_tx.clone();
                let peer_id_for_msg = peer_id.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                
                tokio::spawn(async move {
                    let verification_task = tokio::task::spawn_blocking(move || {
                        if let Some(expected) = expected_hash {
                            let hash = sha1::Sha1::digest(&data);
                            if hash.as_slice() == expected.as_slice() {
                                return Ok(data); 
                            }
                        }
                        Err(()) 
                    });

                    let result = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => return, 
                        res = verification_task => res.unwrap_or(Err(())),
                    };

                    match result {
                        Ok(verified_data) => {
                            let _ = tx.send(TorrentCommand::PieceVerified {
                                piece_index,
                                peer_id: peer_id_for_msg, 
                                verification_result: Ok(verified_data) 
                            }).await;
                        }
                        _ => {
                            let _ = tx.send(TorrentCommand::PieceVerified {
                                piece_index,
                                peer_id: peer_id_for_msg, 
                                verification_result: Err(()) 
                            }).await;
                        }
                    }
                });
            }

            Effect::WriteToDisk { peer_id, piece_index, data } => {
                let multi_file_info = self.multi_file_info.as_ref().expect("File info missing").clone();
                let piece_length = self.state.torrent.as_ref().unwrap().info.piece_length as u64;
                let global_offset = piece_index as u64 * piece_length;
                
                let tx = self.torrent_manager_tx.clone();
                let event_tx = self.manager_event_tx.clone();
                let resource_manager = self.resource_manager.clone();
                let info_hash = self.state.info_hash.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                let peer_id_clone = peer_id.clone(); 

                tokio::spawn(async move {
                    let op = DiskIoOperation {
                        piece_index, offset: global_offset, length: data.len()
                    };
                    let _ = event_tx.try_send(ManagerEvent::DiskWriteStarted { info_hash, op });

                    let mut attempt = 0;
                    loop {
                        let permit_result = tokio::select! {
                            biased;
                            _ = shutdown_rx.recv() => return ,
                            res = resource_manager.acquire_disk_write() => res,
                        };
                        
                        match permit_result {
                            Ok(_p) => {
                                let write_future = write_data_to_disk(&multi_file_info, global_offset, &data);
                                let res = tokio::select! {
                                    biased;
                                    _ = shutdown_rx.recv() => return,
                                    r = write_future => r,
                                };

                                match res {
                                    Ok(_) => {
                                        let _ = tx.send(TorrentCommand::PieceWrittenToDisk { 
                                            peer_id: peer_id_clone.clone(), 
                                            piece_index 
                                        }).await;
                                        let _ = event_tx.try_send(ManagerEvent::DiskWriteFinished);
                                        return;
                                    }
                                    Err(e) => {
                                        event!(Level::WARN, "Disk write failed for piece {}: {}", piece_index, e);
                                    }
                                }
                            }
                            Err(ResourceManagerError::QueueFull) => {
                                event!(Level::DEBUG, "Disk write queue full. Retrying...");
                            }
                            Err(ResourceManagerError::ManagerShutdown) => return,
                        }
                        
                        attempt += 1;
                        if attempt > MAX_PIECE_WRITE_ATTEMPTS {
                            let _ = tx.send(TorrentCommand::PieceWriteFailed { piece_index }).await;
                            let _ = event_tx.try_send(ManagerEvent::DiskWriteFinished);
                            return;
                        }

                        let backoff = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
                        let jitter = rand::rng().random_range(0..=JITTER_MS);
                        let _ = event_tx.try_send(ManagerEvent::DiskIoBackoff { duration: Duration::from_millis(backoff + jitter) });

                        tokio::select! {
                            biased;
                            _ = shutdown_rx.recv() => return ,
                            _ = tokio::time::sleep(Duration::from_millis(backoff + jitter)) => {},
                        }
                    }
                });
            }

            Effect::ReadFromDisk { peer_id, block_info } => {
                let (peer_semaphore, peer_tx) = if let Some(peer) = self.state.peers.get(&peer_id) {
                     (peer.upload_slots_semaphore.clone(), peer.peer_tx.clone())
                } else {
                    return; 
                };

                let _peer_permit = match peer_semaphore.try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => return,
                };

                let multi_file_info = self.multi_file_info.as_ref().expect("File info missing").clone();
                let torrent = self.state.torrent.as_ref().unwrap();
                let global_offset = (block_info.piece_index as u64 * torrent.info.piece_length as u64) + block_info.offset as u64;
                
                let tx = self.torrent_manager_tx.clone();
                let event_tx = self.manager_event_tx.clone();
                let resource_manager = self.resource_manager.clone();
                let info_hash = self.state.info_hash.clone();
                let mut shutdown_rx = self.shutdown_tx.subscribe();
                let peer_id_clone = peer_id.clone();
                let block_info_clone = block_info.clone();

                let handle = tokio::spawn(async move {
                     let _held_permit = _peer_permit; 
                     let op = DiskIoOperation {
                        piece_index: block_info.piece_index, offset: global_offset, length: block_info.length as usize
                    };

                    let result = Self::read_block_with_retry(
                        &multi_file_info, &resource_manager, &mut shutdown_rx, &event_tx, &info_hash, op
                    ).await;

                    if let Ok(data) = result {
                         let _ = peer_tx.try_send(TorrentCommand::Upload(
                            block_info.piece_index, block_info.offset, data
                        ));
                        let _ = event_tx.try_send(ManagerEvent::BlockSent { info_hash: info_hash.clone() });
                    }

                    let _ = tx.try_send(TorrentCommand::UploadTaskCompleted { 
                        peer_id: peer_id_clone, block_info: block_info_clone 
                    });
                });

                self.in_flight_uploads.entry(peer_id).or_default().insert(block_info, handle);
            }

        }
    }

    async fn read_block_with_retry(
        multi_file_info: &MultiFileInfo,
        resource_manager: &ResourceManagerClient,
        shutdown_rx: &mut broadcast::Receiver<()>,
        event_tx: &Sender<ManagerEvent>,
        info_hash: &[u8],
        op: DiskIoOperation,
    ) -> Result<Vec<u8>, StorageError> {
        let mut attempt = 0;
        let _ = event_tx.try_send(ManagerEvent::DiskReadStarted { info_hash: info_hash.to_vec(), op });

        loop {
            let permit_res = tokio::select! {
                biased;
                _ = shutdown_rx.recv() => { return Err(StorageError::Io(std::io::Error::other("Shutdown"))); }
                res = resource_manager.acquire_disk_read() => res,
            };

            match permit_res {
                Ok(_permit) => {
                    let read_future = read_data_from_disk(multi_file_info, op.offset, op.length);
                    let res = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => { return Err(StorageError::Io(std::io::Error::other("Shutdown"))); }
                        r = read_future => r,
                    };

                    match res {
                        Ok(data) => {
                            let _ = event_tx.try_send(ManagerEvent::DiskReadFinished);
                            return Ok(data);
                        }
                        Err(_) => { /* Fallthrough to retry */ }
                    }
                }
                Err(ResourceManagerError::ManagerShutdown) => return Err(StorageError::Io(std::io::Error::other("Manager Shutdown"))),
                Err(ResourceManagerError::QueueFull) => { /* Fallthrough to retry */ }
            }

            attempt += 1;
            if attempt > MAX_UPLOAD_REQUEST_ATTEMPTS {
                let _ = event_tx.try_send(ManagerEvent::DiskReadFinished);
                return Err(StorageError::Io(std::io::Error::other("Max read attempts exceeded")));
            }

            let backoff = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
            let jitter = rand::rng().random_range(0..=JITTER_MS);
            
            let _ = event_tx.try_send(ManagerEvent::DiskIoBackoff { duration: Duration::from_millis(backoff + jitter) });

            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => { return Err(StorageError::Io(std::io::Error::other("Shutdown"))); }
                _ = tokio::time::sleep(Duration::from_millis(backoff + jitter)) => {},
            }
        }
    }

    #[cfg(feature = "dht")]
    fn spawn_dht_lookup_task(&mut self) {
        if let Some(handle) = self.dht_task_handle.take() {
            handle.abort();
        }

        let dht_tx_clone = self.dht_tx.clone();
        let dht_handle_clone = self.dht_handle.clone();
        let mut dht_trigger_rx = self.dht_trigger_tx.subscribe();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        if let Ok(info_hash_id) = Id::from_bytes(self.state.info_hash.clone()) {
            let handle = tokio::spawn(async move {
                loop {
                    event!(Level::DEBUG, "DHT task loop running");
                    let mut peers_stream = dht_handle_clone.get_peers(info_hash_id);
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            event!(Level::DEBUG, "DHT task shutting down.");
                            break;
                        }

                        _ = async {
                            while let Some(peer) = peers_stream.next().await {
                                if dht_tx_clone.send(peer).await.is_err() {
                                    return;
                                }
                            }
                        } => {}
                    }

                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            event!(Level::DEBUG, "DHT task shutting down.");
                            break;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(300)) => {}
                        _ = dht_trigger_rx.changed() => {}
                    }
                }
            });
            self.dht_task_handle = Some(handle);
        }
    }

    async fn sleep_with_shutdown(
        duration: Duration,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<(), ()> {
        tokio::select! {
            biased; // Prioritize shutdown
            _ = shutdown_rx.recv() => Err(()),
            _ = tokio::time::sleep(duration) => Ok(()),
        }
    }

    fn generate_bitfield(&mut self) -> Vec<u8> {
        let num_pieces = self.state.piece_manager.bitfield.len();
        let num_bytes = num_pieces.div_ceil(8);
        let mut bitfield_bytes = vec![0u8; num_bytes];

        for (piece_index, status) in self.state.piece_manager.bitfield.iter().enumerate() {
            if *status == PieceStatus::Done {
                let byte_index = piece_index / 8;
                let bit_index_in_byte = piece_index % 8;
                let mask = 1 << (7 - bit_index_in_byte);
                bitfield_bytes[byte_index] |= mask;
            }
        }

        bitfield_bytes
    }

    pub async fn connect_to_peer(&mut self, peer_ip: String, peer_port: u16) {
        let _ = self
            .manager_event_tx
            .try_send(ManagerEvent::PeerDiscovered {
                info_hash: self.state.info_hash.clone(),
            });

        let peer_ip_port = format!("{}:{}", peer_ip, peer_port);

        if let Some((failure_count, next_attempt_time)) = self.state.timed_out_peers.get(&peer_ip_port) {
            if Instant::now() < *next_attempt_time {
                event!(Level::DEBUG, peer = %peer_ip_port, failures = %failure_count, "Ignoring connection attempt, peer is on exponential backoff.");
                return;
            }
        }

        if self.state.peers.contains_key(&peer_ip_port) {
            event!(
                Level::TRACE,
                peer_ip_port,
                "PEER SESSION ALREADY ESTABLISHED"
            );
            return;
        }

        let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
        let resource_manager_clone = self.resource_manager.clone();
        let global_dl_bucket_clone = self.global_dl_bucket.clone();
        let global_ul_bucket_clone = self.global_ul_bucket.clone();
        let info_hash_clone = self.state.info_hash.clone();
        let torrent_metadata_length_clone = self.state.torrent_metadata_length;
        let peer_ip_port_clone = peer_ip_port.clone();

        let mut shutdown_rx_permit = self.shutdown_tx.subscribe();
        let mut shutdown_rx_session = self.shutdown_tx.subscribe();
        let shutdown_tx = self.shutdown_tx.clone();

        let (peer_session_tx, peer_session_rx) = mpsc::channel::<TorrentCommand>(10);
        self.state.peers.insert(
            peer_ip_port.clone(),
            PeerState::new(peer_ip_port.clone(), peer_session_tx),
        );

        let bitfield = match self.state.torrent {
            None => None,
            _ => Some(self.generate_bitfield()),
        };

        let client_id_clone = self.settings.client_id.clone();
        tokio::spawn(async move {
            let session_permit = tokio::select! {
                permit_result = resource_manager_clone.acquire_peer_connection() => {
                    match permit_result {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            event!(Level::DEBUG, "Failed to acquire permit. Manager shut down?");
                            None
                        }
                    }
                }
                _ = shutdown_rx_permit.recv() => {
                    event!(Level::DEBUG, "PEER SESSION {}: Shutting down before permit acquired.", &peer_ip_port_clone);
                    None
                }
            };

            if let Some(session_permit) = session_permit {
                let connection_result = timeout(
                    Duration::from_secs(2),
                    TcpStream::connect(&peer_ip_port_clone),
                )
                .await;

                if let Ok(Ok(stream)) = connection_result {
                    let _held_session_permit = session_permit;
                    let session = PeerSession::new(PeerSessionParameters {
                        info_hash: info_hash_clone,
                        torrent_metadata_length: torrent_metadata_length_clone,
                        connection_type: ConnectionType::Outgoing,
                        torrent_manager_rx: peer_session_rx,
                        torrent_manager_tx: torrent_manager_tx_clone.clone(),
                        peer_ip_port: peer_ip_port_clone.clone(),
                        client_id: client_id_clone.into(),
                        global_dl_bucket: global_dl_bucket_clone,
                        global_ul_bucket: global_ul_bucket_clone,
                        shutdown_tx,
                    });

                    tokio::select! {
                        session_result = session.run(stream, Vec::new(), bitfield) => {
                            if let Err(e) = session_result {
                                event!(
                                    Level::DEBUG,
                                    "PEER SESSION {}: ENDED IN ERROR: {}",
                                    &peer_ip_port_clone,
                                    e
                                );
                            }
                        }
                        _ = shutdown_rx_session.recv() => {
                            event!(
                                Level::DEBUG,
                                "PEER SESSION {}: Shutting down due to manager signal.",
                                &peer_ip_port_clone
                            );
                        }
                    }
                } else {
                    let _ = torrent_manager_tx_clone
                        .send(TorrentCommand::UnresponsivePeer(peer_ip_port))
                        .await;
                    event!(Level::DEBUG, peer = %peer_ip_port_clone, "PEER TIMEOUT or connection refused");
                }
            }

            let _ = torrent_manager_tx_clone
                .send(TorrentCommand::Disconnect(peer_ip_port_clone))
                .await;
        });
    }

    /// Announces to trackers to get a list of peers and then attempts to connect to them.
    pub async fn connect_to_tracker_peers(&mut self) {
        let torrent_size_left = self
            .multi_file_info
            .as_ref()
            .map_or(1, |info| info.total_size as usize);

        let mut peers = HashSet::new();

        for url in self.state.trackers.keys() {
            let info_hash_clone = self.state.info_hash.clone();
            let client_port_clone = self.settings.client_port;
            let client_id_clone = self.settings.client_id.clone();
            let tracker_response = announce_started(
                url.to_string(),
                &info_hash_clone,
                client_id_clone,
                client_port_clone,
                torrent_size_left,
            )
            .await;

            match tracker_response {
                Ok(value) => {
                    for peer in value.peers {
                        peers.insert((peer.ip, peer.port));
                    }
                }
                Err(e) => {
                    event!(Level::DEBUG, ?e);
                }
            }
        }

        for peer in peers {
            self.connect_to_peer(peer.0, peer.1).await;
        }
    }

    /// Verifies the integrity of the torrent's data on disk by checking each piece against the
    /// hashes in the torrent metadata. This is done at startup to determine which pieces are
    /// already downloaded and correct.
    pub async fn validate_local_file(&mut self) -> Result<(), StorageError> {
        let torrent = self.state.torrent.clone().expect("Torrent metadata not ready.");

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let manager_event_tx_clone = self.manager_event_tx.clone();

        if self.state.torrent_validation_status {
            for piece_index in 0..self.state.piece_manager.bitfield.len() {
                self.state.piece_manager.mark_as_complete(piece_index as u32);
            }
        } else {
            let multi_file_info = match &self.multi_file_info {
                Some(info) => info.clone(),
                None => return Ok(()),
            };

            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_rx.recv() => {
                    event!(Level::INFO, "Shutdown signal received during file allocation. Aborting validation.");
                    return Ok(());
                }
                res = create_and_allocate_files(&multi_file_info) => res?,
            };

            let piece_length_u64 = torrent.info.piece_length as u64;
            let num_pieces = self.state.piece_manager.bitfield.len();

            for piece_index in 0..num_pieces {
                let start_offset = (piece_index as u64) * piece_length_u64;
                let len_this_piece = self.get_piece_size(piece_index as u32);

                if len_this_piece == 0 {
                    continue;
                }

                let start_hash_index = piece_index * HASH_LENGTH;
                let end_hash_index = start_hash_index + HASH_LENGTH;
                let expected_hash = torrent
                    .info
                    .pieces
                    .get(start_hash_index..end_hash_index)
                    .map(|s| s.to_vec());

                let mut attempt = 0;

                let piece_data = loop {
                    let disk_permit_result = tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => {
                            event!(Level::INFO, "Shutdown signal received while acquiring disk permit. Aborting validation.");
                            return Ok(());
                        }
                        acquire_result = self.resource_manager.acquire_disk_read() => acquire_result
                    };

                    match disk_permit_result {
                        Ok(_permit) => {
                            let read_result = tokio::select! {
                                biased;
                                _ = shutdown_rx.recv() => {
                                    event!(Level::INFO, "Shutdown signal received during disk read. Aborting validation.");
                                    Err(StorageError::Io(std::io::Error::other("Shutdown during read")))
                                }
                                res = read_data_from_disk(&multi_file_info, start_offset, len_this_piece) => res
                            };

                            match read_result {
                                Ok(data) => {
                                    break data;
                                }
                                Err(e) => {
                                    event!(Level::WARN, piece = piece_index, error = %e, "Read from disk failed during validation.");
                                }
                            }
                        }
                        Err(ResourceManagerError::QueueFull) => {
                            event!(Level::DEBUG, "Disk read queue full during validation.");
                        }
                        Err(ResourceManagerError::ManagerShutdown) => {
                            event!(
                                Level::WARN,
                                "Resource manager shut down. Aborting validation."
                            );
                            return Ok(());
                        }
                    }

                    if attempt >= MAX_VALIDATION_ATTEMPTS {
                        event!(
                            Level::ERROR,
                            piece = piece_index,
                            "Validation read failed after {} attempts. Giving up.",
                            MAX_VALIDATION_ATTEMPTS
                        );
                        return Ok(());
                    }

                    let backoff_duration_ms = BASE_BACKOFF_MS.saturating_mul(2u64.pow(attempt));
                    let jitter = rand::rng().random_range(0..=JITTER_MS);
                    let total_delay = Duration::from_millis(backoff_duration_ms + jitter);
                    attempt += 1;

                    let _ = manager_event_tx_clone.try_send(ManagerEvent::DiskIoBackoff {
                        duration: total_delay,
                    });
                    event!(
                        Level::WARN,
                        piece = piece_index,
                        "Retrying validation read in {:?} (Attempt {})...",
                        total_delay,
                        attempt
                    );

                    if Self::sleep_with_shutdown(total_delay, &mut shutdown_rx)
                        .await
                        .is_err()
                    {
                        event!(Level::INFO, "Shutdown signal received while waiting to retry disk read. Aborting validation.");
                        return Ok(());
                    }
                };

                let mut validation_task = tokio::task::spawn_blocking(move || {
                    if let Some(expected) = expected_hash {
                        sha1::Sha1::digest(&piece_data).as_slice() == expected.as_slice()
                    } else {
                        false
                    }
                });

                let validation_result = tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => {
                        event!(Level::INFO, "Shutdown signal received during hash validation. Aborting validation.");
                        validation_task.abort();
                        return Ok(());
                    }
                    join_handle_result = &mut validation_task => join_handle_result
                };

                if let Ok(is_valid) = validation_result {
                    if is_valid {
                        self.state.piece_manager.mark_as_complete(piece_index as u32);
                    }
                } else {
                    event!(
                        Level::WARN,
                        "Hash validation task failed to complete. Aborting validation."
                    );
                    return Ok(());
                }

                if piece_index % 20 == 0 {
                    if let Some(ref torrent) = self.state.torrent {
                        let metrics_tx_clone = self.metrics_tx.clone();
                        let info_hash_clone = self.state.info_hash.clone();
                        let torrent_name_clone = torrent.info.name.clone();
                        let number_of_pieces_total = (torrent.info.pieces.len() / 20) as u32;
                        let number_of_pieces_completed = (piece_index + 1) as u32;

                        let torrent_state = TorrentMetrics {
                            info_hash: info_hash_clone,
                            torrent_name: torrent_name_clone,
                            number_of_pieces_total,
                            number_of_pieces_completed,
                            activity_message: "Validating local files...".to_string(),
                            ..Default::default()
                        };

                        if let Err(e) = metrics_tx_clone.send(torrent_state) {
                            tracing::event!(
                                Level::ERROR,
                                "Failed to send validation metrics to TUI: {}",
                                e
                            );
                        }
                    }
                }
            }
        }

        self.apply_action(Action::CheckCompletion);
        Ok(())
    }

    /// Calculates the size of a specific piece. Most pieces have a fixed size, but the last
    /// piece is often smaller.
    fn get_piece_size(&self, piece_index: u32) -> usize {
        let torrent = self.state.torrent.clone().expect("Torrent metadata not ready.");
        let multi_file_info = self.multi_file_info.as_ref().expect("File info not ready.");

        let total_length_u64 = multi_file_info.total_size;
        let piece_length_u64 = torrent.info.piece_length as u64;
        let piece_index_u64 = piece_index as u64;
        let start_offset = piece_index_u64 * piece_length_u64;
        let bytes_remaining = total_length_u64.saturating_sub(start_offset);

        std::cmp::min(piece_length_u64, bytes_remaining) as usize
    }
    /// Generates a human-readable status message for the UI based on the torrent's current state.
    fn generate_activity_message(&self, dl_speed: u64, ul_speed: u64) -> String {
        if self.state.is_paused {
            return "Paused".to_string();
        }

        if self.state.torrent_status == TorrentStatus::Done {
            return if ul_speed > 0 {
                "Seeding".to_string()
            } else {
                "Finished".to_string()
            };
        }

        if dl_speed > 0 {
            return match &self.state.last_activity {
                TorrentActivity::DownloadingPiece(p) => format!("Receiving piece #{}", p),
                TorrentActivity::VerifyingPiece(p) => format!("Verifying piece #{}", p),
                _ => "Downloading".to_string(),
            };
        }

        if ul_speed > 0 {
            return match &self.state.last_activity {
                TorrentActivity::SendingPiece(p) => format!("Sending piece #{}", p),
                _ => "Uploading".to_string(),
            };
        }

        if !self.state.peers.is_empty() {
            return "Stalled".to_string();
        }

        match &self.state.last_activity {
            #[cfg(feature = "dht")]
            TorrentActivity::SearchingDht => "Searching DHT for peers...".to_string(),
            TorrentActivity::AnnouncingToTracker => "Contacting tracker...".to_string(),
            _ => "Connecting to peers...".to_string(),
        }
    }

    fn send_metrics(&mut self, _actual_ms_since_last_tick: u64, bytes_dl: u64, bytes_ul: u64) {
        if let Some(ref torrent) = self.state.torrent {
            let multi_file_info = self.multi_file_info.as_ref().expect("File info not ready.");

            let next_announce_in = self
                .state.trackers
                .values()
                .map(|t| t.next_announce_time)
                .min()
                .map_or(Duration::MAX, |t| {
                    t.saturating_duration_since(Instant::now())
                });


            let smoothed_total_dl_speed = self.state.total_dl_prev_avg_ema as u64;
            let smoothed_total_ul_speed = self.state.total_ul_prev_avg_ema as u64;
            
            let bytes_downloaded_this_tick = bytes_dl;
            let bytes_uploaded_this_tick = bytes_ul;

            let activity_message =
                self.generate_activity_message(smoothed_total_dl_speed, smoothed_total_ul_speed);

            let metrics_tx_clone = self.metrics_tx.clone();
            let info_hash_clone = self.state.info_hash.clone();
            let torrent_name_clone = torrent.info.name.clone();
            let number_of_pieces_total = (torrent.info.pieces.len() / 20) as u32;
            let number_of_pieces_completed =
                number_of_pieces_total - self.state.piece_manager.pieces_remaining as u32;
            let number_of_successfully_connected_peers = self.state.peers.len();

            let eta = if self.state.piece_manager.pieces_remaining == 0 {
                Duration::from_secs(0)
            } else if smoothed_total_dl_speed == 0 {
                Duration::MAX
            } else {
                let total_size_bytes = multi_file_info.total_size;
                let bytes_completed = (torrent.info.piece_length as u64).saturating_mul(
                    self.state.piece_manager
                        .bitfield
                        .iter()
                        .filter(|&s| *s == PieceStatus::Done)
                        .count() as u64,
                );
                let bytes_remaining = total_size_bytes.saturating_sub(bytes_completed);
                let eta_seconds = (bytes_remaining * 8) / smoothed_total_dl_speed;
                Duration::from_secs(eta_seconds)
            };

            let peers_info: Vec<PeerInfo> = self
                .state.peers
                .values()
                .map(|p| {
                    let base_action_str = match &p.last_action {
                        TorrentCommand::SuccessfullyConnected(id) if id.is_empty() => {
                            "Connecting...".to_string()
                        }
                        TorrentCommand::SuccessfullyConnected(_) => {
                            "Exchanged Handshake".to_string()
                        }
                        TorrentCommand::PeerBitfield(_, _) => "Exchanged Bitfield".to_string(),
                        TorrentCommand::Choke(_) => "Choked Us".to_string(),
                        TorrentCommand::Unchoke(_) => "Unchoked Us".to_string(),
                        TorrentCommand::Disconnect(_) => "Disconnected".to_string(),
                        TorrentCommand::Have(_, _) => "Peer Has New Piece".to_string(),
                        TorrentCommand::Block(_, _, _, _) => "Receiving From Peer".to_string(),
                        TorrentCommand::RequestUpload(_, _, _, _) => {
                            "Peer is Requesting".to_string()
                        }
                        TorrentCommand::Cancel(_) => "Peer Canceling Request".to_string(),
                        _ => "Idle".to_string(),
                    };
                    let discriminant = std::mem::discriminant(&p.last_action);
                    let count = p.action_counts.get(&discriminant).unwrap_or(&0);
                    let final_action_str = if *count > 0 {
                        format!("{} (x{})", base_action_str, count)
                    } else {
                        base_action_str
                    };

                    PeerInfo {
                        address: p.ip_port.clone(),
                        peer_id: p.peer_id.clone(),
                        am_choking: p.am_choking != ChokeStatus::Unchoke,
                        peer_choking: p.peer_choking != ChokeStatus::Unchoke,
                        am_interested: p.am_interested,
                        peer_interested: p.peer_is_interested_in_us,
                        bitfield: p.bitfield.clone(),
                        download_speed_bps: p.download_speed_bps,
                        upload_speed_bps: p.upload_speed_bps,
                        total_downloaded: p.total_bytes_downloaded,
                        total_uploaded: p.total_bytes_uploaded,
                        last_action: final_action_str,
                    }
                })
                .collect();

            let total_size_bytes = multi_file_info.total_size;
            let bytes_written = if number_of_pieces_completed == number_of_pieces_total {
                total_size_bytes
            } else {
                (number_of_pieces_completed as u64) * torrent.info.piece_length as u64
            };

            let torrent_state = TorrentMetrics {
                info_hash: info_hash_clone,
                torrent_name: torrent_name_clone,
                number_of_successfully_connected_peers,
                number_of_pieces_total,
                number_of_pieces_completed,
                download_speed_bps: smoothed_total_dl_speed,
                upload_speed_bps: smoothed_total_ul_speed,
                bytes_downloaded_this_tick,
                bytes_uploaded_this_tick,
                eta,
                peers: peers_info,
                activity_message,
                next_announce_in,
                total_size: total_size_bytes,
                bytes_written,
                ..Default::default()
            };
            tokio::spawn(async move {
                if let Err(e) = metrics_tx_clone.send(torrent_state) {
                    tracing::event!(Level::ERROR, "Failed to send metrics to TUI: {}", e);
                }
            });
        }
    }

    pub async fn run(mut self, is_paused: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.state.is_paused = is_paused;

        if self.state.torrent.is_some() {
            if let Err(error) = self.validate_local_file().await {
                match error {
                    StorageError::Io(e) => {
                        eprintln!("Error calling validate local file: {}", e);
                    }
                }
            }
        }

        if !self.state.is_paused {
            event!(
                Level::DEBUG,
                "Performing initial 'started' announce to trackers..."
            );
            let torrent_size_left = self
                .multi_file_info
                .as_ref()
                .map_or(0, |mfi| mfi.total_size as usize);

            for url in self.state.trackers.keys() {
                let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
                let url_clone = url.clone();
                let info_hash_clone = self.state.info_hash.clone();
                let client_port_clone = self.settings.client_port;

                let client_id_clone = self.settings.client_id.clone();

                tokio::spawn(async move {
                    let response = announce_started(
                        url_clone.clone(),
                        &info_hash_clone,
                        client_id_clone,
                        client_port_clone,
                        torrent_size_left,
                    )
                    .await;

                    match response {
                        Ok(resp) => {
                            let _ = torrent_manager_tx_clone
                                .send(TorrentCommand::AnnounceResponse(url_clone, resp))
                                .await;
                        }
                        Err(e) => {
                            let _ = torrent_manager_tx_clone
                                .send(TorrentCommand::AnnounceFailed(url_clone, e.to_string()))
                                .await;
                        }
                    }
                });
            }
        }

        #[cfg(feature = "dht")]
        self.spawn_dht_lookup_task();

        let mut data_rate_ms = 1000;
        let mut tick = tokio::time::interval(Duration::from_millis(data_rate_ms));
        let mut last_tick_time = Instant::now();

        let mut cleanup_timer = tokio::time::interval(Duration::from_secs(3));
        let mut pex_timer = tokio::time::interval(Duration::from_secs(75));
        let mut choke_timer = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    println!("Ctrl+C received, initiating clean shutdown...");
                    break Ok(());
                }
                _ = cleanup_timer.tick(), if !self.state.is_paused => {
                    self.state.timed_out_peers.retain(|_, (retry_count, _)| *retry_count < MAX_TIMEOUT_COUNT);

                    if self.state.torrent_status == TorrentStatus::Done {
                        for peer in self.state.peers.values() {
                            let peer_is_fully_seeded = peer.bitfield.iter().all(|&has| has);

                            if peer_is_fully_seeded {
                                let manager_tx_clone = self.torrent_manager_tx.clone();
                                let peer_id_clone = peer.ip_port.clone();
                                tokio::spawn(async move {
                                    let _ = manager_tx_clone.send(TorrentCommand::Disconnect(peer_id_clone)).await;
                                });
                            }
                        }
                    }
                }
                _ = tick.tick(), if !self.state.is_paused => {

                    let now = Instant::now();
                    let actual_duration = now.duration_since(last_tick_time);
                    last_tick_time = now;
                    let actual_ms = actual_duration.as_millis() as u64;

                    let mut trackers_to_announce = Vec::new();

                    for (url, tracker_state) in &self.state.trackers {
                        if now >= tracker_state.next_announce_time {
                            trackers_to_announce.push(url.clone());
                        }
                    }

                    if !trackers_to_announce.is_empty() {
                        let mut torrent_size_left = 1;
                        if let Some(torrent) = &self.state.torrent {
                            torrent_size_left = torrent.info.length as usize;
                            if !torrent.info.files.is_empty() {
                                torrent_size_left = torrent.info.files.iter().map(|file| file.length as usize).sum();
                            }
                        }
                        for url in trackers_to_announce {
                            if let Some(tracker_state) = self.state.trackers.get_mut(&url) {
                                tracker_state.next_announce_time = now + Duration::from_secs(2048 * 2);
                                let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
                                let url_clone = url.clone();
                                let info_hash_clone = self.state.info_hash.clone();
                                let client_port_clone = self.settings.client_port;
                                let client_id_clone = self.settings.client_id.clone();
                                let session_total_uploaded_clone = self.state.session_total_uploaded as usize;
                                let session_total_downloaded_clone = self.state.session_total_downloaded as usize;
                                tokio::spawn(async move {
                                    let tracker_response = announce_periodic(
                                        url.to_string(),
                                        &info_hash_clone,
                                        client_id_clone,
                                        client_port_clone,
                                        session_total_uploaded_clone,
                                        session_total_downloaded_clone,
                                        torrent_size_left,
                                    ).await;

                                    match tracker_response {
                                        Ok(response) => {
                                            let _ = torrent_manager_tx_clone.send(TorrentCommand::AnnounceResponse(url_clone, response)).await;
                                        },
                                        Err(e) => {
                                            let _ = torrent_manager_tx_clone.send(TorrentCommand::AnnounceFailed(url_clone, e.to_string())).await;
                                        }
                                    }
                                });
                            }
                        }
                    }


                    if self.state.torrent_status == TorrentStatus::Endgame {
                        let peer_ids: Vec<String> = self.state.peers.keys().cloned().collect();
                        for peer_id in peer_ids {
                            if let Some(peer) = self.state.peers.get(&peer_id) {
                                if peer.pending_requests.is_empty() {
                                    self.apply_action(Action::AssignWork { peer_id: peer_id.clone() });
                                }
                            }
                        }
                    }

                    self.apply_action(Action::Tick { dt_ms: actual_ms });
                }

                _ = choke_timer.tick(), if !self.state.is_paused => {
                    if self.state.torrent_status != TorrentStatus::Done {
                        let peer_bitfields = self.state.peers.values().map(|p| &p.bitfield);
                        self.state.piece_manager.update_rarity(peer_bitfields);
                    }

                    self.apply_action(Action::RecalculateChokes { 
                        upload_slots: self.settings.upload_slots 
                    });
                }

                _ = pex_timer.tick(), if !self.state.is_paused => {
                    if self.state.peers.len() < 2 {
                        continue;
                    }

                    let all_peer_ips: Vec<String> = self.state.peers.keys().cloned().collect();

                    for peer_state in self.state.peers.values() {
                        let peer_tx = peer_state.peer_tx.clone();
                        let peers_list = all_peer_ips.clone();

                        let _ = peer_tx.try_send(
                            TorrentCommand::SendPexPeers(peers_list)
                        );
                    }
                }

                Some(manager_command) = self.manager_command_rx.recv() => {
                    event!(Level::TRACE, ?manager_command);
                    match manager_command {
                        ManagerCommand::SetDataRate(new_rate_ms) => {
                            data_rate_ms = new_rate_ms;
                            tick = tokio::time::interval(Duration::from_millis(data_rate_ms));
                            tick.reset();
                            last_tick_time = Instant::now();
                        },
                        ManagerCommand::Pause => {
                            self.state.last_activity = TorrentActivity::Paused;
                            self.state.is_paused = true;

                            for peer in self.state.peers.values() {
                                let peer_tx = peer.peer_tx.clone();
                                let peer_ip_port = peer.ip_port.clone();
                                let _ = peer_tx.try_send(TorrentCommand::Disconnect(peer_ip_port));
                            }

                            self.state.last_known_peers = self.state.peers.keys().cloned().collect();
                            self.state.peers.clear();

                            self.state.bytes_downloaded_in_interval = 0;
                            self.state.bytes_uploaded_in_interval = 0;
                            self.send_metrics(data_rate_ms, 0, 0);

                            event!(Level::INFO, info_hash = %BASE32.encode(&self.state.info_hash), "Torrent paused. Disconnected from all peers.");

                        },
                        ManagerCommand::Resume => {
                            self.state.last_activity = TorrentActivity::ConnectingToPeers;
                            self.state.is_paused = false;
                            event!(Level::INFO, info_hash = %BASE32.encode(&self.state.info_hash), "Torrent resumed. Re-announcing to trackers.");

                            #[cfg(feature = "dht")]
                            let _ = self.dht_trigger_tx.send(());

                            for peer_addr in std::mem::take(&mut self.state.last_known_peers) {
                                if let Ok(socket_addr) = peer_addr.parse::<std::net::SocketAddr>() {
                                    self.connect_to_peer(socket_addr.ip().to_string(), socket_addr.port()).await;
                                }
                            }
                            for tracker_state in self.state.trackers.values_mut() {
                                tracker_state.next_announce_time = Instant::now();
                            }
                        },
                        ManagerCommand::Shutdown => {
                            event!(Level::INFO, info_hash = %BASE32.encode(&self.state.info_hash), "Torrent shutting down.");
                            self.state.is_paused = true;
                            let _ = self.shutdown_tx.send(());

                            event!(Level::DEBUG, "Aborting all in-flight upload tasks...");
                            for (_peer_id, handles_map) in self.in_flight_uploads.iter() {
                                for (block_info, handle) in handles_map.iter() {
                                    event!(Level::TRACE, ?block_info, "Aborting task");
                                    handle.abort();
                                }
                            }
                            self.in_flight_uploads.clear();
                            event!(Level::DEBUG, "All upload tasks aborted.");

                            if let (Some(torrent), Some(multi_file_info)) = (&self.state.torrent, &self.multi_file_info) {
                                let total_size_bytes = multi_file_info.total_size;
                                let bytes_completed = (torrent.info.piece_length as u64).saturating_mul(
                                    self.state.piece_manager
                                        .bitfield
                                        .iter()
                                        .filter(|&s| *s == PieceStatus::Done)
                                        .count() as u64,
                                );
                                let bytes_left = total_size_bytes.saturating_sub(bytes_completed);
                                let mut announce_set = JoinSet::new();
                                for url in self.state.trackers.keys() {
                                    let url_clone = url.clone();
                                    let info_hash_clone = self.state.info_hash.clone();
                                    let client_port_clone = self.settings.client_port;
                                    let client_id_clone = self.settings.client_id.clone();
                                    let session_total_uploaded_clone = self.state.session_total_uploaded as usize;
                                    let session_total_downloaded_clone = self.state.session_total_downloaded as usize;
                                    announce_set.spawn(async move {
                                        announce_stopped(
                                            url_clone,
                                            &info_hash_clone,
                                            client_id_clone,
                                            client_port_clone,
                                            session_total_uploaded_clone,
                                            session_total_downloaded_clone,
                                            bytes_left as usize,
                                        )
                                        .await;
                                    });
                                }
                                event!(Level::DEBUG, "Sending 'stopped' to {} trackers...", announce_set.len());
                                if (tokio::time::timeout(Duration::from_secs(4), async {
                                    while (announce_set.join_next().await).is_some() {
                                    }
                                }).await).is_err() {
                                    event!(Level::WARN, "Tracker announce tasks timed out. Aborting remaining.");
                                    announce_set.abort_all();
                                } else {
                                    event!(Level::DEBUG, "Tracker announces finished.");
                                }
                            }

                            self.state.peers.clear();
                            let _ = self.manager_event_tx.try_send(ManagerEvent::DeletionComplete(self.state.info_hash.clone(), Ok(())));
                            break Ok(());
                        },
                        ManagerCommand::DeleteFile => {
                            let torrent = if let Some(t) = self.state.torrent.clone() {
                                t
                            } else {
                                let error_msg = "Cannot delete files: Torrent metadata has not been downloaded yet (magnet link?).".to_string();
                                event!(Level::ERROR, "{}", error_msg);
                                let _ = self.manager_event_tx.try_send(ManagerEvent::DeletionComplete(self.state.info_hash.clone(), Err(error_msg)));
                                break Ok(());
                            };

                            self.state.peers.clear();
                            let mut event_result = Ok(());

                            if let Some(multi_file_info) = &self.multi_file_info {
                                for file_info in &multi_file_info.files {
                                    if let Err(e) = fs::remove_file(&file_info.path).await {
                                        if e.kind() != std::io::ErrorKind::NotFound {
                                            let error_msg = format!("Failed to delete torrent file {:?}: {}", &file_info.path, e);
                                            event!(Level::ERROR, "{}", error_msg);
                                            event_result = Err(error_msg);
                                            break;
                                        }
                                    }
                                }
                                if event_result.is_ok() && multi_file_info.files.len() > 1 {
                                    let content_dir = self.root_download_path.join(&torrent.info.name);
                                    event!(Level::INFO, "Attempting to clean up directory: {:?}", &content_dir);
                                    let _ = fs::remove_dir(&content_dir).await.ok();
                                }
                            } else {
                                let error_msg = "Could not delete files: torrent metadata unavailable.".to_string();
                                event!(Level::WARN, "{}", error_msg);
                                event_result = Err(error_msg);
                            }
                            let _ = self.manager_event_tx.send(ManagerEvent::DeletionComplete(self.state.info_hash.clone(), event_result)).await;
                            break Ok(());
                        },

                        ManagerCommand::UpdateListenPort(new_port) => {
                            // self.settings is an Arc, so we must clone and replace it
                            let mut settings = (*self.settings).clone();

                            if settings.client_port != new_port {
                                event!(
                                    Level::INFO,
                                    "Listen port updated: {} -> {}. Triggering re-announce.",
                                    settings.client_port,
                                    new_port
                                );
                                settings.client_port = new_port;
                                self.settings = Arc::new(settings); // Update the Arc

                                // Force an immediate re-announce to all trackers with the new port
                                for tracker_state in self.state.trackers.values_mut() {
                                    tracker_state.next_announce_time = Instant::now();
                                }
                            }
                        },
                        #[cfg(feature = "dht")]
                        ManagerCommand::UpdateDhtHandle(new_dht_handle) => {
                            event!(Level::INFO, "DHT handle updated. Restarting DHT lookup task.");
                            self.dht_handle = new_dht_handle;
                            self.spawn_dht_lookup_task();
                        },
                    }
                }

                maybe_peers = async {
                    #[cfg(feature = "dht")]
                    {
                        self.dht_rx.recv().await
                    }
                    #[cfg(not(feature = "dht"))]
                    {
                        std::future::pending().await
                    }
                }, if !self.state.is_paused => {
                    #[cfg(feature = "dht")]
                    {
                        if let Some(peers) = maybe_peers {
                            self.state.last_activity = TorrentActivity::SearchingDht;
                            for peer in peers {
                                event!(Level::DEBUG, "PEER FROM DHT {}", peer);
                                self.connect_to_peer(peer.ip().to_string(), peer.port()).await;
                            }
                        } else {
                            event!(Level::WARN, "DHT channel closed. No longer receiving DHT peers.");
                        }
                    }
                }

                Some((stream, handshake_response)) = self.incoming_peer_rx.recv(), if !self.state.is_paused => {
                    let _ = self.manager_event_tx.try_send(ManagerEvent::PeerDiscovered { info_hash: self.state.info_hash.clone() });
                    if let Ok(peer_addr) = stream.peer_addr() {

                        let peer_ip_port = peer_addr.to_string();
                        event!(Level::DEBUG, peer_addr = %peer_ip_port, "NEW INCOMING PEER CONNECTION");
                        let torrent_manager_tx_clone = self.torrent_manager_tx.clone();
                        let (peer_session_tx, peer_session_rx) = mpsc::channel::<TorrentCommand>(10);

                        if self.state.peers.contains_key(&peer_ip_port) {
                            event!(Level::WARN, peer_ip = %peer_ip_port, "Already connected to this peer. Dropping incoming connection.");
                            continue;
                        }

                        self.state.peers.insert(
                            peer_ip_port.clone(),
                            PeerState::new(peer_ip_port.clone(), peer_session_tx),
                        );

                        let bitfield = match self.state.torrent {
                            None => None,
                            _ => Some(self.generate_bitfield())
                        };
                        let info_hash_clone = self.state.info_hash.clone();
                        let torrent_metadata_length_clone = self.state.torrent_metadata_length;
                        let global_dl_bucket_clone = self.global_dl_bucket.clone();
                        let global_ul_bucket_clone = self.global_ul_bucket.clone();
                        let mut shutdown_rx_manager = self.shutdown_tx.subscribe();
                        let shutdown_tx = self.shutdown_tx.clone();
                        let client_id_clone = self.settings.client_id.clone();

                        let _ = self.manager_event_tx.try_send(ManagerEvent::PeerConnected { info_hash: self.state.info_hash.clone() });
                        tokio::spawn(async move {
                            let session = PeerSession::new(PeerSessionParameters {
                                info_hash: info_hash_clone,
                                torrent_metadata_length: torrent_metadata_length_clone,
                                connection_type: ConnectionType::Incoming,
                                torrent_manager_rx: peer_session_rx,
                                torrent_manager_tx: torrent_manager_tx_clone,
                                peer_ip_port: peer_ip_port.clone(),
                                client_id: client_id_clone.into(),
                                global_dl_bucket: global_dl_bucket_clone,
                                global_ul_bucket: global_ul_bucket_clone,
                                shutdown_tx,
                            });

                            tokio::select! {
                                session_result = session.run(stream, handshake_response, bitfield) => {
                                    if let Err(e) = session_result {
                                        event!(Level::ERROR, peer_ip = %peer_ip_port, error = %e, "Incoming peer session ended with error.");
                                    }
                                }
                                _ = shutdown_rx_manager.recv() => {
                                    event!(
                                        Level::DEBUG,
                                        "INCOMING PEER SESSION {}: Shutting down due to manager signal.",
                                        &peer_ip_port
                                    );
                                }
                            }
                        });
                    } else {
                        event!(Level::DEBUG, "ERROR GETTING PEER ADDRESS FROM STREAM");
                    }
                }

                Some(command) = self.torrent_manager_rx.recv() => {

                    event!(Level::DEBUG, command_summary = ?TorrentCommandSummary(&command));
                    event!(Level::TRACE, ?command);

                    let peer_id_for_action = match &command {
                        TorrentCommand::SuccessfullyConnected(id) => Some(id),
                        TorrentCommand::PeerBitfield(id, _) => Some(id),
                        TorrentCommand::Choke(id) => Some(id),
                        TorrentCommand::Unchoke(id) => Some(id),
                        TorrentCommand::Have(id, _) => Some(id),
                        TorrentCommand::Block(id, _, _, _) => Some(id),
                        TorrentCommand::RequestUpload(id, _, _, _) => Some(id),
                        TorrentCommand::Disconnect(id) => Some(id),
                        _ => None,
                    };
                    if let Some(id) = peer_id_for_action {
                        if let Some(peer) = self.state.peers.get_mut(id) {
                            peer.last_action = command.clone();
                            let discriminant = std::mem::discriminant(&command);
                            *peer.action_counts.entry(discriminant).or_insert(0) += 1;
                        }
                    }

                    match command {
                        TorrentCommand::SuccessfullyConnected(peer_id) => self.apply_action(Action::PeerSuccessfullyConnected { peer_id }),
                        TorrentCommand::PeerId(peer_ip_port, peer_id) => {
                            if let Some(peer) = self.state.peers.get_mut(&peer_ip_port) {
                                peer.peer_id = peer_id;
                            }
                        }
                        TorrentCommand::AddPexPeers(_peer_id, new_peers) => {
                            for peer_tuple in new_peers {
                                self.connect_to_peer(peer_tuple.0, peer_tuple.1).await;
                            }
                        },

                        TorrentCommand::PeerBitfield(pid, bf) => self.apply_action(Action::PeerBitfieldReceived { peer_id: pid, bitfield: bf }),
                        TorrentCommand::Choke(pid) => self.apply_action(Action::PeerChoked { peer_id: pid }),
                        TorrentCommand::Unchoke(pid) => self.apply_action(Action::PeerUnchoked { peer_id: pid }),
                        TorrentCommand::PeerInterested(pid) => self.apply_action(Action::PeerInterested { peer_id: pid }),
                        TorrentCommand::Have(pid, idx) => self.apply_action(Action::PeerHavePiece { peer_id: pid, piece_index: idx }),
                        TorrentCommand::Disconnect(pid) => self.apply_action(Action::PeerDisconnected { peer_id: pid }),
                        TorrentCommand::Block(peer_id, piece_index, block_offset, block_data) => self.apply_action(Action::IncomingBlock { peer_id, piece_index, block_offset, data: block_data }),
                        TorrentCommand::PieceVerified { piece_index, peer_id, verification_result } => {
                            match verification_result {
                                Ok(data) => {
                                    self.apply_action(Action::PieceVerified { 
                                        peer_id, piece_index, valid: true, data 
                                    });
                                }
                                Err(_) => {
                                    self.apply_action(Action::PieceVerified { 
                                        peer_id, piece_index, valid: false, data: Vec::new() 
                                    });
                                }
                            }
                        },

                        TorrentCommand::PieceWrittenToDisk { peer_id, piece_index } => self.apply_action(Action::PieceWrittenToDisk { peer_id, piece_index }),
                        TorrentCommand::PieceWriteFailed { piece_index } => self.apply_action(Action::PieceWriteFailed { piece_index }),
                        TorrentCommand::RequestUpload(peer_id, piece_index, block_offset, block_length) => {
                            if block_length > MAX_BLOCK_SIZE {
                                continue;
                            }
                            self.apply_action(Action::RequestUpload { 
                                peer_id, piece_index, block_offset, length: block_length 
                            });
                        },
                        TorrentCommand::CancelUpload(peer_id, piece_index, block_offset, block_length) => {
                            let block_to_cancel = BlockInfo { piece_index, offset: block_offset, length: block_length };
                            if let Some(peer_uploads) = self.in_flight_uploads.get_mut(&peer_id) {
                                if let Some(handle) = peer_uploads.remove(&block_to_cancel) {
                                    handle.abort();
                                    event!(Level::TRACE, peer = %peer_id, ?block_to_cancel, "Aborted in-flight upload task.");
                                }
                            }
                        },
                        TorrentCommand::UploadTaskCompleted { peer_id, block_info } => {
                            if let Some(peer_uploads) = self.in_flight_uploads.get_mut(&peer_id) {
                                peer_uploads.remove(&block_info);
                            }
                        },
                        TorrentCommand::DhtTorrent(torrent, torrent_metadata_length) => {
                            if self.state.torrent.is_none() {
                                let mut info_dict_hasher = Sha1::new();
                                info_dict_hasher.update(torrent.clone().info_dict_bencode);
                                let dht_info_hash = info_dict_hasher.finalize();

                                if *self.state.info_hash == *dht_info_hash {

                                    #[cfg(all(feature = "dht", feature = "pex"))]
                                    {
                                        // Check if the 'private' key exists and is set to 1
                                        if torrent.info.private == Some(1) {
                                            event!(Level::ERROR, info_hash = %BASE32.encode(&self.state.info_hash), "Rejecting private torrent (from metadata) in normal build.");

                                            let _ = self.manager_event_tx.send(ManagerEvent::DeletionComplete(self.state.info_hash.clone(), Ok(()))).await;
                                            break Ok(());
                                        }
                                    }

                                    self.state.torrent = Some(torrent.clone());
                                    self.state.torrent_metadata_length = Some(torrent_metadata_length);

                                    let multi_file_info = MultiFileInfo::new(
                                        &self.root_download_path,
                                        &torrent.info.name,
                                        if torrent.info.files.is_empty() { None } else { Some(&torrent.info.files) },
                                        if torrent.info.files.is_empty() { Some(torrent.info.length as u64) } else { None },
                                    )
                                    .expect("Failed to create multi-file info from DHT metadata");
                                    self.multi_file_info = Some(multi_file_info);

                                    let pieces_len = torrent.info.pieces.len();
                                    let total_pieces = pieces_len / 20;

                                    self.state.piece_manager.set_initial_fields(pieces_len / 20, self.state.torrent_validation_status);
                                    let bitfield = self.generate_bitfield();

                                    let _ = self.validate_local_file().await;

                                    if let Some(announce) = torrent.announce {
                                        self.state.trackers.insert(announce.clone(), TrackerState {
                                            next_announce_time: Instant::now(),
                                            leeching_interval: None,
                                            seeding_interval: None,
                                        });
                                    }
                                    self.connect_to_tracker_peers().await;

                                    for peer in self.state.peers.values_mut() {
                                        peer.bitfield.resize(total_pieces, false);
                                        let peer_tx_cloned = peer.peer_tx.clone();
                                        let bitfield_clone = bitfield.clone();
                                        let torrent_metadata_length_clone = self.state.torrent_metadata_length;
                                        let _ =
                                            peer_tx_cloned.try_send(TorrentCommand::ClientBitfield(bitfield_clone, torrent_metadata_length_clone));
                                    }
                                }
                            }
                        }
                        TorrentCommand::AnnounceResponse(url, response) => {
                            self.state.last_activity = TorrentActivity::AnnouncingToTracker;
                            for peer in response.peers {
                                self.connect_to_peer(peer.ip, peer.port).await;
                            }

                            if let Some(tracker) = self.state.trackers.get_mut(&url) {
                                let seeding_interval_secs = if response.interval > 0 { (response.interval as u64) + 1 } else { FALLBACK_ANNOUNCE_INTERVAL };
                                tracker.seeding_interval = Some(Duration::from_secs(seeding_interval_secs));

                                let leeching_interval_secs = match response.min_interval {
                                    Some(min) if min > 0 => (min as u64) + 1,
                                    _ => CLIENT_LEECHING_FALLBACK_INTERVAL,
                                };
                                tracker.leeching_interval = Some(Duration::from_secs(leeching_interval_secs));

                                let next_interval = if self.state.torrent_status != TorrentStatus::Done {
                                    tracker.leeching_interval.unwrap()
                                } else {
                                    tracker.seeding_interval.unwrap()
                                };

                                tracker.next_announce_time = Instant::now() + next_interval;
                                event!(Level::DEBUG, tracker = %url, next_announce_in_secs = next_interval.as_secs(), "Announce successful. STATUS {:?}", self.state.torrent_status);
                            }
                        },

                        TorrentCommand::AnnounceFailed(url, error_message) => {
                            if let Some(tracker) = self.state.trackers.get_mut(&url) {

                                let current_interval = if self.state.torrent_status != TorrentStatus::Done {
                                    tracker.leeching_interval.unwrap_or(Duration::from_secs(CLIENT_LEECHING_FALLBACK_INTERVAL))
                                } else {
                                    tracker.seeding_interval.unwrap_or(Duration::from_secs(FALLBACK_ANNOUNCE_INTERVAL))
                                };

                                let backoff_secs = (current_interval.as_secs() * 2).min(FALLBACK_ANNOUNCE_INTERVAL * 2);
                                let backoff_duration = Duration::from_secs(backoff_secs);

                                tracker.next_announce_time = Instant::now() + backoff_duration;
                                event!(Level::DEBUG, tracker = %url, error = %error_message, retry_in_secs = backoff_secs, "Announce failed.");
                            }
                        },

                        TorrentCommand::UnresponsivePeer(peer_ip_port) => {
                            let now = Instant::now();

                            let (failure_count, _) = self.state.timed_out_peers.get(&peer_ip_port).cloned().unwrap_or((0, now));
                            let new_failure_count = (failure_count + 1).min(10);
                            let backoff_duration_secs = (BASE_COOLDOWN_SECS * 2u64.pow(new_failure_count - 1))
                                                        .min(MAX_COOLDOWN_SECS);
                            let next_attempt_time = now + Duration::from_secs(backoff_duration_secs);

                            event!(Level::DEBUG,
                                peer = %peer_ip_port,
                                failures = new_failure_count,
                                cooldown_secs = backoff_duration_secs,
                                "Peer timed out. Applying exponential backoff."
                            );
                            self.state.timed_out_peers.insert(peer_ip_port.clone(), (new_failure_count, next_attempt_time));
                        }
                        _ => {
                            println!("UNIMPLEMENTED TORRENT COMMEND {:?}",  command);
                        }
                    }
                }
            }
        }
    }
}
